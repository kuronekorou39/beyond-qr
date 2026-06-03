// PNG → セル列 → ペイロード のパイプライン (Phase 0d-A クリーン版)。
//
// 歪み無しの整列 PNG 専用。各セル中心ピクセルの sRGB を sRGB ユークリッド距離で
// 最近傍パレット色に量子化する。Phase 0e で歪み対応 (透視 + キャリブレーション +
// OKLab) を追加する。

import 'dart:typed_data';

import 'package:image/image.dart' as img;

import 'bridge.dart';

class PngDecoder {
  final BeyondQrBridge bridge;
  final Uint8List palette;

  PngDecoder(this.bridge) : palette = bridge.paletteRgb();

  /// 整列 RGB 画像からセル列を復元する。各セル中心 1 px をサンプル。
  Uint8List imageToCells(img.Image image, FrameSpec spec) {
    final (expectedW, expectedH) = spec.imageDimensions;
    if (image.width != expectedW || image.height != expectedH) {
      throw ArgumentError(
          'image ${image.width}x${image.height} != expected ${expectedW}x$expectedH');
    }
    final cells = Uint8List(spec.totalCells);
    final half = spec.cellPx ~/ 2;
    for (int gy = 0; gy < spec.gridHeight; gy++) {
      for (int gx = 0; gx < spec.gridWidth; gx++) {
        final px = gx * spec.cellPx + half;
        final py = gy * spec.cellPx + half;
        final pixel = image.getPixel(px, py);
        final r = pixel.r.toInt();
        final g = pixel.g.toInt();
        final b = pixel.b.toInt();
        cells[gy * spec.gridWidth + gx] = _nearestPaletteIndex(r, g, b);
      }
    }
    return cells;
  }

  int _nearestPaletteIndex(int r, int g, int b) {
    int bestIdx = 0;
    int bestDist = 1 << 30;
    for (int i = 0; i < 8; i++) {
      final dr = palette[i * 3] - r;
      final dg = palette[i * 3 + 1] - g;
      final db = palette[i * 3 + 2] - b;
      final d = dr * dr + dg * dg + db * db;
      if (d < bestDist) {
        bestDist = d;
        bestIdx = i;
      }
    }
    return bestIdx;
  }

  /// PNG バイト列 → ペイロード。
  Uint8List decodePngBytes(Uint8List pngBytes,
      [FrameSpec spec = FrameSpec.phase0]) {
    final image = img.decodePng(pngBytes);
    if (image == null) {
      throw ArgumentError('failed to decode PNG');
    }
    final cells = imageToCells(image, spec);
    return bridge.decode(cells, spec);
  }
}
