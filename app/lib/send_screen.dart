import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';
import 'package:screen_brightness/screen_brightness.dart';
import 'package:wakelock_plus/wakelock_plus.dart';
import 'history_store.dart';
import 'protocol.dart';
import 'src/rust/api/fountain.dart';
import 'src/rust/api/qr.dart';

/// grid ごとの実測ベスト packetSize (EC=M / 版=自動 前提)。
const _packetByGrid = {'1x1': 540, '1x2': 300, '2x2': 180, '2x3': 160, '3x3': 140};
const _grids = ['1x1', '1x2', '2x2', '2x3', '3x3'];
const _ecLevels = ['L', 'M', 'Q', 'H'];
const _manifestEveryFrames = 20; // このフレーム間隔で 1 セルをマニフェストにする

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});
  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  final _textCtrl = TextEditingController();
  final _picker = ImagePicker();
  String? _pickedPath;
  String? _pickedName;
  String _pickedType = 'application/octet-stream';
  int _pickedSize = 0;

  String _grid = '1x2';
  String _ec = 'M';
  int _fps = 12;

  bool _running = false;
  int _seq = 0; // 送信世代 (停止/再開の識別)
  List<QrMatrix> _frame = [];
  String _status = '';

  // 送信状態
  BlockSource? _source;
  StreamManifest? _manifest;
  int _blockIdx = -1;
  FountainEncoder? _enc;
  int _packetInBlock = 0;
  int _frameCount = 0;
  int _pass = 0;

  (int, int) get _dims {
    final p = _grid.split('x');
    return (int.parse(p[0]), int.parse(p[1]));
  }

  Future<void> _pickImage() async {
    // 画像は長辺1600px/品質82に縮小して取り込む (フル解像度は転送が非現実的なため。
    // 原寸で送りたい場合は「ファイル」で選ぶ)。
    final x = await _picker.pickImage(
        source: ImageSource.gallery, maxWidth: 1600, maxHeight: 1600, imageQuality: 82);
    if (x == null) return;
    final len = await x.length();
    setState(() {
      _pickedPath = x.path;
      _pickedName = x.name;
      _pickedType = _imageMime(x.name);
      _pickedSize = len;
    });
  }

  Future<void> _pickFile() async {
    final f = await openFile();
    if (f == null) return;
    final len = await f.length();
    setState(() {
      _pickedPath = f.path;
      _pickedName = f.name;
      _pickedType = f.mimeType ?? 'application/octet-stream';
      _pickedSize = len;
    });
  }

  String _imageMime(String name) {
    final n = name.toLowerCase();
    if (n.endsWith('.png')) return 'image/png';
    if (n.endsWith('.webp')) return 'image/webp';
    if (n.endsWith('.gif')) return 'image/gif';
    return 'image/jpeg';
  }

  BlockSource? _buildSource() {
    if (_pickedPath != null) {
      return FileBlockSource(_pickedPath!, _pickedName ?? 'file.bin', _pickedType, _pickedSize);
    }
    final body = Uint8List.fromList(utf8.encode(_textCtrl.text));
    if (body.isEmpty) return null;
    return MemoryBlockSource(body, 'message.txt', 'text/plain;charset=utf-8');
  }

  Future<void> _start() async {
    final source = _buildSource();
    if (source == null || source.length == 0) {
      setState(() => _status = 'ペイロードが空です');
      return;
    }
    final packetSize = _packetByGrid[_grid] ?? 200;
    final total = source.length;
    final blockCount = (total / kBlockSize).ceil().clamp(1, 1 << 30);

    // 各ブロックの OTI を計算 (full/last)。repair はブロックのシンボル数の 30%。
    final lastLen = total - (blockCount - 1) * kBlockSize;
    final lastBytes = await source.readBlock((blockCount - 1) * kBlockSize, lastLen);
    final otiLast = FountainEncoder(
            payload: lastBytes, packetSize: packetSize, extraRepair: (lastLen / packetSize * 0.3).ceil())
        .otiBytes();
    Uint8List otiFull = otiLast;
    if (blockCount > 1) {
      final fullBytes = await source.readBlock(0, kBlockSize);
      otiFull = FountainEncoder(
              payload: fullBytes,
              packetSize: packetSize,
              extraRepair: (kBlockSize / packetSize * 0.3).ceil())
          .otiBytes();
    }

    _manifest = StreamManifest(
      name: source.name,
      type: source.type,
      totalSize: total,
      blockSize: kBlockSize,
      blockCount: blockCount,
      otiFull: otiFull,
      otiLast: otiLast,
    );
    _source = source;
    _blockIdx = -1;
    _enc = null;
    _packetInBlock = 0;
    _frameCount = 0;
    _pass = 0;

    HistoryStore.instance.addSent(source.name, source.type, total, _grid, _ec);

    try {
      await WakelockPlus.enable();
      await ScreenBrightness().setApplicationScreenBrightness(1.0);
    } catch (_) {}

    final mySeq = ++_seq;
    setState(() {
      _running = true;
      _status = '${source.name}  ${_fmtSize(total)}  ·  $blockCount ブロック  ·  grid $_grid';
    });
    _txLoop(mySeq);
  }

  Future<Uint8List> _nextDataPayload() async {
    final m = _manifest!;
    if (_enc == null || _packetInBlock >= _enc!.packetCount()) {
      _blockIdx = (_blockIdx + 1) % m.blockCount;
      if (_blockIdx == 0) _pass++;
      final off = _blockIdx * m.blockSize;
      final len = m.blockLen(_blockIdx);
      final bytes = await _source!.readBlock(off, len);
      final packetSize = _packetByGrid[_grid] ?? 200;
      _enc = FountainEncoder(
          payload: bytes, packetSize: packetSize, extraRepair: (len / packetSize * 0.3).ceil());
      _packetInBlock = 0;
    }
    final packet = _enc!.packet(i: _packetInBlock++);
    return buildDataQr(_blockIdx, packet);
  }

  Future<void> _txLoop(int mySeq) async {
    final interval = Duration(milliseconds: (1000 / _fps).round());
    while (_running && mySeq == _seq) {
      final (rows, cols) = _dims;
      final n = rows * cols;
      final mats = <QrMatrix>[];
      final showManifest = _frameCount % _manifestEveryFrames == 0;
      for (int cell = 0; cell < n; cell++) {
        final Uint8List payload;
        if (showManifest && cell == 0) {
          payload = _manifest!.toQr();
        } else {
          payload = await _nextDataPayload();
        }
        mats.add(makeQr(data: payload, ec: _ec, minVersion: 0));
      }
      if (mySeq != _seq) break;
      _frameCount++;
      setState(() {
        _frame = mats;
        _status = '送信中  ·  ブロック ${_blockIdx < 0 ? 0 : _blockIdx + 1}/${_manifest!.blockCount}'
            '  ·  ${_pass + 1} 巡目';
      });
      await Future.delayed(interval);
    }
  }

  Future<void> _stop() async {
    _seq++;
    _running = false;
    await _source?.close();
    _source = null;
    _enc = null;
    try {
      await WakelockPlus.disable();
      await ScreenBrightness().resetApplicationScreenBrightness();
    } catch (_) {}
    setState(() => _frame = []);
  }

  @override
  void dispose() {
    _seq++;
    _running = false;
    _textCtrl.dispose();
    _source?.close();
    WakelockPlus.disable();
    ScreenBrightness().resetApplicationScreenBrightness().catchError((_) {});
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _running ? _buildTransmit(context) : _buildConfig(context);

  Widget _buildTransmit(BuildContext context) {
    final (rows, cols) = _dims;
    return Column(
      children: [
        Expanded(
          child: Container(
            color: Colors.white,
            alignment: Alignment.center,
            child: AspectRatio(
              aspectRatio: 1,
              child: CustomPaint(painter: _QrGridPainter(_frame, rows, cols)),
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.all(8),
          child: Row(
            children: [
              Expanded(child: Text(_status, style: Theme.of(context).textTheme.bodySmall)),
              FilledButton.icon(onPressed: _stop, icon: const Icon(Icons.stop), label: const Text('停止')),
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildConfig(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        TextField(
          controller: _textCtrl,
          maxLines: 3,
          decoration: const InputDecoration(
            labelText: 'テキスト (または画像/ファイルを選択)',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            OutlinedButton.icon(onPressed: _pickImage, icon: const Icon(Icons.image), label: const Text('画像')),
            const SizedBox(width: 8),
            OutlinedButton.icon(
                onPressed: _pickFile, icon: const Icon(Icons.attach_file), label: const Text('ファイル')),
          ],
        ),
        if (_pickedName != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Row(
              children: [
                const Icon(Icons.insert_drive_file, size: 18),
                const SizedBox(width: 6),
                Expanded(
                    child: Text('$_pickedName (${_fmtSize(_pickedSize)})', overflow: TextOverflow.ellipsis)),
                IconButton(
                  onPressed: () => setState(() {
                    _pickedPath = null;
                    _pickedName = null;
                    _pickedSize = 0;
                  }),
                  icon: const Icon(Icons.clear),
                ),
              ],
            ),
          ),
        const Divider(height: 32),
        Row(
          children: [
            Expanded(child: _dropdown('グリッド', _grid, _grids, (v) => setState(() => _grid = v!))),
            const SizedBox(width: 12),
            Expanded(child: _dropdown('EC', _ec, _ecLevels, (v) => setState(() => _ec = v!))),
          ],
        ),
        const SizedBox(height: 12),
        Row(
          children: [
            const Text('FPS'),
            Expanded(
              child: Slider(
                  value: _fps.toDouble(),
                  min: 3,
                  max: 20,
                  divisions: 17,
                  label: '$_fps',
                  onChanged: (v) => setState(() => _fps = v.round())),
            ),
            Text('$_fps'),
          ],
        ),
        const SizedBox(height: 16),
        FilledButton.icon(onPressed: _start, icon: const Icon(Icons.play_arrow), label: const Text('送信開始')),
        if (_status.isNotEmpty) ...[
          const SizedBox(height: 12),
          Text(_status, style: Theme.of(context).textTheme.bodySmall),
        ],
      ],
    );
  }

  Widget _dropdown(String label, String value, List<String> items, ValueChanged<String?> onChanged) {
    return DropdownButtonFormField<String>(
      initialValue: value,
      decoration: InputDecoration(labelText: label, border: const OutlineInputBorder()),
      items: items.map((e) => DropdownMenuItem(value: e, child: Text(e))).toList(),
      onChanged: onChanged,
    );
  }
}

String _fmtSize(int n) {
  if (n >= 1024 * 1024 * 1024) return '${(n / 1024 / 1024 / 1024).toStringAsFixed(2)}GB';
  if (n >= 1024 * 1024) return '${(n / 1024 / 1024).toStringAsFixed(1)}MB';
  if (n >= 1024) return '${(n / 1024).toStringAsFixed(1)}KB';
  return '${n}B';
}

/// grid のセルに QR モジュール行列を描画 (各セル 8% quiet zone)。
class _QrGridPainter extends CustomPainter {
  final List<QrMatrix> mats;
  final int rows;
  final int cols;
  _QrGridPainter(this.mats, this.rows, this.cols);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = Colors.white);
    if (mats.isEmpty) return;
    final cellW = size.width / cols;
    final cellH = size.height / rows;
    final black = Paint()..color = Colors.black;
    for (int r = 0; r < rows; r++) {
      for (int c = 0; c < cols; c++) {
        final i = r * cols + c;
        if (i >= mats.length) continue;
        final m = mats[i];
        final n = m.size;
        if (n == 0) continue;
        final square = cellW < cellH ? cellW : cellH;
        final margin = square * 0.08;
        final moduleSize = ((square - margin * 2) / n).floorToDouble();
        if (moduleSize < 1) continue;
        final qrPx = moduleSize * n;
        final ox = c * cellW + (cellW - qrPx) / 2;
        final oy = r * cellH + (cellH - qrPx) / 2;
        final path = Path();
        final mod = m.modules;
        for (int y = 0; y < n; y++) {
          final rowBase = y * n;
          for (int x = 0; x < n; x++) {
            if (mod[rowBase + x] == 1) {
              path.addRect(Rect.fromLTWH(ox + x * moduleSize, oy + y * moduleSize, moduleSize, moduleSize));
            }
          }
        }
        canvas.drawPath(path, black);
      }
    }
  }

  @override
  bool shouldRepaint(covariant _QrGridPainter old) => old.mats != mats;
}
