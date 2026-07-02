// beyond-qr PC PWA — エントリ。
// まずは Rust コア (Fountain) を WASM で読み込み、encode→decode 往復で動作検証する。
// スマホアプリと同一の fountain crate を再利用しているため、プロトコルも一致させられる。

import init, { FountainEncoder, FountainDecoder } from "./pkg/beyond_qr_core_wasm.js";

const statusEl = document.getElementById("coreStatus");

async function selfTest() {
  // 5KB のテストデータを Fountain で符号化→復号し、一致を確認
  const n = 5000;
  const payload = new Uint8Array(n);
  for (let i = 0; i < n; i++) payload[i] = (i * 37 + 11) & 0xff;

  const enc = new FountainEncoder(payload, 300, 20);
  const oti = enc.otiBytes();
  const total = enc.packetCount();
  const dec = new FountainDecoder(oti);

  let recovered = null;
  for (let i = 0; i < total && recovered === null; i++) {
    if (dec.addPacket(enc.packet(i))) recovered = dec.payload();
  }

  const ok =
    recovered && recovered.length === payload.length &&
    recovered[0] === payload[0] && recovered[recovered.length - 1] === payload[payload.length - 1];

  statusEl.className = ok ? "ok" : "ng";
  statusEl.textContent = ok
    ? `Rust Fountain コア: OK  (${n}B を ${total} パケットで往復)`
    : "コア異常: 復元不一致";
}

async function main() {
  try {
    await init();
    await selfTest();
  } catch (e) {
    statusEl.className = "ng";
    statusEl.textContent = "コア読み込み失敗: " + (e && e.message ? e.message : e);
  }
}

main();

// PWA: オフライン対応の Service Worker 登録 (HTTPS / localhost のみ)
if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("./sw.js").catch(() => {});
  });
}
