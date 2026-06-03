//! sRGB ↔ OKLab 変換 (Björn Ottosson, 2020)。
//!
//! Phase 0a では未使用だが、Phase 0c 以降のキャリブレーションと
//! 知覚的色距離計算で使用する。

use crate::palette::Rgb;

/// OKLab 色 (L: 明度, a, b: 色度)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// sRGB ガンマ符号化値 (0..255) を線形 sRGB (0.0..1.0) に変換する。
fn srgb_to_linear(c: u8) -> f32 {
    let x = c as f32 / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// 線形 sRGB (0.0..1.0) を sRGB ガンマ符号化値 (0..255) に変換する。
fn linear_to_srgb(x: f32) -> u8 {
    let y = if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (y.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// sRGB を OKLab に変換する。
pub fn srgb_to_oklab(rgb: Rgb) -> Oklab {
    let r = srgb_to_linear(rgb.r);
    let g = srgb_to_linear(rgb.g);
    let b = srgb_to_linear(rgb.b);

    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    Oklab {
        l: 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        a: 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        b: 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    }
}

/// OKLab を sRGB に変換する。範囲外は飽和する。
pub fn oklab_to_srgb(lab: Oklab) -> Rgb {
    let l_ = lab.l + 0.396_337_78 * lab.a + 0.215_803_76 * lab.b;
    let m_ = lab.l - 0.105_561_346 * lab.a - 0.063_854_17 * lab.b;
    let s_ = lab.l - 0.089_484_18 * lab.a - 1.291_485_5 * lab.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;

    Rgb {
        r: linear_to_srgb(r),
        g: linear_to_srgb(g),
        b: linear_to_srgb(b),
    }
}

/// 2 色間の OKLab ユークリッド距離 (知覚的近さ)。
pub fn oklab_distance(a: Oklab, b: Oklab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    (dl * dl + da * da + db * db).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::PALETTE;

    #[test]
    fn black_and_white_have_expected_l() {
        let black = srgb_to_oklab(Rgb::new(0, 0, 0));
        let white = srgb_to_oklab(Rgb::new(255, 255, 255));
        assert!(black.l.abs() < 0.001);
        assert!((white.l - 1.0).abs() < 0.001);
    }

    #[test]
    fn srgb_oklab_roundtrip_is_close() {
        for rgb in PALETTE.iter() {
            let lab = srgb_to_oklab(*rgb);
            let back = oklab_to_srgb(lab);
            let dr = (rgb.r as i32 - back.r as i32).abs();
            let dg = (rgb.g as i32 - back.g as i32).abs();
            let db = (rgb.b as i32 - back.b as i32).abs();
            assert!(
                dr <= 1 && dg <= 1 && db <= 1,
                "roundtrip drift {dr},{dg},{db} for {:?}", rgb
            );
        }
    }

    #[test]
    fn distance_zero_for_identical_colors() {
        let lab = srgb_to_oklab(Rgb::new(128, 64, 200));
        assert_eq!(oklab_distance(lab, lab), 0.0);
    }
}
