// beyond-qr PC PWA — エントリ (結線)。
// Rust コア (fountain) を WASM で初期化し、送信 (Sender) / 受信 (Receiver) を UI に繋ぐ。

import init, { FountainEncoder, FountainDecoder } from "./pkg/beyond_qr_core_wasm.js";
import { Sender } from "./sender.js";
import { Receiver } from "./receiver.js";

const $ = (id) => document.getElementById(id);

// ---- コア自己診断 ----
function selfTest() {
  const n = 5000;
  const payload = new Uint8Array(n);
  for (let i = 0; i < n; i++) payload[i] = (i * 37 + 11) & 0xff;
  const enc = new FountainEncoder(payload, 300, 20);
  const dec = new FountainDecoder(enc.otiBytes());
  const total = enc.packetCount();
  let recovered = null;
  for (let i = 0; i < total && recovered === null; i++) {
    if (dec.addPacket(enc.packet(i))) recovered = dec.payload();
  }
  const ok = recovered && recovered.length === n &&
    recovered[0] === payload[0] && recovered[n - 1] === payload[n - 1];
  const el = $("coreStatus");
  el.className = ok ? "ok" : "ng";
  el.textContent = ok ? `コア: OK (${total} pkt 往復)` : "コア異常";
  return ok;
}

// ---- タブ ----
function showPane(which) {
  $("paneSend").classList.toggle("active", which === "send");
  $("paneRecv").classList.toggle("active", which === "recv");
  $("tabSend").classList.toggle("active", which === "send");
  $("tabRecv").classList.toggle("active", which === "recv");
}
$("tabSend").addEventListener("click", () => showPane("send"));
$("tabRecv").addEventListener("click", () => showPane("recv"));

// ---- 送信 ----
const sender = new Sender({
  canvas: $("txCanvas"),
  onStatus: (s) => { $("txStatus").textContent = s; },
});

function fitTxCanvas() {
  // 画面に収まる最大の正方形で表示 (下部バーの分を少し引く)
  const size = Math.min(window.innerWidth, window.innerHeight - 56) - 8;
  $("txCanvas").style.width = `${size}px`;
  $("txCanvas").style.height = `${size}px`;
}
window.addEventListener("resize", fitTxCanvas);

$("txStart").addEventListener("click", async () => {
  const file = $("txFile").files[0];
  if (!file) { $("txInfo").textContent = "ファイルを選択してください"; return; }
  const grid = $("txGrid").value;
  const ec = $("txEc").value;
  const fps = Math.max(2, Math.min(30, parseInt($("txFps").value) || 10));
  $("txInfo").textContent = "";
  fitTxCanvas();
  $("txStage").style.display = "flex";
  try {
    await sender.start(file, grid, ec, fps);
  } catch (e) {
    $("txStage").style.display = "none";
    $("txInfo").textContent = "送信エラー: " + (e && e.message ? e.message : e);
  }
});
$("txStop").addEventListener("click", () => {
  sender.stop();
  $("txStage").style.display = "none";
});

// ---- 受信 ----
const fmtSize = (n) => {
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(2) + "GB";
  if (n >= 1 << 20) return (n / (1 << 20)).toFixed(1) + "MB";
  if (n >= 1024) return (n / 1024).toFixed(1) + "KB";
  return n + "B";
};

const receiver = new Receiver({
  video: $("rxVideo"),
  onProgress: ({ name, size, done, total, inflight }) => {
    $("rxProgress").value = total ? done / total : 0;
    $("rxInfo").textContent = name
      ? `${name} (${fmtSize(size)}) · ブロック ${done}/${total} · 進行中 ${inflight}`
      : "マニフェスト待ち...";
  },
  onDone: ({ name, type, size, blob }) => {
    $("rxProgress").value = 1;
    $("rxInfo").innerHTML = `<span class="ok">✅ 復元成功: ${name} (${fmtSize(size)})</span>`;
    const url = URL.createObjectURL(blob);
    const isImage = type.startsWith("image/");
    $("rxResult").innerHTML =
      (isImage ? `<p><img src="${url}" style="max-width:100%;border-radius:8px" /></p>` : "") +
      `<p><a href="${url}" download="${name}"><button>ダウンロード: ${name}</button></a></p>`;
    $("rxStart").disabled = false;
    $("rxStop").disabled = true;
  },
});

$("rxStart").addEventListener("click", async () => {
  $("rxResult").innerHTML = "";
  $("rxProgress").value = 0;
  try {
    await receiver.start($("rxGrid").value);
    $("rxStart").disabled = true;
    $("rxStop").disabled = false;
    $("rxInfo").textContent = "スキャン中 — 送信側の QR に向けてください";
  } catch (e) {
    $("rxInfo").textContent = "カメラ起動失敗: " + (e && e.message ? e.message : e);
  }
});
$("rxStop").addEventListener("click", () => {
  receiver.stop();
  $("rxStart").disabled = false;
  $("rxStop").disabled = true;
  $("rxInfo").textContent = "停止しました";
});

// ---- 起動 ----
(async () => {
  try {
    await init();
    selfTest();
  } catch (e) {
    const el = $("coreStatus");
    el.className = "ng";
    el.textContent = "コア読み込み失敗: " + (e && e.message ? e.message : e);
  }
})();

if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("./sw.js").catch(() => {});
  });
}
