//! 実カメラ画像 (グレースケール) から vcode フレームをスキャンする。
//!
//! v0 の前提: UI がガイド枠を表示し、ユーザーがコードを枠内に収める。
//! つまり 4 隅のおおよその位置 (ガイド枠の角) は既知で、スキャナの仕事は
//!   1. 各隅の近傍窓でコーナーマーカーの外角を精密化
//!   2. 4 点からホモグラフィ (セル座標 → 画像座標) を推定
//!   3. セル中心をバイリニアサンプリングし Otsu で二値化
//!   4. 共通デコード経路 (ヘッダ CRC / ブロック CRC / 部分回収) に流す
//! 検出の完全自動化 (ガイドなし) とフレーム間トラッキングは次段。

use crate::{decode_from_sampler, DecodedFrame, FrameError, Layout, CORNER, STRIP_H};

/// 画像座標 (x, y) の 4 点。tl→tr→br→bl の順。
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub tl: (f32, f32),
    pub tr: (f32, f32),
    pub br: (f32, f32),
    pub bl: (f32, f32),
}

/// グレースケール画像
pub struct GrayImage<'a> {
    pub w: usize,
    pub h: usize,
    pub data: &'a [u8],
}

impl<'a> GrayImage<'a> {
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.w + x]
    }

    /// バイリニア補間サンプリング。範囲外は白 (255) 扱い。
    pub fn bilinear(&self, x: f32, y: f32) -> f32 {
        if x < 0.0 || y < 0.0 || x >= (self.w - 1) as f32 || y >= (self.h - 1) as f32 {
            return 255.0;
        }
        let (x0, y0) = (x as usize, y as usize);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p00 = self.get(x0, y0) as f32;
        let p10 = self.get(x0 + 1, y0) as f32;
        let p01 = self.get(x0, y0 + 1) as f32;
        let p11 = self.get(x0 + 1, y0 + 1) as f32;
        p00 * (1.0 - fx) * (1.0 - fy) + p10 * fx * (1.0 - fy) + p01 * (1.0 - fx) * fy + p11 * fx * fy
    }
}

/// 3x3 ホモグラフィ行列 (row-major、h33 = 1 に正規化)
#[derive(Clone, Copy, Debug)]
pub struct Homography(pub [f32; 9]);

impl Homography {
    /// (x, y) を射影変換する
    pub fn map(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.0;
        let d = m[6] * x + m[7] * y + m[8];
        ((m[0] * x + m[1] * y + m[2]) / d, (m[3] * x + m[4] * y + m[5]) / d)
    }

    /// 4 点対応 (src → dst) から DLT でホモグラフィを求める。
    /// 退化配置 (3 点が同一直線上など) では None。
    pub fn from_quad(src: &[(f32, f32); 4], dst: &[(f32, f32); 4]) -> Option<Self> {
        // 8x9 の同次連立方程式を組み、ガウスの消去法で h11..h32 を解く (h33=1)
        let mut a = [[0.0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (src[i].0 as f64, src[i].1 as f64);
            let (u, v) = (dst[i].0 as f64, dst[i].1 as f64);
            a[i * 2] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[i * 2 + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        // 前進消去 (部分ピボット)
        for col in 0..8 {
            let pivot = (col..8).max_by(|&i, &j| {
                a[i][col].abs().partial_cmp(&a[j][col].abs()).unwrap()
            })?;
            if a[pivot][col].abs() < 1e-9 {
                return None;
            }
            a.swap(col, pivot);
            for row in col + 1..8 {
                let f = a[row][col] / a[col][col];
                for k in col..9 {
                    a[row][k] -= f * a[col][k];
                }
            }
        }
        // 後退代入
        let mut hvec = [0.0f64; 8];
        for row in (0..8).rev() {
            let mut sum = a[row][8];
            for k in row + 1..8 {
                sum -= a[row][k] * hvec[k];
            }
            hvec[row] = sum / a[row][row];
        }
        let mut m = [0.0f32; 9];
        for i in 0..8 {
            m[i] = hvec[i] as f32;
        }
        m[8] = 1.0;
        Some(Self(m))
    }

    /// 逆行列 (随伴行列 / 行列式)。特異なら None。
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.0;
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        if det.abs() < 1e-12 {
            return None;
        }
        let adj = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        let mut out = [0.0f32; 9];
        for i in 0..9 {
            out[i] = adj[i] / det;
        }
        Some(Self(out))
    }
}

/// 窓内の Otsu 閾値 (ヒストグラム 256 bin)
fn otsu(values: impl Iterator<Item = u8>) -> u8 {
    let mut hist = [0u32; 256];
    let mut n = 0u32;
    for v in values {
        hist[v as usize] += 1;
        n += 1;
    }
    if n == 0 {
        return 128;
    }
    let total_sum: u64 = hist.iter().enumerate().map(|(i, &c)| i as u64 * c as u64).sum();
    // Otsu の t は「クラス 0 (黒) に含まれる最大値」。二分布間に空白帯があると
    // クラス間分散は帯全体で平坦になるため、argmax 区間 [first, last] の中央を採り、
    // 呼び出し側の「v < thr が黒」に合わせて +1 した排他的閾値を返す。
    let (mut first_t, mut last_t, mut best_var) = (128usize, 128usize, -1.0f64);
    let (mut w0, mut sum0) = (0u64, 0u64);
    for t in 0..256 {
        w0 += hist[t] as u64;
        if w0 == 0 {
            continue;
        }
        let w1 = n as u64 - w0;
        if w1 == 0 {
            break;
        }
        sum0 += t as u64 * hist[t] as u64;
        let m0 = sum0 as f64 / w0 as f64;
        let m1 = (total_sum - sum0) as f64 / w1 as f64;
        let var = w0 as f64 * w1 as f64 * (m0 - m1) * (m0 - m1);
        if var > best_var {
            best_var = var;
            first_t = t;
            last_t = t;
        } else if var == best_var {
            last_t = t;
        }
    }
    (((first_t + last_t) / 2) + 1).min(255) as u8
}

/// ガイド近傍の窓からコーナーマーカーの外角を精密化する。
/// 窓内を Otsu で二値化 → ノイズ除去 (黒 8 近傍 4 個以上) →
/// コーナー方向 dir へ最も突き出た黒画素を外角とみなす。
fn refine_corner(
    img: &GrayImage,
    guess: (f32, f32),
    win: isize,
    dir: (f32, f32),
) -> Option<(f32, f32)> {
    let cx = guess.0 as isize;
    let cy = guess.1 as isize;
    let x0 = (cx - win).max(0) as usize;
    let y0 = (cy - win).max(0) as usize;
    let x1 = ((cx + win) as usize).min(img.w - 1);
    let y1 = ((cy + win) as usize).min(img.h - 1);
    if x1 <= x0 + 2 || y1 <= y0 + 2 {
        return None;
    }

    let thr = otsu((y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)));
    let is_black = |x: usize, y: usize| img.get(x, y) < thr;

    let mut best: Option<((f32, f32), f32)> = None;
    for y in y0 + 1..y1 {
        for x in x0 + 1..x1 {
            if !is_black(x, y) {
                continue;
            }
            // 孤立ノイズ除去: 8 近傍に黒が 4 個未満なら無視
            let neighbors = [
                (x - 1, y - 1), (x, y - 1), (x + 1, y - 1),
                (x - 1, y), (x + 1, y),
                (x - 1, y + 1), (x, y + 1), (x + 1, y + 1),
            ];
            if neighbors.iter().filter(|&&(nx, ny)| is_black(nx, ny)).count() < 4 {
                continue;
            }
            let score = x as f32 * dir.0 + y as f32 * dir.1;
            if best.map_or(true, |(_, s)| score > s) {
                best = Some(((x as f32 + 0.5, y as f32 + 0.5), score));
            }
        }
    }
    best.map(|(p, _)| p)
}

/// スキャン結果 (デコード結果 + 推定ホモグラフィ。トラッキングで次フレームのガイドに使う)
pub struct ScanResult {
    pub frame: DecodedFrame,
    pub homography: Homography,
}

/// 値が既知のセル一覧 (コーナーマーカー 4 個 + 下ストリップの市松)。
/// ホモグラフィ精密化のスコア評価に使う。
fn known_cells(layout: Layout) -> Vec<(usize, usize, bool)> {
    let (w, h) = (layout.width(), layout.height());
    let mut cells = Vec::new();
    for (which, or, oc) in crate::corner_origins(w, h) {
        for r in 0..CORNER {
            for c in 0..CORNER {
                cells.push((or + r, oc + c, crate::corner_black(which, r, c)));
            }
        }
    }
    for r in h - STRIP_H..h {
        for c in CORNER..w - CORNER {
            cells.push((r, c, (r + c) % 2 == 0));
        }
    }
    cells
}

/// 4 隅の画像座標を座標降下で微調整し、既知セルの一致数を最大化する。
/// 粗い外角推定 (±2px 級の誤差) を吸収し、格子中央での半セル級のずれを防ぐ。
fn refine_homography(
    img: &GrayImage,
    corners: &mut [(f32, f32); 4],
    layout: Layout,
    thr: u8,
) -> Option<Homography> {
    let (wc, hc) = (layout.width() as f32, layout.height() as f32);
    let src = [(0.0, 0.0), (wc, 0.0), (wc, hc), (0.0, hc)];
    let cells = known_cells(layout);

    let score = |quad: &[(f32, f32); 4]| -> Option<(Homography, usize)> {
        let hm = Homography::from_quad(&src, quad)?;
        let n = cells
            .iter()
            .filter(|&&(r, c, black)| {
                let (x, y) = hm.map(c as f32 + 0.5, r as f32 + 0.5);
                (img.bilinear(x, y) < thr as f32) == black
            })
            .count();
        Some((hm, n))
    };

    let mut best = score(corners)?;
    // 1px 刻み → 0.5px 刻みの 2 ラウンド。各ラウンドで 4 隅を順に最適化する。
    for &step in &[1.0f32, 0.5] {
        for k in 0..4 {
            let base = corners[k];
            for dy in -3..=3 {
                for dx in -3..=3 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let mut cand = *corners;
                    cand[k] = (base.0 + dx as f32 * step, base.1 + dy as f32 * step);
                    if let Some((hm, n)) = score(&cand) {
                        if n > best.1 {
                            best = (hm, n);
                            corners[k] = cand[k];
                        }
                    }
                }
            }
        }
    }
    Some(best.0)
}

/// ガイド枠 (おおよその 4 隅) を頼りに、グレースケール画像から vcode フレームをスキャンする。
///
/// layout はガイド段階では未知のヘッダ内容に先立ってセル格子を張るためのヒント。
/// ヘッダの実レイアウトと一致しなければ LayoutMismatch を返す。
pub fn scan_frame(
    img: &GrayImage,
    guide: &Quad,
    layout: Layout,
) -> Result<ScanResult, FrameError> {
    let (wc, hc) = (layout.width() as f32, layout.height() as f32);

    // ガイドからセル 1 個のピクセルサイズを見積もり、探索窓を決める
    let guide_w = ((guide.tr.0 - guide.tl.0).powi(2) + (guide.tr.1 - guide.tl.1).powi(2)).sqrt();
    let cell_px = guide_w / wc;
    let win = ((cell_px * CORNER as f32 * 1.5) as isize).max(8);

    // 各隅を精密化 (dir = そのコーナーが突き出ている方向)
    let tl = refine_corner(img, guide.tl, win, (-1.0, -1.0)).ok_or(FrameError::CornerMismatch)?;
    let tr = refine_corner(img, guide.tr, win, (1.0, -1.0)).ok_or(FrameError::CornerMismatch)?;
    let br = refine_corner(img, guide.br, win, (1.0, 1.0)).ok_or(FrameError::CornerMismatch)?;
    let bl = refine_corner(img, guide.bl, win, (-1.0, 1.0)).ok_or(FrameError::CornerMismatch)?;

    // 初期ホモグラフィで粗くサンプリングし、二値化閾値を得る
    let mut corners = [tl, tr, br, bl];
    let hmat0 = Homography::from_quad(
        &[(0.0, 0.0), (wc, 0.0), (wc, hc), (0.0, hc)],
        &corners,
    )
    .ok_or(FrameError::CornerMismatch)?;
    let (w, h) = (layout.width(), layout.height());
    let sample_all = |hm: &Homography| -> Vec<u8> {
        let mut values = vec![0u8; w * h];
        for r in 0..h {
            for c in 0..w {
                let (x, y) = hm.map(c as f32 + 0.5, r as f32 + 0.5);
                values[r * w + c] = img.bilinear(x, y).round().clamp(0.0, 255.0) as u8;
            }
        }
        values
    };
    let thr0 = otsu(sample_all(&hmat0).iter().copied());

    // 既知パターン (コーナー + 市松ストリップ) への一致を最大化するよう 4 隅を微調整
    let hmat = refine_homography(img, &mut corners, layout, thr0)
        .ok_or(FrameError::CornerMismatch)?;

    // 精密化後のホモグラフィで全セルを再サンプリング
    let values = sample_all(&hmat);
    let thr = otsu(values.iter().copied());

    let frame = decode_from_sampler(&|r, c| values[r * w + c] < thr, w, h)?;
    if frame.header.layout != layout {
        return Err(FrameError::LayoutMismatch);
    }
    Ok(ScanResult { frame, homography: hmat })
}
