//! beyond-qr-core の C ABI ラッパー。Dart FFI / flutter_rust_bridge から呼ぶ。
//!
//! 設計方針:
//! - 入力 (cells / payload) は呼び出し側がバッファを確保
//! - 出力は呼び出し側が確保したバッファに書き込み、書き込み byte 数を返す
//! - エラー時は負値を返す (-1: 入力サイズ不整合, -2: ペイロード過大, -3: RS デコード失敗 等)

use beyond_qr_core as bqc;
use std::os::raw::c_int;

/// FFI 共通エラーコード。
#[repr(i32)]
enum FfiError {
    /// 入出力のサイズ不整合 (cells.len() != total_cells など)
    SizeMismatch = -1,
    /// ペイロードが容量を超える
    PayloadTooLarge = -2,
    /// RS デコード失敗 (訂正能力超過)
    DecodeFailure = -3,
    /// ヘッダの長さフィールドが不正
    InvalidLength = -4,
    /// 出力バッファが小さい
    OutputTooSmall = -5,
}

fn make_spec(
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
) -> bqc::FrameSpec {
    bqc::FrameSpec {
        grid_width,
        grid_height,
        cell_px,
        finder_size,
        calibration_row_start,
        calibration_rows,
    }
}

/// ペイロードをセル列に符号化する。
///
/// # 安全性
/// `payload` は `payload_len` 要素の読み取り可能配列、`out_cells` は
/// `out_cells_capacity` 要素の書き込み可能配列を指している必要がある。
///
/// 返り値: 成功時は書き込んだセル数 (= total_cells)、失敗時は負のエラーコード。
#[no_mangle]
pub unsafe extern "C" fn bqc_encode(
    payload: *const u8,
    payload_len: usize,
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
    out_cells: *mut u8,
    out_cells_capacity: usize,
) -> c_int {
    let spec = make_spec(
        grid_width,
        grid_height,
        cell_px,
        finder_size,
        calibration_row_start,
        calibration_rows,
    );
    let total_cells = spec.total_cells();
    if out_cells_capacity < total_cells {
        return FfiError::OutputTooSmall as c_int;
    }
    let payload_slice = if payload_len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(payload, payload_len) }
    };
    let frame = match bqc::encode_payload(payload_slice, spec) {
        Ok(f) => f,
        Err(bqc::EncodeError::PayloadTooLarge { .. }) => {
            return FfiError::PayloadTooLarge as c_int
        }
    };
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out_cells, total_cells) };
    out_slice.copy_from_slice(&frame.cells);
    total_cells as c_int
}

/// セル列をペイロードに復号する。
///
/// 返り値: 成功時は書き込んだペイロード byte 数、失敗時は負のエラーコード。
#[no_mangle]
pub unsafe extern "C" fn bqc_decode(
    cells: *const u8,
    cells_len: usize,
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
    out_payload: *mut u8,
    out_payload_capacity: usize,
) -> c_int {
    let spec = make_spec(
        grid_width,
        grid_height,
        cell_px,
        finder_size,
        calibration_row_start,
        calibration_rows,
    );
    if cells_len != spec.total_cells() {
        return FfiError::SizeMismatch as c_int;
    }
    let cells_vec = unsafe { std::slice::from_raw_parts(cells, cells_len) }.to_vec();
    let frame = bqc::Frame::new(spec, cells_vec);
    let payload = match bqc::decode_payload(&frame) {
        Ok(p) => p,
        Err(bqc::DecodeError::Ecc(_)) => return FfiError::DecodeFailure as c_int,
        Err(bqc::DecodeError::TruncatedHeader) => return FfiError::InvalidLength as c_int,
        Err(bqc::DecodeError::InvalidLength { .. }) => return FfiError::InvalidLength as c_int,
    };
    if out_payload_capacity < payload.len() {
        return FfiError::OutputTooSmall as c_int;
    }
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out_payload, payload.len()) };
    out_slice.copy_from_slice(&payload);
    payload.len() as c_int
}

/// 指定 spec の最大ペイロード byte 数を返す。
#[no_mangle]
pub extern "C" fn bqc_max_payload_bytes(
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
) -> usize {
    make_spec(
        grid_width,
        grid_height,
        cell_px,
        finder_size,
        calibration_row_start,
        calibration_rows,
    )
    .max_payload_bytes()
}

/// 指定 spec の総セル数を返す。
#[no_mangle]
pub extern "C" fn bqc_total_cells(
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
) -> usize {
    make_spec(
        grid_width,
        grid_height,
        cell_px,
        finder_size,
        calibration_row_start,
        calibration_rows,
    )
    .total_cells()
}

/// パレット 8 色の sRGB を out_rgb (24 byte) に書き込む。
/// out_rgb は [r0, g0, b0, r1, g1, b1, ..., r7, g7, b7]。
#[no_mangle]
pub unsafe extern "C" fn bqc_palette_rgb(out_rgb: *mut u8, out_capacity: usize) -> c_int {
    if out_capacity < 24 {
        return FfiError::OutputTooSmall as c_int;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(out_rgb, 24) };
    for (i, rgb) in bqc::palette::PALETTE.iter().enumerate() {
        out[i * 3] = rgb.r;
        out[i * 3 + 1] = rgb.g;
        out[i * 3 + 2] = rgb.b;
    }
    24
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_roundtrip_500_byte() {
        let payload: Vec<u8> = (0..500).map(|i| (i as u8).wrapping_mul(31)).collect();
        let spec = bqc::FrameSpec::PHASE_0;
        let total_cells = spec.total_cells();
        let mut cells = vec![0u8; total_cells];
        let n = unsafe {
            bqc_encode(
                payload.as_ptr(),
                payload.len(),
                spec.grid_width,
                spec.grid_height,
                spec.cell_px,
                spec.finder_size,
                spec.calibration_row_start,
                spec.calibration_rows,
                cells.as_mut_ptr(),
                cells.len(),
            )
        };
        assert_eq!(n as usize, total_cells);
        let mut out = vec![0u8; 1024];
        let m = unsafe {
            bqc_decode(
                cells.as_ptr(),
                cells.len(),
                spec.grid_width,
                spec.grid_height,
                spec.cell_px,
                spec.finder_size,
                spec.calibration_row_start,
                spec.calibration_rows,
                out.as_mut_ptr(),
                out.len(),
            )
        };
        assert_eq!(m as usize, payload.len());
        assert_eq!(&out[..m as usize], &payload[..]);
    }

    #[test]
    fn ffi_palette() {
        let mut out = vec![0u8; 24];
        let n = unsafe { bqc_palette_rgb(out.as_mut_ptr(), out.len()) };
        assert_eq!(n, 24);
        // 0 番 = 黒
        assert_eq!(&out[0..3], &[0, 0, 0]);
        // 7 番 = 白
        assert_eq!(&out[21..24], &[255, 255, 255]);
    }
}
