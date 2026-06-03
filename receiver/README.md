# beyond-qr-receiver

beyond-qr の受信側 Flutter アプリ。Phase 0d-A 時点では Windows desktop で
Rust core を FFI 経由で呼ぶスキャフォールドが動作する。

## 構成

- `lib/src/bridge.dart` — Rust `core-ffi` (cdylib) を Dart FFI で呼ぶラッパー
- `lib/src/png_decoder.dart` — PNG → セル列 → ペイロード のクリーン版パイプライン
- `test/decode_png_test.dart` — E2E テスト (PNG → Dart → Rust → byte 完全一致)

## ビルド・テスト手順

```powershell
# 1. Rust FFI DLL をビルド (workspace ルートで)
cd C:\projects\beyond-qr
cargo build --release -p beyond-qr-core-ffi

# 2. Python 側でサンプル PNG を生成 (一度のみ)
.\.venv\Scripts\python.exe -m beyond_qr_sender.encode samples\input.bin -o samples\frame.png

# 3. Flutter 依存取得
cd receiver
flutter pub get

# 4. テスト実行
flutter test
```

## TODO (Phase 0e / Phase 1 で対応)

- カメラプレビューと静止撮影 (`camera` plugin)
- 歪み画像対応 (透視 unwarp + キャリブレーション + OKLab 量子化を Dart 側にも移植)
- Android / iOS ターゲット (現状 Windows desktop のみ)
