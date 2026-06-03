//! フレームからペイロード bytes を復号する。
//!
//! 流れ:
//! 1. キャリブレーション行をスキップし、データセルだけを row-major 順に集める
//! 2. データセル列を 3 bit/cell でバイト列に展開
//! 3. 先頭 n_blocks * RS_BLOCK_SIZE byte を RS デコード
//! 4. 先頭 4 byte のヘッダから長さを読み、ペイロードを切り出す

use crate::ecc::{decode_blocks, EccError, RS_BLOCK_SIZE};
use crate::frame::{CellRole, Frame, FrameSpec, LENGTH_HEADER_BYTES};
use crate::palette::{Color, BITS_PER_CELL};

#[derive(Debug)]
pub enum DecodeError {
    Ecc(EccError),
    TruncatedHeader,
    InvalidLength { declared: usize, available: usize },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ecc(e) => write!(f, "RS デコード失敗: {e}"),
            Self::TruncatedHeader => write!(f, "長さヘッダが切れている"),
            Self::InvalidLength { declared, available } => write!(
                f,
                "宣言長 {declared} が利用可能領域 {available} を超えている"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<EccError> for DecodeError {
    fn from(e: EccError) -> Self {
        Self::Ecc(e)
    }
}

pub fn decode_payload(frame: &Frame) -> Result<Vec<u8>, DecodeError> {
    let spec = frame.spec;
    let data_cells = extract_data_cells(spec, &frame.cells);
    let bytes = unpack_cells_to_bytes(&data_cells, spec.data_bytes());

    let codeword_bytes = spec.rs_blocks() * RS_BLOCK_SIZE;
    let data = decode_blocks(&bytes[..codeword_bytes])?;

    if data.len() < LENGTH_HEADER_BYTES {
        return Err(DecodeError::TruncatedHeader);
    }
    let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let available = data.len() - LENGTH_HEADER_BYTES;
    if len > available {
        return Err(DecodeError::InvalidLength {
            declared: len,
            available,
        });
    }
    Ok(data[LENGTH_HEADER_BYTES..LENGTH_HEADER_BYTES + len].to_vec())
}

/// ファインダー / キャリブレーション cell を除外してデータセルを row-major 順に取り出す。
fn extract_data_cells(spec: FrameSpec, cells: &[Color]) -> Vec<Color> {
    let mut data = Vec::with_capacity(spec.data_cells());
    for row in 0..spec.grid_height {
        for col in 0..spec.grid_width {
            if matches!(spec.cell_role(row, col), CellRole::Data) {
                let idx = row * spec.grid_width + col;
                data.push(cells[idx]);
            }
        }
    }
    data
}

/// セル列を MSB ファーストで連結してバイト列を復元する。
pub(crate) fn unpack_cells_to_bytes(cells: &[Color], target_bytes: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(target_bytes);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for &c in cells {
        buffer = (buffer << BITS_PER_CELL) | (c as u32 & 0b111);
        bits += BITS_PER_CELL;
        while bits >= 8 && bytes.len() < target_bytes {
            let shift = bits - 8;
            let byte = ((buffer >> shift) & 0xFF) as u8;
            buffer &= (1u32 << shift) - 1;
            bits -= 8;
            bytes.push(byte);
        }
        if bytes.len() == target_bytes {
            break;
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::encode_payload;
    use crate::frame::FrameSpec;

    #[test]
    fn unpack_inverse_of_pack_simple() {
        let bytes = vec![0xABu8, 0xCD, 0xEF];
        let cells = pack_bytes_to_cells_helper(&bytes, 8);
        let back = unpack_cells_to_bytes(&cells, 3);
        assert_eq!(back, bytes);
    }

    fn pack_bytes_to_cells_helper(bytes: &[u8], cell_count: usize) -> Vec<Color> {
        let mut cells = Vec::with_capacity(cell_count);
        let mut buffer: u32 = 0;
        let mut bits: u32 = 0;
        let mut idx = 0usize;
        while cells.len() < cell_count {
            if bits < BITS_PER_CELL {
                if idx < bytes.len() {
                    buffer = (buffer << 8) | bytes[idx] as u32;
                    bits += 8;
                    idx += 1;
                } else {
                    buffer <<= BITS_PER_CELL - bits;
                    bits = BITS_PER_CELL;
                }
                continue;
            }
            let shift = bits - BITS_PER_CELL;
            let c = ((buffer >> shift) & 0b111) as Color;
            buffer &= (1u32 << shift) - 1;
            bits -= BITS_PER_CELL;
            cells.push(c);
        }
        cells
    }

    #[test]
    fn roundtrip_500_byte_payload() {
        let spec = FrameSpec::PHASE_0;
        let payload: Vec<u8> = (0..500).map(|i| (i as u8).wrapping_mul(37)).collect();
        let frame = encode_payload(&payload, spec).expect("encode");
        let recovered = decode_payload(&frame).expect("decode");
        assert_eq!(recovered, payload);
    }

    #[test]
    fn roundtrip_with_no_calibration() {
        let spec = FrameSpec {
            calibration_rows: 0,
            ..FrameSpec::PHASE_0
        };
        let payload: Vec<u8> = (0..500).map(|i| (i as u8).wrapping_mul(13)).collect();
        let frame = encode_payload(&payload, spec).expect("encode");
        let recovered = decode_payload(&frame).expect("decode");
        assert_eq!(recovered, payload);
    }
}
