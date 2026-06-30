# beyond-qr

通信・物理接続なしで、**PC 画面 → スマホカメラ**の光学経路だけで画像/ファイルを送る実験プロジェクト。
PC に QR の動画ループを表示し、スマホ Chrome / Safari で連続スキャンして復元する。

仕組み: ペイロードを Rust の Fountain code (RaptorQ) でパケット列に符号化し、各パケットを QR
として循環表示。受信側はカメラで QR を読み続け、十分なパケットが集まった時点で元データを復元する。
1 枚も取りこぼさずに撮る必要はなく、カメラ位置やフレーム落ちに強い。詳細は `web/README.md`。

## ディレクトリ構成

```
fountain/    RaptorQ ラッパー (Fountain Encoder/Decoder)。本番コアロジック
core-wasm/   wasm-bindgen で FountainEncoder/Decoder をブラウザに公開 (→ web/pkg)
web/         送受信 Web アプリ + 開発用 HTTPS サーバ (実機テストの主役)
qr_bench/    QR 検出率ベンチ (Python / pyzbar)
```

実行時生成物 (いずれも git 管理外・再生成可): `target/` `web/pkg/` `web/certs/` `web/client_logs/`

## 必要環境

- **Rust** (stable) + `wasm-pack` … WASM ビルド用
  ```powershell
  cargo install wasm-pack
  ```
- **Python 3.10+** … 開発用 HTTPS サーバ (`web/serve_https.py`) と QR ベンチ用

## セットアップ

### 1. Python 仮想環境

```powershell
python -m venv .venv
.venv\Scripts\Activate.ps1
pip install cryptography          # HTTPS サーバの自己署名証明書生成に必須
pip install pyzbar qrcode pillow numpy opencv-python   # qr_bench を回す場合のみ
```

### 2. WASM ビルド (初回 / Rust 変更時)

```powershell
cd core-wasm
wasm-pack build --target web --release --out-dir ..\web\pkg
```

`web/pkg/` は git 管理外。クローン直後は必ずこのビルドが必要。

## 実機テスト (PC → スマホ)

PC とスマホを同じ Wi-Fi に接続し、HTTPS サーバを起動する:

```powershell
python web\serve_https.py
```

- PC ブラウザ: `https://localhost:8443/sender.html` で画像/テキストを送信
- スマホ Chrome: `https://<PC の IP>:8443/receiver.html` でカメラ受信
  (IP は起動時にコンソール表示。証明書警告は「詳細設定 → アクセスする」で許容)

カメラ不要の往復テストは `https://localhost:8443/test_phase1.html`。
手順・トラブルシューティングの詳細は **[web/README.md](web/README.md)** を参照。

## ビルド / テスト

```powershell
cargo test --manifest-path fountain/Cargo.toml   # Fountain コアの単体テスト
```

## 備考

- `serve_https.py` は開発専用 (自己署名証明書・no-cache・LAN 内限定)。本番用途には使わない。
  受信状況を送受信間で共有する `/state` バックチャネルや、診断用 `/log` `/capture`
  エンドポイントを持つ。
- 経緯: Phase 0 の 8 色カラーセル方式は実機のモアレ + 色 ISP で破綻し、Phase 1 で B/W の
  QR + Fountain 動画ループにピボットして実機転送に成功した。
