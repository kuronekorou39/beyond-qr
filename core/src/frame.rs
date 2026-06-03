//! フレームのレイアウト仕様とデータ構造。
//!
//! Phase 0c.2: ファインダーパターンを 3 隅 (TL/TR/BL) に追加。
//! 各セルの役割を `CellRole` 列挙で表現する。

use crate::ecc::{RS_BLOCK_SIZE, RS_DATA_PER_BLOCK};
use crate::palette::{Color, BITS_PER_CELL, PALETTE_SIZE};

/// 4 byte: ペイロード長 (u32 BE) を格納するヘッダ。
pub const LENGTH_HEADER_BYTES: usize = 4;

/// セルの役割。エンコーダ/デコーダはこの値で各セルの処理を分岐する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRole {
    /// データセル。エンコーダはペイロードビットを埋める。
    Data,
    /// キャリブレーションセル。固定色を表示する。
    Calibration(Color),
    /// ファインダーパターンのセル。QR 風の 7×7 構造。
    Finder(Color),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSpec {
    pub grid_width: usize,
    pub grid_height: usize,
    pub cell_px: usize,
    /// ファインダーサイズ (cells)。0 でファインダー無効。
    /// 推奨は 7 (QR の finder pattern と同じ 1:1:3:1:1 比率)。
    pub finder_size: usize,
    /// キャリブレーション行の開始 row index。0c.2 以降は画像中央付近に置く。
    /// 0 にすると最上行から始まる (旧挙動)。ファインダーと重ならないように設定する。
    pub calibration_row_start: usize,
    /// キャリブレーション行数。0 で無効。
    pub calibration_rows: usize,
}

impl FrameSpec {
    /// Phase 0 標準仕様 (8 px/cell, 128×128, finder 7, 中央 calibration 1 行)。
    /// キャリブレーション行は画像中央 (row 64) に置き、ファインダー検出への干渉を避ける。
    pub const PHASE_0: Self = Self {
        grid_width: 128,
        grid_height: 128,
        cell_px: 8,
        finder_size: 7,
        calibration_row_start: 64,
        calibration_rows: 1,
    };

    pub const fn total_cells(&self) -> usize {
        self.grid_width * self.grid_height
    }

    /// 4 つのファインダー領域 + 1 セル幅の quiet zone (白) の合計セル数。
    /// 各隅で (finder_size + 1)² セルを占有する。データに隣接するセルが既知の白に
    /// なるので、検出器がランダムなデータの暗クラスタと混同しにくい (QR の quiet zone と同じ発想)。
    pub const fn finder_cells(&self) -> usize {
        4 * (self.finder_size + 1) * (self.finder_size + 1)
    }

    pub const fn calibration_cells(&self) -> usize {
        self.calibration_rows * self.grid_width
    }

    pub const fn data_cells(&self) -> usize {
        self.total_cells() - self.finder_cells() - self.calibration_cells()
    }

    /// データセルだけを 3 bit/cell でパックしたときの総バイト数。
    pub const fn data_bytes(&self) -> usize {
        (self.data_cells() * BITS_PER_CELL as usize) / 8
    }

    pub const fn rs_blocks(&self) -> usize {
        self.data_bytes() / RS_BLOCK_SIZE
    }

    pub const fn max_payload_bytes(&self) -> usize {
        self.rs_blocks() * RS_DATA_PER_BLOCK - LENGTH_HEADER_BYTES
    }

    pub const fn image_dimensions(&self) -> (usize, usize) {
        (
            self.grid_width * self.cell_px,
            self.grid_height * self.cell_px,
        )
    }

    /// セル (row, col) の役割を返す。各ファインダーは 1 セル幅の quiet zone (白) で
    /// 囲まれる。画像端側 (上/左 for TL, 等) はキャンバス境界そのものが quiet zone の
    /// 役割を果たすので、quiet zone はデータに面する 2 辺だけ拡張する。
    pub fn cell_role(&self, row: usize, col: usize) -> CellRole {
        let fs = self.finder_size;
        if fs > 0 {
            let qz = fs + 1; // finder + 1-cell quiet zone

            // TL 拡張領域 (rows 0..qz, cols 0..qz)
            if row < qz && col < qz {
                if row < fs && col < fs {
                    return CellRole::Finder(finder_pattern_color(row, col, fs));
                }
                return CellRole::Finder(7); // quiet zone = 白
            }
            // TR 拡張領域 (rows 0..qz, cols (W-qz)..W)
            if row < qz && col >= self.grid_width.saturating_sub(qz) {
                if row < fs && col >= self.grid_width.saturating_sub(fs) {
                    let lc = col - (self.grid_width - fs);
                    return CellRole::Finder(finder_pattern_color(row, lc, fs));
                }
                return CellRole::Finder(7);
            }
            // BL 拡張領域 (rows (H-qz)..H, cols 0..qz)
            if row >= self.grid_height.saturating_sub(qz) && col < qz {
                if row >= self.grid_height.saturating_sub(fs) && col < fs {
                    let lr = row - (self.grid_height - fs);
                    return CellRole::Finder(finder_pattern_color(lr, col, fs));
                }
                return CellRole::Finder(7);
            }
            // BR 拡張領域 (rows (H-qz)..H, cols (W-qz)..W)
            if row >= self.grid_height.saturating_sub(qz)
                && col >= self.grid_width.saturating_sub(qz)
            {
                if row >= self.grid_height.saturating_sub(fs)
                    && col >= self.grid_width.saturating_sub(fs)
                {
                    let lr = row - (self.grid_height - fs);
                    let lc = col - (self.grid_width - fs);
                    return CellRole::Finder(finder_pattern_color(lr, lc, fs));
                }
                return CellRole::Finder(7);
            }
        }
        if self.calibration_rows > 0 {
            let start = self.calibration_row_start;
            let end = start + self.calibration_rows;
            if row >= start && row < end {
                return CellRole::Calibration(self.calibration_color_for(col));
            }
        }
        CellRole::Data
    }

    /// キャリブレーションパッチが特定 col で表示すべき色。
    /// 列を 8 等分し palette[0..=7] を順に割り当てる。
    #[inline]
    pub const fn calibration_color_for(&self, col: usize) -> Color {
        let patch = (col * PALETTE_SIZE) / self.grid_width;
        (patch & 0b111) as Color
    }
}

/// QR 風 7×7 ファインダー (任意サイズ s に一般化):
/// 外周ring (黒) + 1 cell 内周 ring (白) + 中心ブロック (黒)。
const fn finder_pattern_color(row: usize, col: usize, size: usize) -> Color {
    if row == 0 || row == size - 1 || col == 0 || col == size - 1 {
        return 0; // outer black ring
    }
    if row == 1 || row == size - 2 || col == 1 || col == size - 2 {
        return 7; // inner white ring
    }
    0 // center black block
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub spec: FrameSpec,
    pub cells: Vec<Color>,
}

impl Frame {
    pub fn new(spec: FrameSpec, cells: Vec<Color>) -> Self {
        debug_assert_eq!(cells.len(), spec.total_cells());
        Self { spec, cells }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Color {
        self.cells[y * self.spec.grid_width + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_0_spec_consistent() {
        let s = FrameSpec::PHASE_0;
        assert_eq!(s.total_cells(), 128 * 128);
        // 4 隅 × (7+1)² = 256 (finder 49 + quiet zone 15 が 4 隅)
        assert_eq!(s.finder_cells(), 4 * 8 * 8);
        assert_eq!(s.calibration_cells(), 128);
        assert_eq!(s.data_cells(), 128 * 128 - 256 - 128);
        assert_eq!(s.image_dimensions(), (1024, 1024));
        assert!(s.max_payload_bytes() >= 500);
    }

    #[test]
    fn quiet_zone_around_finders_is_white() {
        let s = FrameSpec::PHASE_0;
        // TL の quiet zone: 元 finder の外 1 セル幅
        // row 7 cols 0..=7, col 7 rows 0..=7 — 全部白
        for c in 0..=7 {
            assert_eq!(s.cell_role(7, c), CellRole::Finder(7), "TL bottom qz col {c}");
        }
        for r in 0..=7 {
            assert_eq!(s.cell_role(r, 7), CellRole::Finder(7), "TL right qz row {r}");
        }
        // データ領域はその直外 (row 8 col 0..7 や row 0..7 col 8) から始まる
        assert!(matches!(s.cell_role(8, 0), CellRole::Data));
        assert!(matches!(s.cell_role(0, 8), CellRole::Data));
    }

    #[test]
    fn finder_pattern_at_four_corners() {
        let s = FrameSpec::PHASE_0;
        assert!(matches!(s.cell_role(0, 0), CellRole::Finder(_))); // TL
        assert!(matches!(s.cell_role(0, 127), CellRole::Finder(_))); // TR
        assert!(matches!(s.cell_role(127, 0), CellRole::Finder(_))); // BL
        assert!(matches!(s.cell_role(127, 127), CellRole::Finder(_))); // BR
    }

    #[test]
    fn finder_pattern_outer_ring_is_black() {
        let s = FrameSpec::PHASE_0;
        // TL の外周は全て黒 (color 0)
        for c in 0..7 {
            assert_eq!(s.cell_role(0, c), CellRole::Finder(0));
            assert_eq!(s.cell_role(6, c), CellRole::Finder(0));
            assert_eq!(s.cell_role(c, 0), CellRole::Finder(0));
            assert_eq!(s.cell_role(c, 6), CellRole::Finder(0));
        }
    }

    #[test]
    fn finder_pattern_inner_ring_is_white() {
        let s = FrameSpec::PHASE_0;
        // TL の内周 (rows 1, 5 / cols 1, 5) は白 (color 7)
        for c in 1..6 {
            assert_eq!(s.cell_role(1, c), CellRole::Finder(7));
            assert_eq!(s.cell_role(5, c), CellRole::Finder(7));
        }
        for r in 1..6 {
            assert_eq!(s.cell_role(r, 1), CellRole::Finder(7));
            assert_eq!(s.cell_role(r, 5), CellRole::Finder(7));
        }
    }

    #[test]
    fn finder_pattern_center_is_black() {
        let s = FrameSpec::PHASE_0;
        for r in 2..5 {
            for c in 2..5 {
                assert_eq!(s.cell_role(r, c), CellRole::Finder(0));
            }
        }
    }

    #[test]
    fn calibration_row_at_configured_start() {
        let s = FrameSpec::PHASE_0;
        for c in 0..s.grid_width {
            assert!(matches!(
                s.cell_role(s.calibration_row_start, c),
                CellRole::Calibration(_)
            ));
        }
        // 上端と下端は data か finder で、calibration ではない
        assert!(!matches!(s.cell_role(0, 64), CellRole::Calibration(_)));
        assert!(!matches!(s.cell_role(127, 64), CellRole::Calibration(_)));
    }

    #[test]
    fn data_cell_count_matches_layout() {
        let s = FrameSpec::PHASE_0;
        let mut count = 0;
        for row in 0..s.grid_height {
            for col in 0..s.grid_width {
                if matches!(s.cell_role(row, col), CellRole::Data) {
                    count += 1;
                }
            }
        }
        assert_eq!(count, s.data_cells());
    }
}
