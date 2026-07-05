import 'package:camera/camera.dart';
import 'package:flutter/material.dart';

/// vcode 受信の共有パーツ。本番受信 (VcodeReceiveScreen) と校正受信 (_VCalReceive)
/// で「カメラ描画」「緑のガイド枠」「スキャン範囲 (guideFrac)」を完全に共有し、
/// 校正で合わせた位置がそのまま本番でも成立するようにする。
///
/// ここを唯一の真実 (source of truth) とし、両画面はこの値/ウィジェットを使う。
/// UI のガイド枠と Rust 側のスキャン範囲計算はこの比率で一致させること。
const double kVcodeGuideFrac = 0.8;

/// カメラプレビュー + 緑ガイド枠。受信系はすべてこれを使う。
class VcodeCameraView extends StatelessWidget {
  const VcodeCameraView(this.controller, {super.key});
  final CameraController controller;

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.expand,
      children: [
        CameraPreview(controller),
        IgnorePointer(child: CustomPaint(painter: VcodeGuidePainter())),
      ],
    );
  }
}

/// 中央に vcode の枠 (guideFrac 準拠) を描く。Rust 側の guide_frac と同一規約。
class VcodeGuidePainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = Colors.greenAccent
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2;
    final gw = size.width * kVcodeGuideFrac;
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
    for (final (dx, dy) in [
      (0.0, 0.0),
      (rect.width, 0.0),
      (0.0, rect.height),
      (rect.width, rect.height)
    ]) {
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
