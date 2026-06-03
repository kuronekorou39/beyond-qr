/// beyond-qr-receiver: Flutter 側エントリポイント。
///
/// Phase 0d-A: Windows desktop で Rust core を FFI 経由で呼び、PNG → ペイロード
/// の往復を検証する。
library beyond_qr_receiver;

export 'src/bridge.dart';
export 'src/png_decoder.dart';
