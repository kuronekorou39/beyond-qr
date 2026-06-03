// Phase 0d-A 受け入れテスト: クリーン PNG → Rust FFI 復号 → ペイロード byte 一致。

import 'dart:io';
import 'dart:typed_data';

import 'package:beyond_qr_receiver/beyond_qr_receiver.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:path/path.dart' as p;

void main() {
  test('Phase 0d-A: clean PNG roundtrip via Dart FFI', () {
    // samples/ は workspace ルートにある (receiver の 1 階層上)
    final samplesDir = p.join('..', 'samples');
    final pngPath = p.join(samplesDir, 'frame.png');
    final inputPath = p.join(samplesDir, 'input.bin');

    expect(File(pngPath).existsSync(), isTrue,
        reason: 'samples/frame.png が見つからない。先に Python で生成してください。');
    expect(File(inputPath).existsSync(), isTrue,
        reason: 'samples/input.bin が見つからない。');

    final bridge = BeyondQrBridge.loadDefault();
    final decoder = PngDecoder(bridge);

    final pngBytes = File(pngPath).readAsBytesSync();
    final expectedPayload = File(inputPath).readAsBytesSync();

    final recovered = decoder.decodePngBytes(pngBytes);

    expect(recovered.length, expectedPayload.length,
        reason: '復号ペイロード長が一致しない');
    expect(recovered, expectedPayload, reason: '復号ペイロードがバイト一致しない');
  });

  test('Phase 0d-A: bridge can read palette', () {
    final bridge = BeyondQrBridge.loadDefault();
    final palette = bridge.paletteRgb();
    expect(palette.length, 24);
    // 黒 (palette[0])
    expect(palette[0], 0);
    expect(palette[1], 0);
    expect(palette[2], 0);
    // 白 (palette[7])
    expect(palette[21], 255);
    expect(palette[22], 255);
    expect(palette[23], 255);
  });

  test('Phase 0d-A: encode then decode in Dart', () {
    final bridge = BeyondQrBridge.loadDefault();
    const spec = FrameSpec.phase0;
    final payload = List<int>.generate(200, (i) => (i * 73) & 0xFF);
    final payloadBytes = Uint8List.fromList(payload);

    final cells = bridge.encode(payloadBytes, spec);
    expect(cells.length, spec.totalCells);

    final recovered = bridge.decode(cells, spec);
    expect(recovered, payloadBytes);
  });
}
