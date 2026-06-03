# beyond-qr web (Phase 0e: スマホブラウザ実機テスト)

PC 画面に表示した beyond-qr フレームを、**スマホ Chrome / Safari** のカメラで読み取って
復号する Web アプリ。Rust core は WASM 化、画像処理は JS で実装。

## ファイル構成

```
web/
├── index.html         # スマホ側受信 UI (カメラ → 撮影 → 復号 → 表示)
├── display.html       # PC 側送信 UI (frame.png を全画面表示)
├── algo.js            # JS 画像処理 (find finder / perspective unwarp / calibration / OKLab)
├── pkg/               # wasm-pack 出力 (core-wasm)
├── samples/           # テスト用フレーム
├── certs/             # 自己署名証明書 (初回起動時に自動生成)
├── serve_https.py     # 開発用 HTTPS サーバー
├── test_wasm.html     # WASM 単体テスト (PC ブラウザでアクセス)
├── test_algo.html     # JS 画像処理 + WASM 復号テスト (frame.png を fetch)
└── README.md          # 本ファイル
```

## セットアップ手順

### 1. WASM ビルド (初回 / Rust 変更時)

```powershell
cd C:\projects\beyond-qr\core-wasm
wasm-pack build --target web --release --out-dir ..\web\pkg
```

### 2. サンプル PNG 生成 (一度のみ)

```powershell
cd C:\projects\beyond-qr
.\.venv\Scripts\python.exe -m beyond_qr_sender.encode samples\input.bin -o samples\frame.png
# web/samples/ にコピー
Copy-Item samples\frame.png web\samples\frame.png -Force
Copy-Item samples\input.bin web\samples\input.bin -Force
```

### 3. ファイアウォール許可 (初回のみ、管理者 PowerShell)

```powershell
New-NetFirewallRule -DisplayName "beyond-qr HTTPS 8443" -Direction Inbound -Protocol TCP -LocalPort 8443 -Action Allow -Profile Private
```

(または Python の最初の起動時に Windows のポップアップで「アクセスを許可」を選ぶ)

### 4. HTTPS サーバー起動

```powershell
.\.venv\Scripts\python.exe web\serve_https.py
```

初回起動時に `web/certs/` 配下に自己署名証明書を自動生成する。

### 5. PC 側で送信フレーム表示

PC ブラウザで `https://localhost:8443/display.html` を開き、F11 で全画面表示。

### 6. スマホでアクセス

スマホを **PC と同じ Wi-Fi に接続**。スマホ Chrome で以下を開く:

```
https://<PC の IP>:8443/index.html
```

(IP はサーバー起動時にコンソールに表示される。例: `https://192.168.11.52:8443/`)

**証明書警告**: 「詳細設定」→「<host> にアクセスする (安全ではありません)」を選択。
自己署名証明書なので警告が出るのは正常。

### 7. 撮影と復号

1. スマホ画面の「カメラ開始」をタップ → カメラ許可
2. 背面カメラのプレビュー表示 (環境光モード)
3. PC 画面 (全画面表示中の frame.png) にスマホをかざす
4. 黄色の枠内に画像を収める
5. 「撮影 & 復号」をタップ
6. 結果ペイロードが画面下部に表示される

## トラブルシューティング

- **カメラ起動エラー** → HTTPS 経由でアクセスしているか確認 (HTTP では getUserMedia 不可)
- **WASM 読み込み失敗** → pkg/ が web/ 下に正しくビルドされているか確認
- **復号失敗 (Decode failure)** → 撮影距離が遠すぎる / 手ぶれ / 反射光、画面のオートブライトネスを切る
- **接続できない** → ファイアウォール許可、PC と同じ Wi-Fi、IP アドレスが合っているか

## 既知の制限 (Phase 0e 時点)

- 静止 1 フレームのみ復号。動画ストリーム (Phase 1) は未実装
- 4 隅のファインダーが画像内にすべて収まる必要がある
- 極端な暗所・極端な反射では復号失敗する (撮影条件に依存)
