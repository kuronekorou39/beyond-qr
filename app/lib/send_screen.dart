import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import 'package:image_picker/image_picker.dart';
import 'package:screen_brightness/screen_brightness.dart';
import 'package:wakelock_plus/wakelock_plus.dart';
import 'history_store.dart';
import 'src/rust/api/fountain.dart';
import 'src/rust/api/qr.dart';

/// grid ごとの実測ベスト packetSize (web 版と同じ。EC=M / 版=自動 前提)。
const _packetByGrid = {
  '1x1': 540,
  '1x2': 300,
  '2x2': 180,
  '2x3': 160,
  '3x3': 140,
};

const _grids = ['1x1', '1x2', '2x2', '2x3', '3x3'];
const _ecLevels = ['L', 'M', 'Q', 'H'];

/// web 版 wrapPayload と同じ: [uint32be headerLen][json header][body]。
Uint8List _wrapPayload(String name, String type, Uint8List body) {
  final header = utf8.encode(jsonEncode({'name': name, 'type': type, 'size': body.length}));
  final out = BytesBuilder();
  final lenBytes = ByteData(4)..setUint32(0, header.length, Endian.big);
  out.add(lenBytes.buffer.asUint8List());
  out.add(header);
  out.add(body);
  return out.toBytes();
}

class SendScreen extends StatefulWidget {
  const SendScreen({super.key});
  @override
  State<SendScreen> createState() => _SendScreenState();
}

class _SendScreenState extends State<SendScreen> {
  final _textCtrl = TextEditingController();
  final _picker = ImagePicker();
  String? _pickedName;
  Uint8List? _pickedBytes;
  String _pickedType = 'application/octet-stream';

  String _grid = '1x2';
  String _ec = 'M';
  int _fps = 12;

  FountainEncoder? _enc;
  Uint8List _oti = Uint8List(0);
  int _total = 0;
  int _cursor = 0;
  Timer? _timer;
  bool _running = false;
  List<QrMatrix> _frame = [];
  String _status = '';

  (int, int) get _dims {
    final p = _grid.split('x');
    return (int.parse(p[0]), int.parse(p[1]));
  }

  Future<void> _pickImage() async {
    final x = await _picker.pickImage(source: ImageSource.gallery);
    if (x == null) return;
    final bytes = await x.readAsBytes();
    setState(() {
      _pickedName = x.name;
      _pickedBytes = bytes;
      _pickedType = _imageMime(x.name);
    });
  }

  Future<void> _pickFile() async {
    final f = await openFile();
    if (f == null) return;
    final bytes = await f.readAsBytes();
    setState(() {
      _pickedName = f.name;
      _pickedBytes = bytes;
      _pickedType = f.mimeType ?? 'application/octet-stream';
    });
  }

  String _imageMime(String name) {
    final n = name.toLowerCase();
    if (n.endsWith('.png')) return 'image/png';
    if (n.endsWith('.webp')) return 'image/webp';
    if (n.endsWith('.gif')) return 'image/gif';
    return 'image/jpeg';
  }

  Uint8List _buildPayload() {
    if (_pickedBytes != null) {
      return _wrapPayload(_pickedName ?? 'file.bin', _pickedType, _pickedBytes!);
    }
    final body = Uint8List.fromList(utf8.encode(_textCtrl.text));
    return _wrapPayload('message.txt', 'text/plain;charset=utf-8', body);
  }

  Future<void> _start() async {
    final payload = _buildPayload();
    if (payload.length <= 6) {
      setState(() => _status = 'ペイロードが空です');
      return;
    }
    final packetSize = _packetByGrid[_grid] ?? 200;
    final extraRepair = (payload.length / packetSize * 0.5).ceil();
    final enc = FountainEncoder(payload: payload, packetSize: packetSize, extraRepair: extraRepair);
    _enc = enc;
    _oti = enc.otiBytes();
    _total = enc.packetCount();
    _cursor = 0;

    // 送信試行を履歴に記録 (オフラインなので成否は不明=試行記録)
    final sentName = _pickedBytes != null ? (_pickedName ?? 'file.bin') : 'message.txt';
    final sentType = _pickedBytes != null ? _pickedType : 'text/plain;charset=utf-8';
    HistoryStore.instance.addSent(sentName, sentType, payload.length, _grid, _ec);

    try {
      await WakelockPlus.enable();
      await ScreenBrightness().setApplicationScreenBrightness(1.0);
    } catch (_) {/* 非対応端末は無視 */}

    setState(() {
      _running = true;
      _status = 'payload ${payload.length}B → $_total packets (${packetSize}B/QR, grid $_grid)';
    });
    _renderFrame();
    _timer = Timer.periodic(Duration(milliseconds: (1000 / _fps).round()), (_) => _renderFrame());
  }

  void _renderFrame() {
    final enc = _enc;
    if (enc == null || _total == 0) return;
    final (rows, cols) = _dims;
    final n = rows * cols;
    final mats = <QrMatrix>[];
    for (int k = 0; k < n; k++) {
      final idx = (_cursor + k) % _total;
      final packet = enc.packet(i: idx);
      final data = Uint8List(_oti.length + packet.length)
        ..setRange(0, _oti.length, _oti)
        ..setRange(_oti.length, _oti.length + packet.length, packet);
      mats.add(makeQr(data: data, ec: _ec, minVersion: 0));
    }
    _cursor = (_cursor + n) % _total;
    setState(() => _frame = mats);
  }

  Future<void> _stop() async {
    _timer?.cancel();
    _timer = null;
    try {
      await WakelockPlus.disable();
      await ScreenBrightness().resetApplicationScreenBrightness();
    } catch (_) {}
    setState(() {
      _running = false;
      _frame = [];
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    _textCtrl.dispose();
    WakelockPlus.disable();
    ScreenBrightness().resetApplicationScreenBrightness().catchError((_) {});
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_running) return _buildTransmit(context);
    return _buildConfig(context);
  }

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
              FilledButton.icon(
                onPressed: _stop,
                icon: const Icon(Icons.stop),
                label: const Text('停止'),
              ),
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
            OutlinedButton.icon(
              onPressed: _pickImage,
              icon: const Icon(Icons.image),
              label: const Text('画像'),
            ),
            const SizedBox(width: 8),
            OutlinedButton.icon(
              onPressed: _pickFile,
              icon: const Icon(Icons.attach_file),
              label: const Text('ファイル'),
            ),
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
                  child: Text('$_pickedName (${_pickedBytes?.length ?? 0}B)',
                      overflow: TextOverflow.ellipsis),
                ),
                IconButton(
                  onPressed: () => setState(() {
                    _pickedName = null;
                    _pickedBytes = null;
                  }),
                  icon: const Icon(Icons.clear),
                ),
              ],
            ),
          ),
        const Divider(height: 32),
        Row(
          children: [
            Expanded(
              child: _dropdown('グリッド', _grid, _grids, (v) => setState(() => _grid = v!)),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: _dropdown('EC', _ec, _ecLevels, (v) => setState(() => _ec = v!)),
            ),
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
                onChanged: (v) => setState(() => _fps = v.round()),
              ),
            ),
            Text('$_fps'),
          ],
        ),
        const SizedBox(height: 16),
        FilledButton.icon(
          onPressed: _start,
          icon: const Icon(Icons.play_arrow),
          label: const Text('送信開始'),
        ),
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

/// grid のセルに QR モジュール行列を描画する。各セルは 8% の quiet zone を確保。
class _QrGridPainter extends CustomPainter {
  final List<QrMatrix> mats;
  final int rows;
  final int cols;
  _QrGridPainter(this.mats, this.rows, this.cols);

  @override
  void paint(Canvas canvas, Size size) {
    final white = Paint()..color = Colors.white;
    canvas.drawRect(Offset.zero & size, white);
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
              path.addRect(Rect.fromLTWH(
                  ox + x * moduleSize, oy + y * moduleSize, moduleSize, moduleSize));
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
