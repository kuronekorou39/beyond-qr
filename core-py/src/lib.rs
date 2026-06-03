//! beyond-qr-core の Python バインディング。
//!
//! 公開関数:
//! - `encode(payload: bytes) -> bytes`: ペイロード → セル列 (各セルは 0..=7 の 1 byte)
//! - `decode(cells: bytes) -> bytes`: セル列 → ペイロード
//! - `palette_rgb() -> list[tuple[int,int,int]]`: 8 色 sRGB 値
//! - `rgb_to_color(r, g, b) -> int`: 観測 RGB を最近傍パレット色に量子化
//! - `frame_spec() -> tuple[int,int,int,int]`: (grid_w, grid_h, cell_px, max_payload_bytes)

use ::beyond_qr_core as bqc;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

fn build_spec(
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

#[pyfunction]
#[pyo3(signature = (payload, grid_width=128, grid_height=128, cell_px=8, finder_size=7, calibration_row_start=64, calibration_rows=1))]
fn encode<'py>(
    py: Python<'py>,
    payload: Vec<u8>,
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
) -> PyResult<Bound<'py, PyBytes>> {
    let spec = build_spec(grid_width, grid_height, cell_px, finder_size, calibration_row_start, calibration_rows);
    let frame = bqc::encode_payload(&payload, spec)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok(PyBytes::new_bound(py, &frame.cells))
}

#[pyfunction]
#[pyo3(signature = (cells, grid_width=128, grid_height=128, cell_px=8, finder_size=7, calibration_row_start=64, calibration_rows=1))]
fn decode<'py>(
    py: Python<'py>,
    cells: Vec<u8>,
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
) -> PyResult<Bound<'py, PyBytes>> {
    let spec = build_spec(grid_width, grid_height, cell_px, finder_size, calibration_row_start, calibration_rows);
    if cells.len() != spec.total_cells() {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "expected {} cells, got {}",
            spec.total_cells(),
            cells.len()
        )));
    }
    let frame = bqc::Frame::new(spec, cells);
    let data = bqc::decode_payload(&frame)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok(PyBytes::new_bound(py, &data))
}

#[pyfunction]
fn palette_rgb() -> Vec<(u8, u8, u8)> {
    bqc::palette::PALETTE
        .iter()
        .map(|p| (p.r, p.g, p.b))
        .collect()
}

#[pyfunction]
fn rgb_to_color(r: u8, g: u8, b: u8) -> u8 {
    bqc::palette::rgb_to_color(bqc::palette::Rgb::new(r, g, b))
}

#[pyfunction]
#[pyo3(signature = (grid_width=128, grid_height=128, cell_px=8, finder_size=7, calibration_row_start=64, calibration_rows=1))]
fn frame_spec(
    grid_width: usize,
    grid_height: usize,
    cell_px: usize,
    finder_size: usize,
    calibration_row_start: usize,
    calibration_rows: usize,
) -> (usize, usize, usize, usize, usize, usize, usize, usize) {
    let s = build_spec(grid_width, grid_height, cell_px, finder_size, calibration_row_start, calibration_rows);
    (
        s.grid_width,
        s.grid_height,
        s.cell_px,
        s.finder_size,
        s.calibration_row_start,
        s.calibration_rows,
        s.max_payload_bytes(),
        s.rs_blocks(),
    )
}

#[pymodule]
fn beyond_qr_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(encode, m)?)?;
    m.add_function(wrap_pyfunction!(decode, m)?)?;
    m.add_function(wrap_pyfunction!(palette_rgb, m)?)?;
    m.add_function(wrap_pyfunction!(rgb_to_color, m)?)?;
    m.add_function(wrap_pyfunction!(frame_spec, m)?)?;
    Ok(())
}
