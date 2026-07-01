//! QR 生成 (モジュール行列を返すだけ)。描画は Dart 側 (CustomPainter)。
//! web 版 (qrcode-generator) と同じ思想で、版と EC を明示制御する。

use qrcode::types::Color;
use qrcode::{EcLevel, QrCode, Version};

/// QR のモジュール行列。row-major で size*size 個、1=暗 (黒), 0=明 (白)。
pub struct QrMatrix {
    pub size: u32,
    pub modules: Vec<u8>,
}

fn ec_level(ec: &str) -> EcLevel {
    match ec {
        "L" => EcLevel::L,
        "Q" => EcLevel::Q,
        "H" => EcLevel::H,
        _ => EcLevel::M, // 既定 M
    }
}

/// data を QR 化してモジュール行列を返す。
/// min_version=0 なら容量に収まる最小版を自動選択、1..=40 ならその版に固定。
#[flutter_rust_bridge::frb(sync)]
pub fn make_qr(data: Vec<u8>, ec: String, min_version: u8) -> Result<QrMatrix, String> {
    let level = ec_level(&ec);
    let code = if min_version == 0 {
        QrCode::with_error_correction_level(&data, level)
    } else {
        QrCode::with_version(&data, Version::Normal(min_version as i16), level)
    }
    .map_err(|e| e.to_string())?;

    let size = code.width() as u32;
    let modules = code
        .into_colors()
        .into_iter()
        .map(|c| if c == Color::Dark { 1u8 } else { 0u8 })
        .collect();
    Ok(QrMatrix { size, modules })
}
