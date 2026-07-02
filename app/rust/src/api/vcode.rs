//! vcode (動画ネイティブ独自 2D コード) の Flutter 向け FFI ラッパ。
//!
//! 送信: VcodeTx が payload を RaptorQ で符号化し、フレーム画像 (グレースケール, scale=1)
//!       を生成する。Flutter 側は nearest-neighbor 拡大で表示するだけ。
//! 受信: カメラの Y プレーンを vcode_scan_gray に渡す。ストライド除去・回転・ガイド枠の
//!       計算は Rust 側で行い、回収できたパケット列を返す (Fountain 投入は Dart 側)。

use beyond_qr_fountain as fountain;
use beyond_qr_vcode as vcode;
use beyond_qr_vcode::scan::{scan_frame, scan_frame_tracked, GrayImage, Quad};

/// 送信側ハンドル。payload を vcode フレーム列に変換する。
pub struct VcodeTx {
    encoder: fountain::Encoder,
    layout: vcode::Layout,
}

/// フレーム画像 (グレースケール、1 セル = 1 ピクセル)
pub struct VcodeFrameImage {
    pub width: u32,
    pub height: u32,
    /// 0=黒, 255=白 の行優先グレースケール
    pub pixels: Vec<u8>,
}

impl VcodeTx {
    /// payload を vcode 用に符号化する。extra_repair はリペアパケット追加数。
    /// packet_size はレイアウトから決まる (V0 = 44 バイト)。
    #[flutter_rust_bridge::frb(sync)]
    pub fn new(payload: Vec<u8>, extra_repair: u32) -> VcodeTx {
        let layout = vcode::Layout::V0;
        VcodeTx {
            encoder: fountain::Encoder::new(&payload, layout.packet_size() as u16, extra_repair),
            layout,
        }
    }

    #[flutter_rust_bridge::frb(sync)]
    pub fn packet_count(&self) -> u32 {
        self.encoder.packet_count() as u32
    }

    /// ループ 1 周のフレーム数 (= ceil(packet_count / ブロック数))
    #[flutter_rust_bridge::frb(sync)]
    pub fn frame_count(&self) -> u32 {
        let bc = self.layout.block_count();
        (self.encoder.packet_count().div_ceil(bc)) as u32
    }

    /// i 番目のフレーム画像を生成する。パケットは循環割り当てなので全フレームが満杯になる。
    #[flutter_rust_bridge::frb(sync)]
    pub fn frame_gray(&self, i: u32) -> VcodeFrameImage {
        let bc = self.layout.block_count();
        let pc = self.encoder.packet_count();
        let header = vcode::FrameHeader {
            version: vcode::VERSION,
            bits_per_cell: 1,
            layout: self.layout,
            frame_seq: (i % 0x10000) as u16,
            oti: {
                let mut oti = [0u8; 12];
                oti.copy_from_slice(&self.encoder.oti_bytes());
                oti
            },
        };
        let blocks: Vec<Vec<u8>> = (0..bc)
            .map(|j| self.encoder.packet((i as usize * bc + j) % pc))
            .collect();
        let bm = vcode::encode_frame(&header, &blocks, 1);
        VcodeFrameImage {
            width: bm.w as u32,
            height: bm.h as u32,
            pixels: bm.data,
        }
    }
}

/// スキャン結果。detected=false のとき error に理由 (デバッグログ用)。
pub struct VcodeScanReport {
    pub detected: bool,
    /// トラッキング (前フレームからの追従) で検出したか (効果検証用)
    pub tracked: bool,
    pub frame_seq: u32,
    pub oti: Vec<u8>,
    /// CRC が通ったブロックのペイロード (= シリアライズ済み RaptorQ パケット) 列
    pub packets: Vec<Vec<u8>>,
    pub blocks_ok: u32,
    pub blocks_total: u32,
    pub error: Option<String>,
    /// debug_dump=true のとき、回転処理後のグレースケール画像 (PC 側解析用)
    pub debug_gray: Option<Vec<u8>>,
    pub debug_w: u32,
    pub debug_h: u32,
}

fn fail(reason: &str) -> VcodeScanReport {
    VcodeScanReport {
        detected: false,
        tracked: false,
        frame_seq: 0,
        oti: vec![],
        packets: vec![],
        blocks_ok: 0,
        blocks_total: 0,
        error: Some(reason.to_string()),
        debug_gray: None,
        debug_w: 0,
        debug_h: 0,
    }
}

fn success(result: beyond_qr_vcode::scan::ScanResult, tracked: bool, layout: vcode::Layout) -> VcodeScanReport {
    let frame = result.frame;
    let packets: Vec<Vec<u8>> = frame.blocks.into_iter().flatten().collect();
    VcodeScanReport {
        detected: true,
        tracked,
        frame_seq: frame.header.frame_seq as u32,
        oti: frame.header.oti.to_vec(),
        blocks_ok: packets.len() as u32,
        blocks_total: layout.block_count() as u32,
        packets,
        error: None,
        debug_gray: None,
        debug_w: 0,
        debug_h: 0,
    }
}

/// ストライド除去 + 回転 (rot: 0/90/180/270)。戻りは (回転後画像, 幅, 高さ)。
fn rotate_y_plane(y: &[u8], w: usize, h: usize, stride: usize, rot: u32) -> (Vec<u8>, usize, usize) {
    let (rw, rh) = match rot {
        90 | 270 => (h, w),
        _ => (w, h),
    };
    let mut gray = vec![0u8; rw * rh];
    for sy in 0..h {
        let row = &y[sy * stride..sy * stride + w];
        for sx in 0..w {
            let (dx, dy) = match rot {
                90 => (rw - 1 - sy, sx),
                180 => (rw - 1 - sx, rh - 1 - sy),
                270 => (sy, rh - 1 - sx),
                _ => (sx, sy),
            };
            gray[dy * rw + dx] = row[sx];
        }
    }
    (gray, rw, rh)
}

/// 受信側ハンドル。前フレームで成功した 4 隅と回転を保持し、
/// 次フレームは全数粗探索をスキップして追従 (トラッキング) する。
///
/// scan() の引数:
/// - stride: Y プレーンの行バイト数 (>= width)
/// - rotation_deg: 画像を起こす回転 (0/90/180/270)。Android は通常 sensorOrientation を渡す
/// - guide_frac: 回転後画像の幅に対するガイド枠幅の比 (UI のガイド枠と同じ値を渡す)
#[flutter_rust_bridge::frb(opaque)]
pub struct VcodeRx {
    /// 直近成功時の (回転 deg, 精密化後の 4 隅)
    last: Option<(u32, [(f32, f32); 4])>,
}

impl VcodeRx {
    #[flutter_rust_bridge::frb(sync)]
    pub fn new() -> VcodeRx {
        VcodeRx { last: None }
    }

    /// カメラの Y プレーンから vcode をスキャンする。
    /// トラッキング成功時は report.tracked = true。
    /// 注: sync にしない。非同期 (Rust ワーカースレッド実行) にすることで
    /// UI isolate をブロックせず、カメラプレビューのカクつきを防ぐ。
    pub fn scan(
        &mut self,
        y: Vec<u8>,
        width: u32,
        height: u32,
        stride: u32,
        rotation_deg: u32,
        guide_frac: f64,
        debug_dump: bool,
    ) -> VcodeScanReport {
        let (w, h, stride) = (width as usize, height as usize, stride as usize);
        if stride < w || y.len() < stride * h {
            return fail("Y プレーン寸法不正");
        }
        let layout = vcode::Layout::V0;

        // トラッキング: 前回成功した回転・4 隅から追従を試す
        if let Some((rot, corners)) = self.last {
            let (gray, rw, rh) = rotate_y_plane(&y, w, h, stride, rot);
            let img = GrayImage { w: rw, h: rh, data: &gray };
            if let Ok(result) = scan_frame_tracked(&img, &corners, layout) {
                self.last = Some((rot, result.corners));
                return success(result, true, layout);
            }
            // 追従失敗 → フル探索へフォールバック (ロック解除はフル探索も失敗した時)
        }

        // フル探索: 与えられた回転で失敗したら 180 度違いも試す (回転方向の系統誤差対策)
        let mut errors = Vec::new();
        for rot in [rotation_deg % 360, (rotation_deg + 180) % 360] {
            let (gray, rw, rh) = rotate_y_plane(&y, w, h, stride, rot);

            // ガイド枠: 中央配置、幅 = guide_frac * 画像幅、アスペクトはレイアウト準拠
            let gw = (guide_frac.clamp(0.2, 1.0) * rw as f64) as f32;
            let gh = (gw * layout.height() as f32 / layout.width() as f32).min(rh as f32 * 0.95);
            let cx = rw as f32 / 2.0;
            let cy = rh as f32 / 2.0;
            let guide = Quad {
                tl: (cx - gw / 2.0, cy - gh / 2.0),
                tr: (cx + gw / 2.0, cy - gh / 2.0),
                br: (cx + gw / 2.0, cy + gh / 2.0),
                bl: (cx - gw / 2.0, cy + gh / 2.0),
            };

            let img = GrayImage { w: rw, h: rh, data: &gray };
            match scan_frame(&img, &guide, layout) {
                Err(e) => errors.push(format!("rot{rot}:{e:?}")),
                Ok(result) => {
                    self.last = Some((rot, result.corners));
                    return success(result, false, layout);
                }
            }
            if debug_dump {
                // 最初の回転の処理済み画像を添付して返す (PC 側 debug_scan での解析用)
                let mut report = fail(&errors.join(" / "));
                report.debug_gray = Some(gray);
                report.debug_w = rw as u32;
                report.debug_h = rh as u32;
                self.last = None;
                return report;
            }
        }
        self.last = None;
        fail(&errors.join(" / "))
    }
}
