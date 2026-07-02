import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:wakelock_plus/wakelock_plus.dart';
import 'src/rust/api/qr.dart';

/// 校正モード: 送信側が「ゆるい→きつい」レベルのテストパターンを表示し、
/// 受信側が「どのレベルまで読めるか」を確認する (オフラインの人アシスト校正)。
/// ここでは QR 版。読めた最も密なレベルが、その環境で使える上限の目安になる。

class QrCalLevel {
  final String label;
  final int rows;
  final int cols;
  final int bytes; // QR に載せるデータ量 (版=密度を決める)
  const QrCalLevel(this.label, this.rows, this.cols, this.bytes);
}

/// ゆるい (大きい単一QR・低版) → きつい (多グリッド・高版)。
const qrCalLevels = <QrCalLevel>[
  QrCalLevel('Lv1  1×1 特ゆる', 1, 1, 150),
  QrCalLevel('Lv2  1×1 標準', 1, 1, 400),
  QrCalLevel('Lv3  1×1 密', 1, 1, 700),
  QrCalLevel('Lv4  1×2', 1, 2, 300),
  QrCalLevel('Lv5  2×2', 2, 2, 200),
  QrCalLevel('Lv6  2×2 密', 2, 2, 320),
  QrCalLevel('Lv7  3×3', 3, 3, 150),
];

Uint8List _calPayload(int levelIndex, int bytes) {
  final head = 'CQR$levelIndex';
  final b = utf8.encode(head);
  final out = Uint8List(bytes < b.length ? b.length : bytes);
  out.setRange(0, b.length, b);
  for (var i = b.length; i < out.length; i++) {
    out[i] = 0x41; // 'A' padding
  }
  return out;
}

class CalibrationScreen extends StatefulWidget {
  const CalibrationScreen({super.key});
  @override
  State<CalibrationScreen> createState() => _CalibrationScreenState();
}

class _CalibrationScreenState extends State<CalibrationScreen> {
  bool _send = true; // true=送信(表示), false=受信(確認)

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('校正 (QR)'),
        bottom: PreferredSize(
          preferredSize: const Size.fromHeight(48),
          child: Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: SegmentedButton<bool>(
              segments: const [
                ButtonSegment(value: true, label: Text('送信(表示)'), icon: Icon(Icons.qr_code_2)),
                ButtonSegment(value: false, label: Text('受信(確認)'), icon: Icon(Icons.center_focus_strong)),
              ],
              selected: {_send},
              onSelectionChanged: (s) => setState(() => _send = s.first),
            ),
          ),
        ),
      ),
      body: _send ? const _CalSend() : const _CalReceive(),
    );
  }
}

// ============ 送信 (テストパターン表示) ============
class _CalSend extends StatefulWidget {
  const _CalSend();
  @override
  State<_CalSend> createState() => _CalSendState();
}

class _CalSendState extends State<_CalSend> {
  int _lv = 0;

  List<QrMatrix> _matrices() {
    final lv = qrCalLevels[_lv];
    final data = _calPayload(_lv, lv.bytes);
    final one = makeQr(data: data, ec: 'M', minVersion: 0);
    return List.filled(lv.rows * lv.cols, one);
  }

  @override
  Widget build(BuildContext context) {
    final lv = qrCalLevels[_lv];
    return Column(
      children: [
        Expanded(
          child: Container(
            color: Colors.white,
            alignment: Alignment.center,
            child: AspectRatio(
              aspectRatio: 1,
              child: CustomPaint(painter: _CalQrPainter(_matrices(), lv.rows, lv.cols)),
            ),
          ),
        ),
        Container(
          padding: const EdgeInsets.all(12),
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          child: Column(
            children: [
              Text(lv.label, style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 4),
              Text('受信側が読めたらこのレベルはOK。ゆるい→きつい と上げて限界を探す',
                  style: Theme.of(context).textTheme.bodySmall),
              const SizedBox(height: 8),
              Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  IconButton.filledTonal(
                    onPressed: _lv > 0 ? () => setState(() => _lv--) : null,
                    icon: const Icon(Icons.chevron_left),
                  ),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 16),
                    child: Text('${_lv + 1} / ${qrCalLevels.length}',
                        style: Theme.of(context).textTheme.titleMedium),
                  ),
                  IconButton.filledTonal(
                    onPressed: _lv < qrCalLevels.length - 1 ? () => setState(() => _lv++) : null,
                    icon: const Icon(Icons.chevron_right),
                  ),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _CalQrPainter extends CustomPainter {
  final List<QrMatrix> mats;
  final int rows;
  final int cols;
  _CalQrPainter(this.mats, this.rows, this.cols);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = Colors.white);
    final cellW = size.width / cols;
    final cellH = size.height / rows;
    final black = Paint()..color = Colors.black;
    for (int r = 0; r < rows; r++) {
      for (int c = 0; c < cols; c++) {
        final m = mats[r * cols + c];
        final n = m.size;
        if (n == 0) continue;
        final square = cellW < cellH ? cellW : cellH;
        final margin = square * 0.08;
        final ms = ((square - margin * 2) / n).floorToDouble();
        if (ms < 1) continue;
        final qrPx = ms * n;
        final ox = c * cellW + (cellW - qrPx) / 2;
        final oy = r * cellH + (cellH - qrPx) / 2;
        final path = Path();
        final mod = m.modules;
        for (int y = 0; y < n; y++) {
          final rb = y * n;
          for (int x = 0; x < n; x++) {
            if (mod[rb + x] == 1) {
              path.addRect(Rect.fromLTWH(ox + x * ms, oy + y * ms, ms, ms));
            }
          }
        }
        canvas.drawPath(path, black);
      }
    }
  }

  @override
  bool shouldRepaint(covariant _CalQrPainter old) => old.mats != mats;
}

// ============ 受信 (どのレベルまで読めるか確認) ============
class _CalReceive extends StatefulWidget {
  const _CalReceive();
  @override
  State<_CalReceive> createState() => _CalReceiveState();
}

class _CalReceiveState extends State<_CalReceive> {
  final _controller = MobileScannerController(
    detectionSpeed: DetectionSpeed.noDuplicates,
    formats: const [BarcodeFormat.qrCode],
  );
  final Set<int> _readable = {};

  @override
  void initState() {
    super.initState();
    WakelockPlus.enable();
  }

  @override
  void dispose() {
    _controller.dispose();
    WakelockPlus.disable();
    super.dispose();
  }

  void _onDetect(BarcodeCapture cap) {
    var changed = false;
    for (final b in cap.barcodes) {
      final v = b.displayValue;
      if (v != null && v.startsWith('CQR') && v.length > 3) {
        final idx = int.tryParse(v.substring(3, 4));
        if (idx != null && idx >= 0 && idx < qrCalLevels.length && _readable.add(idx)) {
          changed = true;
        }
      }
    }
    if (changed) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final best = _readable.isEmpty ? -1 : _readable.reduce((a, b) => a > b ? a : b);
    return Column(
      children: [
        Expanded(child: MobileScanner(controller: _controller, onDetect: _onDetect)),
        Container(
          padding: const EdgeInsets.all(12),
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      best < 0
                          ? '送信側のテストパターンに向けてください'
                          : '✅ 読めた最密: ${qrCalLevels[best].label}',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                  ),
                  TextButton.icon(
                    onPressed: () => setState(() => _readable.clear()),
                    icon: const Icon(Icons.refresh),
                    label: const Text('リセット'),
                  ),
                ],
              ),
              const SizedBox(height: 6),
              Wrap(
                spacing: 6,
                runSpacing: 4,
                children: [
                  for (var i = 0; i < qrCalLevels.length; i++)
                    Chip(
                      visualDensity: VisualDensity.compact,
                      avatar: Icon(
                        _readable.contains(i) ? Icons.check_circle : Icons.circle_outlined,
                        color: _readable.contains(i) ? Colors.green : Colors.grey,
                        size: 18,
                      ),
                      label: Text('Lv${i + 1}'),
                    ),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }
}
