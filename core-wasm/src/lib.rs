//! beyond-qr の WebAssembly バインディング (Phase 1: Fountain code)。
//!
//! `wasm-pack build --target web --release` で `pkg/` に JS モジュールを生成する。
//! JS 側からは `import init, { FountainEncoder, FountainDecoder } from "./pkg/beyond_qr_core_wasm.js"` で使う。

use ::beyond_qr_fountain as fountain;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {}

// =============================================================
// Phase 1: Fountain code (RaptorQ) over QR streaming
// =============================================================

/// JS から見える Fountain エンコーダのハンドル。
#[wasm_bindgen]
pub struct FountainEncoder {
    inner: fountain::Encoder,
}

#[wasm_bindgen]
impl FountainEncoder {
    /// payload を packet_size byte で符号化。extra_repair はリペアパケットの追加数 (損失耐性向上)。
    #[wasm_bindgen(constructor)]
    pub fn new(payload: &[u8], packet_size: u16, extra_repair: u32) -> FountainEncoder {
        FountainEncoder {
            inner: fountain::Encoder::new(payload, packet_size, extra_repair),
        }
    }

    /// 受信側に渡す 12 byte の OTI を返す (decoder 構築に必須)。
    #[wasm_bindgen(js_name = otiBytes)]
    pub fn oti_bytes(&self) -> Vec<u8> {
        self.inner.oti_bytes().to_vec()
    }

    /// 生成済みパケットの総数。送信側はこの値で循環表示する。
    #[wasm_bindgen(js_name = packetCount)]
    pub fn packet_count(&self) -> usize {
        self.inner.packet_count()
    }

    /// i 番目のシリアライズ済みパケット (4 byte ID + symbol_size byte data)。
    pub fn packet(&self, i: usize) -> Vec<u8> {
        self.inner.packet(i)
    }
}

/// JS から見える Fountain デコーダのハンドル。
#[wasm_bindgen]
pub struct FountainDecoder {
    inner: fountain::Decoder,
    last_result: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl FountainDecoder {
    /// 12 byte の OTI から初期化する。
    #[wasm_bindgen(constructor)]
    pub fn new(oti_bytes: &[u8]) -> Result<FountainDecoder, JsError> {
        if oti_bytes.len() != 12 {
            return Err(JsError::new(&format!("OTI must be 12 bytes, got {}", oti_bytes.len())));
        }
        let mut arr = [0u8; 12];
        arr.copy_from_slice(oti_bytes);
        Ok(FountainDecoder {
            inner: fountain::Decoder::from_oti_bytes(&arr),
            last_result: None,
        })
    }

    /// パケットを 1 つ追加。復元できれば内部に payload を保存し true を返す。
    #[wasm_bindgen(js_name = addPacket)]
    pub fn add_packet(&mut self, packet_bytes: &[u8]) -> bool {
        if self.last_result.is_some() {
            return true;
        }
        if let Some(result) = self.inner.add_packet(packet_bytes) {
            self.last_result = Some(result);
            true
        } else {
            false
        }
    }

    /// 復元済みなら payload を返す。
    pub fn payload(&self) -> Option<Vec<u8>> {
        self.last_result.clone()
    }

    /// 受信パケット数。
    #[wasm_bindgen(js_name = packetsReceived)]
    pub fn packets_received(&self) -> u32 {
        self.inner.packets_received()
    }

    /// 想定 payload サイズ (byte)。
    #[wasm_bindgen(js_name = payloadSize)]
    pub fn payload_size(&self) -> u64 {
        self.inner.payload_size()
    }
}
