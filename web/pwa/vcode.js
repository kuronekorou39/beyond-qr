// PC 用 vcode 送受信。スマホアプリの vcode と同形式:
//   - 送信: 生ペイロード全体を VcodeTx で符号化 (チャンク無し=単一ペイロード)、フレーム循環表示
//   - 受信: カメラ→輝度Y→VcodeRx.scan→パケット→FountainDecoder→生バイト→型sniff→保存
// 受信側で CANDIDATES に無い格子は検出できないため、格子は 7x6 / 5x4 のみ。

import { VcodeTx, VcodeRx, FountainDecoder } from "./pkg/beyond_qr_core_wasm.js";

const REPAIR_RATE = 0.5;

export class VcodeSender {
  constructor({ canvas, onStatus }) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.onStatus = onStatus;
    this.off = document.createElement("canvas");
    this.running = false;
    this.seq = 0;
  }

  async start(fileOrBytes, gridStr, bpc, fps) {
    const payload = fileOrBytes instanceof Uint8Array
      ? fileOrBytes
      : new Uint8Array(await fileOrBytes.arrayBuffer());
    const [gw, gh] = gridStr.split("x").map(Number);
    const packetSize = bpc === 2 ? 94 : 44;
    const sourcePackets = Math.ceil(payload.length / packetSize);
    const extraRepair = Math.ceil(sourcePackets * REPAIR_RATE);
    const tx = new VcodeTx(payload, extraRepair, gw, gh, bpc);
    this.tx = tx;
    this.w = tx.frameWidth();
    this.h = tx.frameHeight();
    this.frameCount = tx.frameCount();
    this.off.width = this.w;
    this.off.height = this.h;
    this.running = true;
    const mySeq = ++this.seq;
    this._loop(mySeq, fps, payload.length);
  }

  _drawFrame(i) {
    const gray = this.tx.frameGray(i);
    const octx = this.off.getContext("2d");
    const id = octx.createImageData(this.w, this.h);
    for (let p = 0; p < this.w * this.h; p++) {
      const v = gray[p];
      id.data[p * 4] = v; id.data[p * 4 + 1] = v; id.data[p * 4 + 2] = v; id.data[p * 4 + 3] = 255;
    }
    octx.putImageData(id, 0, 0);

    const { ctx, canvas } = this;
    ctx.fillStyle = "#fff";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.imageSmoothingEnabled = false;
    // アスペクト維持で中央にフィット
    const scale = Math.min(canvas.width / this.w, canvas.height / this.h);
    const dw = Math.floor(this.w * scale), dh = Math.floor(this.h * scale);
    const dx = ((canvas.width - dw) / 2) | 0, dy = ((canvas.height - dh) / 2) | 0;
    ctx.drawImage(this.off, 0, 0, this.w, this.h, dx, dy, dw, dh);
  }

  async _loop(mySeq, fps, payloadLen) {
    const interval = 1000 / fps;
    let idx = 0, pass = 0;
    while (this.running && mySeq === this.seq) {
      const t0 = performance.now();
      this._drawFrame(idx % this.frameCount);
      idx++;
      if (idx % this.frameCount === 0) pass++;
      if (mySeq !== this.seq) break;
      this.onStatus(`vcode 送信中 · ${payloadLen}B · frame ${idx} · ${pass + 1} 巡目`);
      const dt = performance.now() - t0;
      if (dt < interval) await new Promise((r) => setTimeout(r, interval - dt));
    }
  }

  stop() { this.seq++; this.running = false; }
}

export class VcodeReceiver {
  constructor({ video, onProgress, onDone, onError }) {
    this.video = video;
    this.onProgress = onProgress;
    this.onDone = onDone;
    this.onError = onError;
    this.cap = document.createElement("canvas");
    this.stream = null;
    this.rafId = null;
  }

  async start() {
    this._reset();
    this.stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: "environment", width: { ideal: 1920 }, height: { ideal: 1080 } },
      audio: false,
    });
    this.video.srcObject = this.stream;
    await this.video.play();
    this.rx = new VcodeRx();
    const loop = () => { if (!this.stream) return; this._scan(); this.rafId = requestAnimationFrame(loop); };
    this.rafId = requestAnimationFrame(loop);
  }

  _reset() {
    this.rx = null; this.dec = null; this.finished = false; this.frames = 0; this.detected = 0;
  }

  stop() {
    if (this.rafId) cancelAnimationFrame(this.rafId);
    this.rafId = null;
    if (this.stream) { this.stream.getTracks().forEach((t) => t.stop()); this.stream = null; }
  }

  _scan() {
    if (this.finished) return;
    const vw = this.video.videoWidth, vh = this.video.videoHeight;
    if (!vw || !vh) return;
    // 中央正方形を最大 1280 に
    const crop = Math.min(vw, vh);
    const target = Math.min(1280, crop);
    const cx = (vw - crop) >> 1, cy = (vh - crop) >> 1;
    this.cap.width = target; this.cap.height = target;
    const ctx = this.cap.getContext("2d", { willReadFrequently: true });
    ctx.drawImage(this.video, cx, cy, crop, crop, 0, 0, target, target);
    const rgba = ctx.getImageData(0, 0, target, target).data;
    // RGBA → 輝度 Y
    const gray = new Uint8Array(target * target);
    for (let p = 0, q = 0; p < gray.length; p++, q += 4) {
      gray[p] = (rgba[q] * 77 + rgba[q + 1] * 150 + rgba[q + 2] * 29) >> 8;
    }
    this.frames++;
    let report;
    try {
      report = this.rx.scan(gray, target, target, target, 0, 0.9);
    } catch (_) { return; }
    if (report.detected) {
      this.detected++;
      if (!this.dec) {
        try { this.dec = new FountainDecoder(report.oti); } catch (_) { return; }
      }
      const n = report.packetCount();
      let done = false;
      for (let i = 0; i < n; i++) {
        if (this.dec.addPacket(report.packet(i))) { done = true; break; }
      }
      this.onProgress({ frames: this.frames, detected: this.detected,
        blocks: report.blocksOk, blocksTotal: report.blocksTotal });
      if (done) { this._finish(this.dec.payload()); }
    } else {
      this.onProgress({ frames: this.frames, detected: this.detected, blocks: 0, blocksTotal: 0 });
    }
  }

  _finish(payload) {
    this.finished = true;
    this.stop();
    const [ext, mime] = sniffType(payload);
    const blob = new Blob([payload], { type: mime });
    const name = `vcode_${new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19)}.${ext}`;
    this.onDone({ name, type: mime, size: payload.length, blob });
  }
}

// スマホアプリ _sniffType と同一の型推定
function sniffType(b) {
  if (b.length > 3 && b[0] === 0xff && b[1] === 0xd8) return ["jpg", "image/jpeg"];
  if (b.length > 7 && b[0] === 0x89 && b[1] === 0x50) return ["png", "image/png"];
  if (b.length > 11 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50) return ["webp", "image/webp"];
  if (b.length > 3 && b[0] === 0x25 && b[1] === 0x50 && b[2] === 0x44 && b[3] === 0x46) return ["pdf", "application/pdf"];
  if (b.length > 1 && b[0] === 0x50 && b[1] === 0x4b) return ["zip", "application/zip"];
  const probe = b.subarray(0, 4096);
  let ctrl = 0;
  for (const c of probe) if (c < 9 || (c > 13 && c < 32) || c === 127) ctrl++;
  if (probe.length && ctrl / probe.length < 0.02) return ["txt", "text/plain;charset=utf-8"];
  return ["bin", "application/octet-stream"];
}
