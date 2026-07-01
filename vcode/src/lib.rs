//! vcode v0 — 動画ネイティブな独自 2D コードのフレームフォーマット。
//!
//! QR が印刷物向けに払っているコスト (ファインダー/アライメント/全か無かの RS) を捨て、
//! 画面→カメラの動画ストリーミング専用に再設計したもの。
//!
//! 設計の柱:
//!   - フレームは独立した「ブロック」の格子。各ブロックが RaptorQ パケット 1 個 + CRC-16 を持ち、
//!     部分的に壊れたフレームからも読めたブロックだけ回収できる (全か無かをやめる)
//!   - 機能パターンは四隅マーカー + 上下ストリップのみ (面積 ~12%、QR の機能パターン+EC より小さい)
//!   - フレーム内 EC は持たない。フレーム欠落・ブロック欠落は上位の fountain (RaptorQ) が吸収する
//!   - v0 は 1 bit/セル (白黒)。ヘッダに bits_per_cell を持ち、将来の輝度多値化 (2bit) に備える
//!
//! フレーム構造 (セル座標、デフォルトレイアウト 100x92):
//!   - 上ストリップ (行 0..6): 両端に 6x6 コーナーマーカー、間にヘッダ (22 byte + CRC を 3 コピー)
//!   - データ領域 (行 6..86): 20x20 セルのブロックが 5x4 = 20 個。ブロック = 48 byte payload + CRC-16
//!   - 下ストリップ (行 86..92): 両端にコーナーマーカー、間に市松の較正/タイミングパターン
//!
//! この v0 デコーダは「理想チャネル」(スケール整数倍・歪みなしのビットマップ) を仮定する。
//! 実カメラ画像からの検出・射影補正・サンプリングは次段で別モジュールとして実装する。

/// 上下ストリップの高さ (セル)
pub const STRIP_H: usize = 6;
/// コーナーマーカーの一辺 (セル)
pub const CORNER: usize = 6;
/// ヘッダ先頭のマジックバイト
pub const MAGIC: u8 = 0xB9;
/// ヘッダのシリアライズ長 (CRC-16 込み)
pub const HEADER_LEN: usize = 22;
/// フォーマットバージョン
pub const VERSION: u8 = 0;

/// CRC-16/CCITT-FALSE (init=0xFFFF, poly=0x1021)
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// ブロック格子のレイアウト。ヘッダに載るのでデコーダは事前知識なしで復元できる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// ブロックの一辺 (セル)
    pub block: usize,
    /// 横方向のブロック数
    pub grid_w: usize,
    /// 縦方向のブロック数
    pub grid_h: usize,
}

impl Layout {
    /// デフォルトレイアウト: 100x92 セル、20 ブロック、実効 880 byte/フレーム
    pub const V0: Layout = Layout { block: 20, grid_w: 5, grid_h: 4 };

    pub fn width(&self) -> usize {
        self.grid_w * self.block
    }

    pub fn height(&self) -> usize {
        self.grid_h * self.block + 2 * STRIP_H
    }

    pub fn block_count(&self) -> usize {
        self.grid_w * self.grid_h
    }

    /// 1 ブロックのセル数から得られる総バイト数 (1 bit/セル)
    pub fn block_bytes(&self) -> usize {
        self.block * self.block / 8
    }

    /// CRC-16 を除いたブロックペイロード長 (= シリアライズ済み RaptorQ パケットがそのまま入る)
    pub fn block_payload_len(&self) -> usize {
        self.block_bytes() - 2
    }

    /// このレイアウトに合わせる場合の RaptorQ packet_size
    /// (シリアライズ済みパケット = 4 byte payload ID + packet_size)
    pub fn packet_size(&self) -> usize {
        self.block_payload_len() - 4
    }

    /// ブロック bi (行優先) の左上セル座標 (row, col)
    fn block_origin(&self, bi: usize) -> (usize, usize) {
        let by = bi / self.grid_w;
        let bx = bi % self.grid_w;
        (STRIP_H + by * self.block, bx * self.block)
    }
}

/// フレームヘッダ。上ストリップに CRC 付きで複数コピー格納される。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub version: u8,
    pub bits_per_cell: u8,
    pub layout: Layout,
    /// 表示シーケンス番号 (受信統計・重複検出用)
    pub frame_seq: u16,
    /// RaptorQ の OTI (12 byte)。全フレームに載せるので受信はどのフレームからでも開始できる
    pub oti: [u8; 12],
}

impl FrameHeader {
    pub fn serialize(&self) -> [u8; HEADER_LEN] {
        let mut buf = [0u8; HEADER_LEN];
        buf[0] = MAGIC;
        buf[1] = self.version;
        buf[2] = self.bits_per_cell;
        buf[3] = self.layout.block as u8;
        buf[4] = self.layout.grid_w as u8;
        buf[5] = self.layout.grid_h as u8;
        buf[6..8].copy_from_slice(&self.frame_seq.to_le_bytes());
        buf[8..20].copy_from_slice(&self.oti);
        let crc = crc16(&buf[..20]);
        buf[20..22].copy_from_slice(&crc.to_be_bytes());
        buf
    }

    /// magic と CRC が一致しなければ None
    pub fn deserialize(buf: &[u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN || buf[0] != MAGIC {
            return None;
        }
        let crc_stored = u16::from_be_bytes([buf[20], buf[21]]);
        if crc16(&buf[..20]) != crc_stored {
            return None;
        }
        Some(Self {
            version: buf[1],
            bits_per_cell: buf[2],
            layout: Layout {
                block: buf[3] as usize,
                grid_w: buf[4] as usize,
                grid_h: buf[5] as usize,
            },
            frame_seq: u16::from_le_bytes([buf[6], buf[7]]),
            oti: buf[8..20].try_into().unwrap(),
        })
    }
}

/// グレースケール画像 (行優先、0=黒, 255=白)
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Bitmap {
    pub fn new_white(w: usize, h: usize) -> Self {
        Self { w, h, data: vec![255; w * h] }
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.w + x]
    }

    pub fn set(&mut self, x: usize, y: usize, v: u8) {
        self.data[y * self.w + x] = v;
    }

    /// セル (row, col) を scale x scale ピクセルで塗る
    fn fill_cell(&mut self, scale: usize, row: usize, col: usize, black: bool) {
        let v = if black { 0 } else { 255 };
        for dy in 0..scale {
            for dx in 0..scale {
                self.set(col * scale + dx, row * scale + dy, v);
            }
        }
    }

    /// セル (row, col) の中心をサンプリングして黒なら true
    fn sample_cell(&self, scale: usize, row: usize, col: usize) -> bool {
        self.get(col * scale + scale / 2, row * scale + scale / 2) < 128
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// ビットマップ寸法が scale の整数倍でない、またはストリップ幅すら確保できない
    BadDimensions,
    /// 四隅マーカーが期待パターンと一致しない
    CornerMismatch,
    /// どのヘッダコピーも CRC を通らなかった
    HeaderNotFound,
    /// ヘッダのレイアウトとビットマップ寸法が矛盾
    LayoutMismatch,
}

/// デコード結果。blocks[i] は CRC が通ればペイロード、壊れていれば None (部分回収)。
#[derive(Debug)]
pub struct DecodedFrame {
    pub header: FrameHeader,
    pub blocks: Vec<Option<Vec<u8>>>,
}

#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// コーナーマーカーのパターン。外周 1 セルは全コーナー共通で黒 (検出用)、
/// 内部 4x4 はコーナーごとに異なる (将来の回転判定用)。
fn corner_black(which: Corner, r: usize, c: usize) -> bool {
    let border = r == 0 || r == CORNER - 1 || c == 0 || c == CORNER - 1;
    if border {
        return true;
    }
    match which {
        Corner::TopLeft => true,                                     // 塗りつぶし
        Corner::TopRight => false,                                   // 白抜きリング
        Corner::BottomLeft => (2..4).contains(&r) && (2..4).contains(&c), // 中央 2x2 のみ黒
        Corner::BottomRight => (r + c) % 2 == 0,                     // 市松
    }
}

/// 4 コーナーの (種別, 左上セル座標)
fn corner_origins(w: usize, h: usize) -> [(Corner, usize, usize); 4] {
    [
        (Corner::TopLeft, 0, 0),
        (Corner::TopRight, 0, w - CORNER),
        (Corner::BottomLeft, h - CORNER, 0),
        (Corner::BottomRight, h - CORNER, w - CORNER),
    ]
}

/// ヘッダ領域のセルを行優先で列挙 (上ストリップのコーナー間)
fn header_cells(w: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..STRIP_H).flat_map(move |r| (CORNER..w - CORNER).map(move |c| (r, c)))
}

/// バイト列を MSB-first のビット列にする
fn byte_bits(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes
        .iter()
        .flat_map(|&b| (0..8).map(move |j| (b >> (7 - j)) & 1 == 1))
}

/// ビット列 (MSB-first) をバイト列に戻す
fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|ch| ch.iter().fold(0u8, |acc, &b| (acc << 1) | b as u8))
        .collect()
}

/// フレームを 1 枚エンコードする。
/// blocks は各ブロックのペイロード (長さ layout.block_payload_len())。
/// block_count() より少ない場合、余りは全ゼロセル (CRC 不成立 = 受信側で None) で埋める。
pub fn encode_frame(header: &FrameHeader, blocks: &[Vec<u8>], scale: usize) -> Bitmap {
    let layout = header.layout;
    let (w, h) = (layout.width(), layout.height());
    assert!(scale >= 1);
    assert!(blocks.len() <= layout.block_count(), "ブロック数超過");
    assert!(
        w > 2 * CORNER + HEADER_LEN * 8 / STRIP_H,
        "ヘッダ 1 コピーも入らないレイアウト"
    );
    let mut bm = Bitmap::new_white(w * scale, h * scale);

    // コーナーマーカー
    for (which, or, oc) in corner_origins(w, h) {
        for r in 0..CORNER {
            for c in 0..CORNER {
                bm.fill_cell(scale, or + r, oc + c, corner_black(which, r, c));
            }
        }
    }

    // ヘッダ: 領域に入るだけコピーを繰り返す (100 セル幅なら丁度 3 コピー)
    let hdr_bits: Vec<bool> = byte_bits(&header.serialize()).collect();
    let cells: Vec<(usize, usize)> = header_cells(w).collect();
    for (i, &(r, c)) in cells.iter().enumerate() {
        let bit = hdr_bits[i % hdr_bits.len()];
        // 端数領域に中途半端なコピーは書かない (白のまま)
        if i < (cells.len() / hdr_bits.len()) * hdr_bits.len() {
            bm.fill_cell(scale, r, c, bit);
        }
    }

    // 下ストリップ: 市松の較正/タイミングパターン
    for r in h - STRIP_H..h {
        for c in CORNER..w - CORNER {
            bm.fill_cell(scale, r, c, (r + c) % 2 == 0);
        }
    }

    // データブロック: payload + CRC-16 を行優先ビットで敷き詰める
    for bi in 0..layout.block_count() {
        let content: Vec<u8> = if bi < blocks.len() {
            assert_eq!(blocks[bi].len(), layout.block_payload_len(), "ペイロード長不一致");
            let mut v = blocks[bi].clone();
            v.extend_from_slice(&crc16(&blocks[bi]).to_be_bytes());
            v
        } else {
            vec![0u8; layout.block_bytes()]
        };
        let (or, oc) = layout.block_origin(bi);
        for (i, bit) in byte_bits(&content).enumerate() {
            bm.fill_cell(scale, or + i / layout.block, oc + i % layout.block, bit);
        }
    }

    bm
}

/// 理想チャネルのビットマップからフレームをデコードする。
/// 壊れたブロックは None として返し、読めたブロックだけ回収する。
pub fn decode_frame(bm: &Bitmap, scale: usize) -> Result<DecodedFrame, FrameError> {
    if scale == 0 || bm.w % scale != 0 || bm.h % scale != 0 {
        return Err(FrameError::BadDimensions);
    }
    let (w, h) = (bm.w / scale, bm.h / scale);
    if w < 2 * CORNER + 1 || h < 2 * STRIP_H + 1 {
        return Err(FrameError::BadDimensions);
    }

    // 四隅マーカー検証 (理想チャネルなので完全一致を要求)
    for (which, or, oc) in corner_origins(w, h) {
        for r in 0..CORNER {
            for c in 0..CORNER {
                if bm.sample_cell(scale, or + r, oc + c) != corner_black(which, r, c) {
                    return Err(FrameError::CornerMismatch);
                }
            }
        }
    }

    // ヘッダ: 各コピーを順に試し、最初に CRC が通ったものを採用
    let hdr_cell_bits: Vec<bool> = header_cells(w)
        .map(|(r, c)| bm.sample_cell(scale, r, c))
        .collect();
    let copy_bits = HEADER_LEN * 8;
    let header = (0..hdr_cell_bits.len() / copy_bits)
        .find_map(|k| {
            let bytes = bits_to_bytes(&hdr_cell_bits[k * copy_bits..(k + 1) * copy_bits]);
            FrameHeader::deserialize(&bytes)
        })
        .ok_or(FrameError::HeaderNotFound)?;

    let layout = header.layout;
    if layout.width() != w || layout.height() != h || header.bits_per_cell != 1 {
        return Err(FrameError::LayoutMismatch);
    }

    // データブロック: CRC が通ったものだけ回収
    let blocks = (0..layout.block_count())
        .map(|bi| {
            let (or, oc) = layout.block_origin(bi);
            let bits: Vec<bool> = (0..layout.block * layout.block)
                .map(|i| bm.sample_cell(scale, or + i / layout.block, oc + i % layout.block))
                .collect();
            let bytes = bits_to_bytes(&bits);
            let (payload, crc) = bytes.split_at(layout.block_payload_len());
            if crc16(payload) == u16::from_be_bytes([crc[0], crc[1]]) {
                Some(payload.to_vec())
            } else {
                None
            }
        })
        .collect();

    Ok(DecodedFrame { header, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_header(frame_seq: u16) -> FrameHeader {
        FrameHeader {
            version: VERSION,
            bits_per_cell: 1,
            layout: Layout::V0,
            frame_seq,
            oti: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        }
    }

    /// 決定的な擬似ランダムペイロード
    fn test_blocks(n: usize, len: usize, seed: u8) -> Vec<Vec<u8>> {
        (0..n)
            .map(|bi| {
                (0..len)
                    .map(|i| (i as u8).wrapping_mul(31).wrapping_add(bi as u8 ^ seed))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn crc16_known_vector() {
        // CRC-16/CCITT-FALSE の標準テストベクタ
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }

    #[test]
    fn header_roundtrip() {
        let h = test_header(1234);
        let buf = h.serialize();
        assert_eq!(FrameHeader::deserialize(&buf), Some(h));
        // 1 バイト壊すと CRC で棄却される
        let mut bad = buf;
        bad[9] ^= 0x40;
        assert_eq!(FrameHeader::deserialize(&bad), None);
    }

    #[test]
    fn layout_v0_capacity() {
        let l = Layout::V0;
        assert_eq!((l.width(), l.height()), (100, 92));
        assert_eq!(l.block_count(), 20);
        assert_eq!(l.block_bytes(), 50);
        assert_eq!(l.block_payload_len(), 48);
        assert_eq!(l.packet_size(), 44);
        // ヘッダ領域 = 6 * (100-12) = 528 セル → 22byte*8=176bit が丁度 3 コピー
        assert_eq!(header_cells(l.width()).count(), 528);
    }

    #[test]
    fn frame_roundtrip_scale1() {
        let header = test_header(7);
        let blocks = test_blocks(20, Layout::V0.block_payload_len(), 0xA5);
        let bm = encode_frame(&header, &blocks, 1);
        let decoded = decode_frame(&bm, 1).unwrap();
        assert_eq!(decoded.header, header);
        for (i, b) in decoded.blocks.iter().enumerate() {
            assert_eq!(b.as_deref(), Some(blocks[i].as_slice()), "block {i}");
        }
    }

    #[test]
    fn frame_roundtrip_scale3() {
        let header = test_header(8);
        let blocks = test_blocks(20, Layout::V0.block_payload_len(), 0x5A);
        let bm = encode_frame(&header, &blocks, 3);
        assert_eq!((bm.w, bm.h), (300, 276));
        let decoded = decode_frame(&bm, 3).unwrap();
        assert_eq!(decoded.header, header);
        for (i, b) in decoded.blocks.iter().enumerate() {
            assert_eq!(b.as_deref(), Some(blocks[i].as_slice()), "block {i}");
        }
    }

    #[test]
    fn filler_blocks_decode_as_none() {
        let header = test_header(9);
        // 20 ブロック中 5 個だけ実データ
        let blocks = test_blocks(5, Layout::V0.block_payload_len(), 0x11);
        let bm = encode_frame(&header, &blocks, 1);
        let decoded = decode_frame(&bm, 1).unwrap();
        for i in 0..5 {
            assert!(decoded.blocks[i].is_some());
        }
        for i in 5..20 {
            assert_eq!(decoded.blocks[i], None, "filler block {i} が誤って回収された");
        }
    }

    #[test]
    fn partial_corruption_recovers_intact_blocks() {
        let header = test_header(10);
        let blocks = test_blocks(20, Layout::V0.block_payload_len(), 0x77);
        let mut bm = encode_frame(&header, &blocks, 1);

        // ブロック格子 (bx,by) = (1..3, 1..3) の 4 ブロックを覆う黒塗り
        // = セル座標 行 26..66, 列 20..60 (データ領域は行 6 開始)
        for y in 26..66 {
            for x in 20..60 {
                bm.set(x, y, 0);
            }
        }
        let decoded = decode_frame(&bm, 1).unwrap();
        assert_eq!(decoded.header, header, "ヘッダはデータ領域の破損の影響を受けない");

        let expect_dead = [6usize, 7, 11, 12]; // by*5+bx for (1,1),(2,1),(1,2),(2,2)
        for i in 0..20 {
            if expect_dead.contains(&i) {
                assert_eq!(decoded.blocks[i], None, "block {i} は破損しているはず");
            } else {
                assert_eq!(
                    decoded.blocks[i].as_deref(),
                    Some(blocks[i].as_slice()),
                    "block {i} は無傷で回収できるはず (部分回収)"
                );
            }
        }
    }

    #[test]
    fn corner_corruption_is_detected() {
        let header = test_header(11);
        let blocks = test_blocks(20, Layout::V0.block_payload_len(), 0x33);
        let mut bm = encode_frame(&header, &blocks, 1);
        bm.set(0, 0, 255); // TL マーカーの角を白に
        assert_eq!(decode_frame(&bm, 1).unwrap_err(), FrameError::CornerMismatch);
    }
}
