//! scan モジュールの合成チャネルテスト。
//! レンダリングしたフレームに透視変形・輝度勾配・ノイズを加えた「擬似カメラ画像」を作り、
//! ずれたガイド枠からのスキャンでヘッダ+ブロックが回収できることを検証する。

use beyond_qr_vcode::scan::{scan_frame, GrayImage, Homography, Quad};
use beyond_qr_vcode::{encode_frame, FrameHeader, Layout, VERSION};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

fn test_frame(layout: Layout, seed: u8) -> (FrameHeader, Vec<Vec<u8>>) {
    let header = FrameHeader {
        version: VERSION,
        bits_per_cell: 1,
        layout,
        frame_seq: 42,
        oti: [9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2],
    };
    let blocks = (0..layout.block_count())
        .map(|bi| {
            (0..layout.block_payload_len())
                .map(|i| (i as u8).wrapping_mul(17).wrapping_add(bi as u8 ^ seed))
                .collect()
        })
        .collect();
    (header, blocks)
}

/// レンダリング済みフレームを、指定した 4 隅へ射影変換して canvas に描き込む。
/// 輝度勾配 (左 75% → 右 95%) とノイズ (±8) も加える。
fn synth_camera_image(
    frame_px: &beyond_qr_vcode::Bitmap,
    canvas_w: usize,
    canvas_h: usize,
    dst: &[(f32, f32); 4],
    noise_seed: u64,
) -> Vec<u8> {
    let src_quad = [
        (0.0, 0.0),
        (frame_px.w as f32, 0.0),
        (frame_px.w as f32, frame_px.h as f32),
        (0.0, frame_px.h as f32),
    ];
    // canvas → フレーム画素座標 の逆写像で各画素を埋める
    let h_fwd = Homography::from_quad(&src_quad, dst).unwrap();
    let h_inv = h_fwd.inverse().unwrap();
    let src = GrayImage { w: frame_px.w, h: frame_px.h, data: &frame_px.data };

    let mut rng = Lcg(noise_seed);
    let mut out = vec![250u8; canvas_w * canvas_h];
    for y in 0..canvas_h {
        for x in 0..canvas_w {
            let (sx, sy) = h_inv.map(x as f32, y as f32);
            let v = if sx < -1.0 || sy < -1.0 || sx > frame_px.w as f32 || sy > frame_px.h as f32 {
                250.0 // コード外は明るい背景
            } else {
                src.bilinear(sx, sy)
            };
            let gain = 0.75 + 0.20 * (x as f32 / canvas_w as f32);
            let noise = (rng.next() % 17) as f32 - 8.0;
            out[y * canvas_w + x] = (v * gain + noise).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[test]
fn scan_recovers_from_perspective_and_noise() {
    let layout = Layout::V0;
    let (header, blocks) = test_frame(layout, 0xC3);
    let frame_px = encode_frame(&header, &blocks, 8); // 800x736

    // 少し傾いた台形に射影 (手持ちカメラの構図を模擬)
    let dst = [
        (180.0, 130.0),  // tl
        (1010.0, 155.0), // tr
        (985.0, 950.0),  // br
        (205.0, 920.0),  // bl
    ];
    let canvas = synth_camera_image(&frame_px, 1280, 1080, &dst, 0xFACE);
    let img = GrayImage { w: 1280, h: 1080, data: &canvas };

    // ガイドは真の 4 隅から最大 12px ずらす (ユーザーの構図ずれを模擬)
    let guide = Quad {
        tl: (dst[0].0 - 10.0, dst[0].1 + 8.0),
        tr: (dst[1].0 + 12.0, dst[1].1 - 6.0),
        br: (dst[2].0 + 9.0, dst[2].1 + 11.0),
        bl: (dst[3].0 - 7.0, dst[3].1 - 12.0),
    };

    let result = scan_frame(&img, &guide, layout).expect("スキャン失敗");
    assert_eq!(result.frame.header, header);

    let ok = result.frame.blocks.iter().filter(|b| b.is_some()).count();
    assert!(ok >= 19, "回収ブロックが少なすぎる: {ok}/20");
    for (i, b) in result.frame.blocks.iter().enumerate() {
        if let Some(payload) = b {
            assert_eq!(payload, &blocks[i], "block {i} の内容不一致");
        }
    }
}

#[test]
fn scan_fails_gracefully_on_blank_image() {
    let blank = vec![250u8; 640 * 480];
    let img = GrayImage { w: 640, h: 480, data: &blank };
    let guide = Quad {
        tl: (100.0, 100.0),
        tr: (500.0, 100.0),
        br: (500.0, 460.0),
        bl: (100.0, 460.0),
    };
    assert!(scan_frame(&img, &guide, Layout::V0).is_err());
}
