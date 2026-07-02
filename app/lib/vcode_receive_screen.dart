import 'dart:io';

import 'package:camera/camera.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import 'history_store.dart';
import 'src/rust/api/fountain.dart';
import 'src/rust/api/vcode.dart';

/// vcode 受信画面。camera パッケージで生 YUV フレームを取得し、
/// Y プレーンを Rust の vcode スキャナに渡す (mobile_scanner/MLKit 不使用)。
class VcodeReceiveScreen extends StatefulWidget {
  const VcodeReceiveScreen({super.key});
  @override
  State<VcodeReceiveScreen> createState() => _VcodeReceiveScreenState();
}

/// UI のガイド枠と Rust 側のガイド枠計算で共有する比率 (回転後画像幅に対する枠幅)
const _guideFrac = 0.8;

class _VcodeReceiveScreenState extends State<VcodeReceiveScreen>
    with WidgetsBindingObserver {
  CameraController? _cam;
  bool _busy = false;
  bool _active = false;

  FountainDecoder? _dec;
  Uint8List? _payload;
  String? _savedPath;

  // 統計
  int _framesSeen = 0;
  int _framesDetected = 0;
  int _blocksOk = 0;
  int _packetsAdded = 0;
  int _lastScanMs = 0;
  int _scanMsSum = 0;
  int _scanCount = 0;
  DateTime? _firstDetected;
  Duration? _elapsed;
  String _status = 'カメラ起動待ち';

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _initCamera();
  }

  Future<void> _initCamera() async {
    try {
      final cams = await availableCameras();
      final back = cams.firstWhere(
          (c) => c.lensDirection == CameraLensDirection.back,
          orElse: () => cams.first);
      final cam = CameraController(
        back,
        ResolutionPreset.high, // 720p: 解像度と処理速度のバランス
        enableAudio: false,
        imageFormatGroup: ImageFormatGroup.yuv420,
      );
      await cam.initialize();
      await cam.startImageStream(_onFrame);
      await WakelockPlus.enable();
      setState(() {
        _cam = cam;
        _active = true;
        _status = 'スキャン中';
      });
    } catch (e) {
      setState(() => _status = 'カメラ初期化失敗: $e');
    }
  }

  Future<void> _onFrame(CameraImage img) async {
    if (_busy || !_active || _payload != null) return;
    _busy = true;
    try {
      final sw = Stopwatch()..start();
      final y = img.planes[0];
      final rotation = _cam?.description.sensorOrientation ?? 90;
      // 未検出のあいだ 150 フレームごとに処理済み画像を上書き保存 (PC 解析用)
      final wantDump = _framesDetected == 0 && _framesSeen > 0 && _framesSeen % 150 == 0;
      final report = await vcodeScanGray(
        y: y.bytes,
        width: img.width,
        height: img.height,
        stride: y.bytesPerRow,
        rotationDeg: rotation,
        guideFrac: _guideFrac,
        debugDump: wantDump,
      );
      sw.stop();
      if (!_active || _payload != null) return;
      if (report.debugGray != null) _saveDump(report);

      _framesSeen++;
      _lastScanMs = sw.elapsedMilliseconds;
      _scanMsSum += _lastScanMs;
      _scanCount++;
      if (report.detected) {
        _framesDetected++;
        _firstDetected ??= DateTime.now();
        _blocksOk += report.blocksOk;
        _dec ??= FountainDecoder(otiBytes: report.oti);
        var done = false;
        for (final p in report.packets) {
          _packetsAdded++;
          if (_dec!.addPacket(packet: p)) {
            done = true;
            break;
          }
        }
        debugPrint('[vcode-rx] seq=${report.frameSeq} '
            'blocks=${report.blocksOk}/${report.blocksTotal} '
            'pkts=$_packetsAdded scan=${_lastScanMs}ms done=$done');
        if (done) {
          _onComplete(_dec!.payload()!);
          return;
        }
      } else if (_framesSeen % 30 == 0) {
        debugPrint('[vcode-rx] not detected (${report.error}) '
            'scan=${_lastScanMs}ms seen=$_framesSeen detected=$_framesDetected');
      }
      if (mounted && _framesSeen % 5 == 0) setState(() {});
    } finally {
      _busy = false;
    }
  }

  Future<void> _saveDump(VcodeScanReport report) async {
    try {
      final dir = await getApplicationDocumentsDirectory();
      final path = '${dir.path}/vcode_dump_${report.debugW}x${report.debugH}.gray';
      await File(path).writeAsBytes(report.debugGray!);
      debugPrint('[vcode-rx] DUMP saved: $path (err=${report.error})');
    } catch (e) {
      debugPrint('[vcode-rx] DUMP 保存失敗: $e');
    }
  }

  Future<void> _onComplete(Uint8List payload) async {
    final elapsed = _firstDetected == null
        ? Duration.zero
        : DateTime.now().difference(_firstDetected!);
    setState(() {
      _payload = payload;
      _elapsed = elapsed;
      _status = '受信完了';
    });
    debugPrint('[vcode-rx] COMPLETE: ${payload.length} bytes in '
        '${elapsed.inMilliseconds}ms, '
        'frames seen=$_framesSeen detected=$_framesDetected, '
        'blocks=$_blocksOk, packets=$_packetsAdded');
    // 履歴に保存 (QR 受信と同じ HistoryStore を使用)
    try {
      final slot = HistoryStore.instance.reserveReceivedPath();
      await File(slot.path).writeAsBytes(payload);
      final name =
          'vcode_${DateTime.now().toIso8601String().replaceAll(':', '-').substring(0, 19)}.bin';
      await HistoryStore.instance
          .registerReceived(slot.id, name, 'application/octet-stream', payload.length);
      setState(() => _savedPath = '履歴: $name');
    } catch (e) {
      debugPrint('[vcode-rx] 履歴保存失敗: $e');
    }
    await _stopCamera();
  }

  Future<void> _stopCamera() async {
    _active = false;
    final cam = _cam;
    _cam = null;
    if (cam != null) {
      try {
        await cam.stopImageStream();
      } catch (_) {}
      await cam.dispose();
    }
    await WakelockPlus.disable();
  }

  Future<void> _reset() async {
    await _stopCamera();
    setState(() {
      _dec = null;
      _payload = null;
      _savedPath = null;
      _framesSeen = 0;
      _framesDetected = 0;
      _blocksOk = 0;
      _packetsAdded = 0;
      _scanMsSum = 0;
      _scanCount = 0;
      _firstDetected = null;
      _elapsed = null;
      _status = 'カメラ起動待ち';
    });
    await _initCamera();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused) _stopCamera();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _stopCamera();
    super.dispose();
  }

  /// 受信完了時の統計テーブル
  Widget _statsTable() {
    final p = _payload!;
    final ms = _elapsed?.inMilliseconds ?? 0;
    final kbps = ms > 0 ? (p.length / 1024) / (ms / 1000) : 0.0;
    final rows = <(String, String)>[
      ('サイズ', '${p.length} B'),
      ('所要時間 (初検出→完了)', ms >= 1000 ? '${(ms / 1000).toStringAsFixed(2)} 秒' : '$ms ms'),
      ('実効スループット', '${kbps.toStringAsFixed(1)} KB/s'),
      ('カメラフレーム数', '$_framesSeen (検出 $_framesDetected)'),
      ('回収ブロック', '$_blocksOk (部分回収込み)'),
      ('投入パケット', '$_packetsAdded'),
      ('平均スキャン時間', _scanCount > 0 ? '${(_scanMsSum / _scanCount).round()} ms' : '-'),
    ];
    return Table(
      columnWidths: const {0: IntrinsicColumnWidth(), 1: FlexColumnWidth()},
      defaultVerticalAlignment: TableCellVerticalAlignment.middle,
      children: [
        for (final (label, value) in rows)
          TableRow(children: [
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 3, horizontal: 8),
              child: Text(label,
                  style: TextStyle(
                      fontSize: 12, color: Theme.of(context).colorScheme.onSurfaceVariant)),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 3, horizontal: 8),
              child: Text(value,
                  style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600)),
            ),
          ]),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final cam = _cam;
    final received = _dec?.packetsReceived() ?? 0;
    final total = _dec == null
        ? null
        : (_dec!.payloadSize().toInt() + 43) ~/ 44; // packet_size=44 での必要 source 数

    return Column(
      children: [
        Expanded(
          child: _payload != null
              ? Center(
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        const Icon(Icons.check_circle, size: 64, color: Colors.green),
                        const SizedBox(height: 12),
                        Text(_status, style: Theme.of(context).textTheme.titleMedium),
                        const SizedBox(height: 12),
                        _statsTable(),
                        if (_savedPath != null)
                          Padding(
                            padding: const EdgeInsets.all(8),
                            child: Text(_savedPath!,
                                style: const TextStyle(fontSize: 11)),
                          ),
                        const SizedBox(height: 12),
                        FilledButton(
                            onPressed: _reset, child: const Text('もう一度受信')),
                      ],
                    ),
                  ),
                )
              : cam == null || !cam.value.isInitialized
                  ? Center(child: Text(_status))
                  : Stack(
                      fit: StackFit.expand,
                      children: [
                        CameraPreview(cam),
                        // ガイド枠 (Rust 側の guide_frac と同じ規約で中央配置)
                        IgnorePointer(
                          child: CustomPaint(painter: _GuidePainter()),
                        ),
                      ],
                    ),
        ),
        Padding(
          padding: const EdgeInsets.all(8),
          child: Text(
            _payload != null
                ? _status
                : 'frames: $_framesSeen  detected: $_framesDetected  '
                    'blocks: $_blocksOk  pkts: $received${total != null ? "/$total" : ""}  '
                    'scan: ${_lastScanMs}ms',
            style: const TextStyle(fontSize: 12),
          ),
        ),
      ],
    );
  }
}

class _GuidePainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = Colors.greenAccent
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2;
    final gw = size.width * _guideFrac;
    final gh = gw * 92 / 100;
    final rect = Rect.fromCenter(
        center: Offset(size.width / 2, size.height / 2), width: gw, height: gh);
    canvas.drawRect(rect, paint);
    // 四隅を強調
    const l = 24.0;
    final corner = Paint()
      ..color = Colors.greenAccent
      ..style = PaintingStyle.stroke
      ..strokeWidth = 5;
    for (final (dx, dy) in [(0.0, 0.0), (rect.width, 0.0), (0.0, rect.height), (rect.width, rect.height)]) {
      final p = rect.topLeft + Offset(dx, dy);
      final sx = dx == 0 ? 1.0 : -1.0;
      final sy = dy == 0 ? 1.0 : -1.0;
      canvas.drawLine(p, p + Offset(sx * l, 0), corner);
      canvas.drawLine(p, p + Offset(0, sy * l), corner);
    }
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
