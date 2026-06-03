//! Reed-Solomon 誤り訂正符号 (GF(256))。
//!
//! 1 ブロック = 255 byte (191 data + 64 parity)。25% パリティで
//! 各ブロック最大 32 byte の誤りを訂正できる。
//!
//! Phase 0a では誤りなしの往復のみ検証する。誤り訂正能力は Phase 0c で
//! 合成歪み試験により検証する。

use reed_solomon::{Decoder as RsDecoder, Encoder as RsEncoder};

/// 1 ブロックあたりのデータバイト数。
pub const RS_DATA_PER_BLOCK: usize = 191;
/// 1 ブロックあたりのパリティバイト数 (25%)。
pub const RS_PARITY_PER_BLOCK: usize = 64;
/// 1 ブロックの符号語長 (常に 255、GF(256) の上限)。
pub const RS_BLOCK_SIZE: usize = RS_DATA_PER_BLOCK + RS_PARITY_PER_BLOCK;

const _: () = assert!(RS_BLOCK_SIZE == 255);

#[derive(Debug, PartialEq, Eq)]
pub enum EccError {
    /// 受信長がブロックサイズの倍数でない。
    InvalidLength,
    /// RS デコードが訂正不能。
    DecodeFailure,
}

impl core::fmt::Display for EccError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidLength => write!(f, "受信長が RS_BLOCK_SIZE の倍数でない"),
            Self::DecodeFailure => write!(f, "RS デコードに失敗 (訂正能力を超える誤り)"),
        }
    }
}

impl std::error::Error for EccError {}

/// N ブロック分の data (N * RS_DATA_PER_BLOCK byte) を符号化して
/// N * RS_BLOCK_SIZE byte の符号語ストリームを返す。
pub fn encode_blocks(data: &[u8]) -> Vec<u8> {
    debug_assert!(data.len() % RS_DATA_PER_BLOCK == 0);
    let enc = RsEncoder::new(RS_PARITY_PER_BLOCK);
    let n_blocks = data.len() / RS_DATA_PER_BLOCK;
    let mut out = Vec::with_capacity(n_blocks * RS_BLOCK_SIZE);
    for block in data.chunks(RS_DATA_PER_BLOCK) {
        let buf = enc.encode(block);
        out.extend_from_slice(&buf[..]);
    }
    out
}

/// N ブロック分の符号語 (N * RS_BLOCK_SIZE byte) をデコードして
/// N * RS_DATA_PER_BLOCK byte のデータを返す。
pub fn decode_blocks(received: &[u8]) -> Result<Vec<u8>, EccError> {
    if received.len() % RS_BLOCK_SIZE != 0 {
        return Err(EccError::InvalidLength);
    }
    let dec = RsDecoder::new(RS_PARITY_PER_BLOCK);
    let n_blocks = received.len() / RS_BLOCK_SIZE;
    let mut out = Vec::with_capacity(n_blocks * RS_DATA_PER_BLOCK);
    for chunk in received.chunks(RS_BLOCK_SIZE) {
        let recovered = dec.correct(chunk, None).map_err(|_| EccError::DecodeFailure)?;
        out.extend_from_slice(recovered.data());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_roundtrip_single_block() {
        let data: Vec<u8> = (0..RS_DATA_PER_BLOCK).map(|i| i as u8).collect();
        let encoded = encode_blocks(&data);
        assert_eq!(encoded.len(), RS_BLOCK_SIZE);
        let decoded = decode_blocks(&encoded).expect("decode failed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn clean_roundtrip_multi_block() {
        let n = 6;
        let data: Vec<u8> = (0..n * RS_DATA_PER_BLOCK).map(|i| (i % 251) as u8).collect();
        let encoded = encode_blocks(&data);
        assert_eq!(encoded.len(), n * RS_BLOCK_SIZE);
        let decoded = decode_blocks(&encoded).expect("decode failed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn corrects_up_to_half_parity_errors() {
        let data: Vec<u8> = (0..RS_DATA_PER_BLOCK).map(|i| i as u8).collect();
        let mut encoded = encode_blocks(&data);
        // RS_PARITY_PER_BLOCK / 2 = 32 byte までは訂正できる。
        for i in 0..(RS_PARITY_PER_BLOCK / 2) {
            encoded[i * 3] ^= 0xA5;
        }
        let decoded = decode_blocks(&encoded).expect("decode within bound failed");
        assert_eq!(decoded, data);
    }
}
