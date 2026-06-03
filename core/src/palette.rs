//! 8 色 sRGB パレット。sRGB キューブの 8 頂点を採用する。
//!
//! 各色はインデックス 0..=7 (3 bit) で表現され、ビットパターンは (R, G, B)。
//! 例: 0b101 = Red + Blue = Magenta。
//!
//! Phase 4 で OKLab 等距離最適パレットに置換を検討する。

/// 3 bit のパレットインデックス (0..=7)。
pub type Color = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

pub const PALETTE_SIZE: usize = 8;
pub const BITS_PER_CELL: u32 = 3;

/// sRGB キューブ 8 頂点。インデックスのビットパターンは (b2=R, b1=G, b0=B)。
pub const PALETTE: [Rgb; PALETTE_SIZE] = [
    Rgb::new(0, 0, 0),       // 0b000 Black
    Rgb::new(0, 0, 255),     // 0b001 Blue
    Rgb::new(0, 255, 0),     // 0b010 Green
    Rgb::new(0, 255, 255),   // 0b011 Cyan
    Rgb::new(255, 0, 0),     // 0b100 Red
    Rgb::new(255, 0, 255),   // 0b101 Magenta
    Rgb::new(255, 255, 0),   // 0b110 Yellow
    Rgb::new(255, 255, 255), // 0b111 White
];

/// パレットインデックスから RGB を取り出す。
#[inline]
pub fn color_to_rgb(c: Color) -> Rgb {
    PALETTE[(c & 0b111) as usize]
}

/// 観測 sRGB を最近傍のパレット色に量子化する。
///
/// Phase 0a プレースホルダ。Phase 0c で OKLab 距離に置き換える。
pub fn rgb_to_color(rgb: Rgb) -> Color {
    let mut best: Color = 0;
    let mut best_dist: i32 = i32::MAX;
    for (i, p) in PALETTE.iter().enumerate() {
        let dr = rgb.r as i32 - p.r as i32;
        let dg = rgb.g as i32 - p.g as i32;
        let db = rgb.b as i32 - p.b as i32;
        let d = dr * dr + dg * dg + db * db;
        if d < best_dist {
            best_dist = d;
            best = i as Color;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_eight_entries() {
        assert_eq!(PALETTE.len(), PALETTE_SIZE);
    }

    #[test]
    fn color_roundtrip_through_rgb() {
        for c in 0u8..PALETTE_SIZE as u8 {
            let rgb = color_to_rgb(c);
            assert_eq!(rgb_to_color(rgb), c, "color {c} roundtrip failed");
        }
    }

    #[test]
    fn bit_pattern_matches_rgb_channels() {
        for c in 0u8..PALETTE_SIZE as u8 {
            let rgb = color_to_rgb(c);
            assert_eq!(rgb.r, if c & 0b100 != 0 { 255 } else { 0 });
            assert_eq!(rgb.g, if c & 0b010 != 0 { 255 } else { 0 });
            assert_eq!(rgb.b, if c & 0b001 != 0 { 255 } else { 0 });
        }
    }
}
