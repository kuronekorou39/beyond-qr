"""合成歪みシミュレータ (Phase 0c)。

カメラ撮影で起きる歪みを Python 側で模倣して、復号アルゴリズムの
堅牢性を計測する。Phase 0c.0 ではノイズと明度のみ、0c.1 で WB、
0c.2 で透視変換を追加する。
"""

from __future__ import annotations

import numpy as np
from PIL import Image

from .geometry import apply_perspective


def add_gaussian_noise(
    image: Image.Image,
    sigma: float,
    rng: np.random.Generator | None = None,
) -> Image.Image:
    """各チャンネル独立にガウスノイズを加える。

    Args:
        sigma: 標準偏差 (0..=255 のスケール)。実カメラの ISO ノイズ相当。
    """
    if rng is None:
        rng = np.random.default_rng()
    arr = np.asarray(image, dtype=np.float32)
    noise = rng.normal(0.0, sigma, arr.shape).astype(np.float32)
    noisy = np.clip(arr + noise, 0.0, 255.0).astype(np.uint8)
    return Image.fromarray(noisy, mode=image.mode)


def shift_brightness(image: Image.Image, factor: float) -> Image.Image:
    """乗算的な明度シフト。1.0 で無変化、<1.0 で暗く、>1.0 で明るく。"""
    arr = np.asarray(image, dtype=np.float32)
    arr = np.clip(arr * factor, 0.0, 255.0).astype(np.uint8)
    return Image.fromarray(arr, mode=image.mode)


def shift_white_balance(
    image: Image.Image,
    r_gain: float,
    g_gain: float,
    b_gain: float,
) -> Image.Image:
    """チャンネル別ゲイン (オートホワイトバランスの誤差を模倣)。"""
    arr = np.asarray(image, dtype=np.float32)
    arr[..., 0] = np.clip(arr[..., 0] * r_gain, 0.0, 255.0)
    arr[..., 1] = np.clip(arr[..., 1] * g_gain, 0.0, 255.0)
    arr[..., 2] = np.clip(arr[..., 2] * b_gain, 0.0, 255.0)
    return Image.fromarray(arr.astype(np.uint8), mode=image.mode)


def random_perspective(
    image: Image.Image,
    max_shift: float,
    rng: np.random.Generator | None = None,
) -> tuple[Image.Image, np.ndarray]:
    """4 隅をランダムに内側へ最大 max_shift px ずらす透視変換を適用する。

    各隅は画像中心方向にだけ動くので、コンテンツがキャンバス外に切れることはない。
    これは「画像がカメラフレーム内に台形として写る」現実的なシナリオに対応する。

    返り値: (歪んだ画像, dst_corners (4,2))。
    dst_corners は元画像の (0,0), (w,0), (w,h), (0,h) がどこに移ったか。
    """
    if rng is None:
        rng = np.random.default_rng()
    w, h = image.size
    # 各隅は内側方向にのみシフト (TL は右下、TR は左下、BR は左上、BL は右上)
    shifts = np.array(
        [
            [rng.uniform(0, max_shift), rng.uniform(0, max_shift)],
            [-rng.uniform(0, max_shift), rng.uniform(0, max_shift)],
            [-rng.uniform(0, max_shift), -rng.uniform(0, max_shift)],
            [rng.uniform(0, max_shift), -rng.uniform(0, max_shift)],
        ],
        dtype=np.float64,
    )
    base = np.array([(0, 0), (w, 0), (w, h), (0, h)], dtype=np.float64)
    dst_corners = base + shifts
    return apply_perspective(image, dst_corners), dst_corners
