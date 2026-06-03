//! ペイロード bytes をフレームに符号化する。
//!
//! 流れ:
//! 1. ヘッダ (u32 BE length) + ペイロード + ゼロパディング を組み立てる
//! 2. RS ブロック単位 (191 byte) で誤り訂正符号化 → 255 byte ブロック列
//! 3. データバイト総数に揃えてゼロパディング
//! 4. 3 bit/cell でデータセル列にパック
//! 5. キャリブレーション cell に参照色を埋め、データセルを残りの位置に並べる

use crate::ecc::{encode_blocks, RS_DATA_PER_BLOCK};
use crate::frame::{CellRole, Frame, FrameSpec, LENGTH_HEADER_BYTES};
use crate::palette::{Color, BITS_PER_CELL};

#[derive(Debug, PartialEq, Eq)]
pub enum EncodeError {
    PayloadTooLarge { max: usize, given: usize },
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PayloadTooLarge { max, given } => {
                write!(f, "ペイロードが大きすぎる (max={max}, given={given})")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

pub fn encode_payload(payload: &[u8], spec: FrameSpec) -> Result<Frame, EncodeError> {
    let max = spec.max_payload_bytes();
    if payload.len() > max {
        return Err(EncodeError::PayloadTooLarge {
            max,
            given: payload.len(),
        });
    }

    let n_blocks = spec.rs_blocks();
    let data_byte_count = n_blocks * RS_DATA_PER_BLOCK;
    let payload_end = LENGTH_HEADER_BYTES + payload.len();
    let mut data = vec![0u8; data_byte_count];
    data[..LENGTH_HEADER_BYTES].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    data[LENGTH_HEADER_BYTES..payload_end].copy_from_slice(payload);
    // ペイロード後の領域は決定論的 PRNG で埋める。
    // 0 詰めだと符号化後の画像下部が真っ黒の塊になり、ファインダー検出を阻害する。
    fill_with_prng(&mut data, payload_end, data_byte_count);

    let mut encoded = encode_blocks(&data);
    // RS 後の末尾パディング (data_bytes に揃える) も 0 にせず、同じ PRNG で埋める。
    let encoded_len = encoded.len();
    let data_bytes = spec.data_bytes();
    encoded.resize(data_bytes, 0);
    fill_with_prng(&mut encoded, encoded_len, data_bytes);

    let data_cells = pack_bytes_to_cells(&encoded, spec.data_cells());
    let cells = place_cells_with_calibration(spec, &data_cells);

    Ok(Frame::new(spec, cells))
}

/// 区間 [start..end) を決定論的 PRNG (xorshift32) で埋める。
fn fill_with_prng(buffer: &mut [u8], start: usize, end: usize) {
    // 開始位置から派生したシードで初期化することで、長さによらず再現性を保つ。
    let mut state: u32 = 0xCAFE_BABE ^ (start as u32).wrapping_mul(2_654_435_761);
    if state == 0 {
        state = 0x1234_5678;
    }
    for slot in &mut buffer[start..end] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *slot = (state & 0xFF) as u8;
    }
}

/// バイト列を MSB ファーストで 3 bit ずつ取り出してセル列を構築する。
fn pack_bytes_to_cells(bytes: &[u8], cell_count: usize) -> Vec<Color> {
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

/// セルの役割に応じて配置する: Data は data_cells から、Calibration と Finder は固定色から。
fn place_cells_with_calibration(spec: FrameSpec, data_cells: &[Color]) -> Vec<Color> {
    let mut cells = vec![0u8; spec.total_cells()];
    let mut data_idx = 0usize;
    for row in 0..spec.grid_height {
        for col in 0..spec.grid_width {
            let idx = row * spec.grid_width + col;
            cells[idx] = match spec.cell_role(row, col) {
                CellRole::Data => {
                    let c = data_cells[data_idx];
                    data_idx += 1;
                    c
                }
                CellRole::Calibration(c) | CellRole::Finder(c) => c,
            };
        }
    }
    debug_assert_eq!(data_idx, data_cells.len());
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_payload() {
        let spec = FrameSpec::PHASE_0;
        let oversize = vec![0u8; spec.max_payload_bytes() + 1];
        let r = encode_payload(&oversize, spec);
        assert!(matches!(r, Err(EncodeError::PayloadTooLarge { .. })));
    }

    #[test]
    fn encodes_to_expected_cell_count() {
        let spec = FrameSpec::PHASE_0;
        let payload = vec![0xAB; 500];
        let frame = encode_payload(&payload, spec).unwrap();
        assert_eq!(frame.cells.len(), spec.total_cells());
        for &c in &frame.cells {
            assert!(c < 8, "color index out of range: {c}");
        }
    }

    #[test]
    fn calibration_row_filled_with_reference_colors() {
        let spec = FrameSpec::PHASE_0;
        let frame = encode_payload(&vec![0; 100], spec).unwrap();
        // キャリブレーション行 (finder_size の直下) の各 cell が
        // calibration_color_for(col) と一致する (ただしファインダー領域の下は無関係)。
        let cal_row = spec.calibration_row_start;
        for col in 0..spec.grid_width {
            assert_eq!(
                frame.cells[cal_row * spec.grid_width + col],
                spec.calibration_color_for(col)
            );
        }
    }

    #[test]
    fn finder_cells_filled_with_finder_pattern() {
        let spec = FrameSpec::PHASE_0;
        let frame = encode_payload(&vec![0; 100], spec).unwrap();
        // TL の (0,0) は黒 (0)、(1,1) は白 (7)、(3,3) は黒 (0)
        assert_eq!(frame.cells[0], 0);
        assert_eq!(frame.cells[1 * spec.grid_width + 1], 7);
        assert_eq!(frame.cells[3 * spec.grid_width + 3], 0);
    }
}
