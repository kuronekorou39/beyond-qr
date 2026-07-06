import 'dart:async';
import 'dart:io';

import 'package:camera/camera.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import 'history_screen.dart' show shareReceived;
import 'history_store.dart';
import 'src/rust/api/fountain.dart';
import 'src/rust/api/vcode.dart';
import 'vcode_view.dart';

/// vcode 受信画面。camera パッケージで生 YUV フレームを取得し、
/// Y プレーンを Rust の vcode スキャナに渡す (mobile_scanner/MLKit 不使用)。
class VcodeReceiveScreen extends StatefulWidget {
  /// このタブが表示中で校正も開いていない = カメラを動かしてよいとき true。
  /// false の間は背面カメラを解放し、他画面 (校正など) と奪い合わないようにする。
  const VcodeReceiveScreen({super.key, this.active = true});
  final bool active;
  @override
  State<VcodeReceiveScreen> createState() => _VcodeReceiveScreenState();
}

class _VcodeReceiveScreenState extends State<VcodeReceiveScreen>
    with WidgetsBindingObserver {
  CameraController? _cam;
  bool _busy = false;
  bool _active = false;
  bool _camBusy = false; // カメラ初期化/再初期化の多重実行ガード
  Timer? _watchdog; // プレビューが灰色 (フレーム途絶) になったら作り直す
  VcodeRx? _rx;

  FountainDecoder? _dec;
  int? _packetSize; // 最初の回収パケットから推定 (シリアライズ長 - 4)
  Uint8List? _payload;
  String? _savedPath;
  HistoryItem? _savedItem;

  // 統計
  int _camCallbacks = 0; // カメラが配信した全フレーム (busy スキップ含む)
  DateTime? _camStarted;
  int _framesSeen = 0;
  int _framesDetected = 0;
  int _framesTracked = 0;
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
    if (widget.active) _initCamera();
    // フレームが一定時間途絶えたらカメラを作り直す (校正からの復帰レースや
    // 一時的なカメラ喪失で灰色のまま固まるのを自己修復する)。
    _watchdog =
        Timer.periodic(const Duration(seconds: 1), (_) => _checkStale());
  }

  void _checkStale() {
    if (!mounted || !widget.active || _payload != null || _camBusy) return;
    final cam = _cam;
    if (cam == null) {
      _initCamera(); // active なのにカメラが無い → 再取得
      return;
    }
    if (!cam.value.isInitialized) return;
    final ref = _lastCallbackAt ?? _camStarted;
    if (ref != null && DateTime.now().difference(ref).inMilliseconds > 2000) {
      _reinit(); // フレームが 2 秒途絶 = 灰色 → 作り直す
    }
  }

  Future<void> _reinit() async {
    if (_camBusy) return;
    await _stopCamera();
    if (mounted && widget.active && _payload == null) await _initCamera();
  }

  @override
  void didUpdateWidget(VcodeReceiveScreen old) {
    super.didUpdateWidget(old);
    if (widget.active == old.active) return;
    if (widget.active) {
      // 再表示: 未完了ならカメラを再取得してスキャン再開
      if (_payload == null && _cam == null) _initCamera();
    } else {
      // 非表示 / 校正表示中: カメラを解放
      _stopCamera();
    }
  }

  Future<void> _initCamera() async {
    if (_camBusy) return;
    _camBusy = true;
    try {
      await _initCameraInner();
    } finally {
      _camBusy = false;
    }
  }

  Future<void> _initCameraInner() async {
    // 直前まで校正/別タブがカメラを掴んでいると初回 initialize が失敗しうるので、
    // 解放待ちのため数回リトライする。
    for (var attempt = 0; attempt < 6; attempt++) {
      if (!mounted || !widget.active) return;
      CameraController? cam;
      try {
        final cams = await availableCameras();
        final back = cams.firstWhere(
            (c) => c.lensDirection == CameraLensDirection.back,
            orElse: () => cams.first);
        cam = CameraController(
          back,
          // 1080p: 高密度レイアウト (7x6) はセル解像度が必要 (720p だと ~4px/セルで限界)
          ResolutionPreset.veryHigh,
          enableAudio: false,
          // 60fps 要求 (対応外の端末では無視される。実配信レートは統計で確認)
          fps: 60,
          imageFormatGroup: ImageFormatGroup.yuv420,
        );
        await cam.initialize();
        if (!mounted || !widget.active) {
          await cam.dispose();
          return;
        }
        _rx = VcodeRx();
        _camStarted = DateTime.now();
        _camCallbacks = 0;
        await cam.startImageStream(_onFrame);
        await WakelockPlus.enable();
        setState(() {
          _cam = cam;
          _active = true;
          _status = 'スキャン中';
        });
        return;
      } catch (e) {
        try {
          await cam?.dispose();
        } catch (_) {}
        if (attempt == 5) {
          if (mounted) setState(() => _status = 'カメラ初期化失敗: $e');
        } else {
          await Future.delayed(const Duration(milliseconds: 300));
        }
      }
    }
  }

  /// カメラの実配信フレームレート (要求 60fps がどこまで通ったかの検証用)。
  /// 最後のコールバック時刻までで計測する (完了後に表示しても値が減衰しない)。
  double get _camFps {
    final started = _camStarted;
    final last = _lastCallbackAt;
    if (started == null || last == null || _camCallbacks < 2) return 0;
    final sec = last.difference(started).inMilliseconds / 1000.0;
    return sec > 0 ? _camCallbacks / sec : 0;
  }

  DateTime? _lastCallbackAt;

  Future<void> _onFrame(CameraImage img) async {
    _camCallbacks++;
    _lastCallbackAt = DateTime.now();
    if (_busy || !_active || _payload != null) return;
    _busy = true;
    try {
      final sw = Stopwatch()..start();
      final y = img.planes[0];
      final rotation = _cam?.description.sensorOrientation ?? 90;
      // 未検出のあいだ 150 フレームごとに処理済み画像を上書き保存 (PC 解析用)
      final wantDump = _framesDetected == 0 && _framesSeen > 0 && _framesSeen % 150 == 0;
      final rx = _rx;
      if (rx == null) return;
      final report = await rx.scan(
        y: y.bytes,
        width: img.width,
        height: img.height,
        stride: y.bytesPerRow,
        rotationDeg: rotation,
        guideFrac: kVcodeGuideFrac,
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
        if (report.tracked) _framesTracked++;
        _firstDetected ??= DateTime.now();
        _blocksOk += report.blocksOk;
        _dec ??= FountainDecoder(otiBytes: report.oti);
        if (_packetSize == null && report.packets.isNotEmpty) {
          _packetSize = report.packets.first.length - 4;
        }
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
            'pkts=$_packetsAdded scan=${_lastScanMs}ms '
            'tracked=${report.tracked} done=$done');
        if (done) {
          _onComplete(_dec!.payload()!);
          return;
        }
      } else if (_framesSeen % 30 == 0) {
        debugPrint('[vcode-rx] not detected (${report.error}) '
            'scan=${_lastScanMs}ms seen=$_framesSeen detected=$_framesDetected '
            'camFps=${_camFps.toStringAsFixed(1)}');
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

  /// 先頭バイトからファイル種別を推定する (vcode はメタデータを運ばないため)
  (String, String) _sniffType(Uint8List b) {
    if (b.length > 3 && b[0] == 0xFF && b[1] == 0xD8) return ('jpg', 'image/jpeg');
    if (b.length > 7 && b[0] == 0x89 && b[1] == 0x50) return ('png', 'image/png');
    if (b.length > 11 && b[8] == 0x57 && b[9] == 0x45 && b[10] == 0x42 && b[11] == 0x50) {
      return ('webp', 'image/webp');
    }
    if (b.length > 3 && b[0] == 0x25 && b[1] == 0x50 && b[2] == 0x44 && b[3] == 0x46) {
      return ('pdf', 'application/pdf');
    }
    if (b.length > 1 && b[0] == 0x50 && b[1] == 0x4B) return ('zip', 'application/zip');
    // 先頭 4KB の制御文字率でテキスト判定
    final probe = b.take(4096);
    final ctrl = probe.where((c) => c < 9 || (c > 13 && c < 32) || c == 127).length;
    if (probe.isNotEmpty && ctrl / probe.length < 0.02) {
      return ('txt', 'text/plain;charset=utf-8');
    }
    return ('bin', 'application/octet-stream');
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
    final ms = elapsed.inMilliseconds;
    final kbps = ms > 0 ? (payload.length / 1024) / (ms / 1000) : 0.0;
    final note = '${(ms / 1000).toStringAsFixed(2)}s'
        ' · ${kbps.toStringAsFixed(1)}KB/s'
        ' · cam${_camFps.toStringAsFixed(0)}fps'
        ' · 検出$_framesDetected/$_framesSeen(追従$_framesTracked)'
        ' · blk$_blocksOk · pkt$_packetsAdded'
        ' · scan${_scanCount > 0 ? (_scanMsSum / _scanCount).round() : 0}ms';
    debugPrint('[vcode-rx] COMPLETE: ${payload.length} bytes in ${ms}ms, $note');
    // 履歴に保存 (QR 受信と同じ HistoryStore、種別は内容から推定)
    try {
      final (ext, mime) = _sniffType(payload);
      final slot = HistoryStore.instance.reserveReceivedPath();
      await File(slot.path).writeAsBytes(payload);
      final name =
          'vcode_${DateTime.now().toIso8601String().replaceAll(':', '-').substring(0, 19)}.$ext';
      await HistoryStore.instance
          .registerReceived(slot.id, name, mime, payload.length, note: note);
      setState(() {
        _savedPath = '履歴に保存: $name';
        _savedItem = HistoryStore.instance.received.first;
      });
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
      _packetSize = null;
      _payload = null;
      _savedPath = null;
      _savedItem = null;
      _framesSeen = 0;
      _framesDetected = 0;
      _framesTracked = 0;
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
    _watchdog?.cancel();
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
      ('カメラフレーム数', '$_framesSeen (検出 $_framesDetected / 追従 $_framesTracked)'),
      ('カメラ実効fps', _camFps.toStringAsFixed(1)),
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
    final ps = _packetSize ?? 44;
    final total = _dec == null
        ? null
        : (_dec!.payloadSize().toInt() + ps - 1) ~/ ps; // 必要 source パケット数

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
                        Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            if (_savedItem != null)
                              FilledButton.tonalIcon(
                                onPressed: () => shareReceived(_savedItem!),
                                icon: const Icon(Icons.share),
                                label: const Text('共有 / 保存'),
                              ),
                            const SizedBox(width: 12),
                            FilledButton(
                                onPressed: _reset, child: const Text('もう一度受信')),
                          ],
                        ),
                      ],
                    ),
                  ),
                )
              : cam == null || !cam.value.isInitialized
                  ? Center(child: Text(_status))
                  : VcodeCameraView(cam),
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
