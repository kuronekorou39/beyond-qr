"""Phase 0c.2 Step 2b: ファインダー自動検出 + 透視 unwarp + 復号テスト。

歪み適用後、distortion パラメータは復号側に渡さず、画像から自動で
ファインダーの中心を検出して unwarp + 復号する。
"""

from __future__ import annotations

import numpy as np

from beyond_qr_sender.distort import random_perspective
from beyond_qr_sender.geometry import find_finder_centers
from beyond_qr_sender.render import (
    PHASE_0_8PX,
    cells_to_image,
    decode,
    encode,
    image_to_cells_perspective,
)

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))


def _trial_blind(max_shift: float, seed: int) -> bool:
    spec = PHASE_0_8PX
    rng = np.random.default_rng(seed)

    cells = encode(PAYLOAD, spec)
    image = cells_to_image(cells, spec)
    w, h = image.size

    distorted, _ = random_perspective(image, max_shift, rng)

    # 検出: 歪み画像から 4 隅 (TL/TR/BL/BR) のファインダー中心 px を見つける
    arr = np.asarray(distorted, dtype=np.float32)
    observed = find_finder_centers(arr, spec.finder_size, spec.cell_px)

    # expected: 元画像でのファインダー中心
    fp_half = spec.finder_size * spec.cell_px / 2
    expected = np.array(
        [
            (fp_half, fp_half),
            (w - fp_half, fp_half),
            (fp_half, h - fp_half),
            (w - fp_half, h - fp_half),
        ],
        dtype=np.float64,
    )

    cells_back = image_to_cells_perspective(
        distorted,
        observed_corners=observed,
        expected_corners=expected,
        spec=spec,
    )
    try:
        return decode(cells_back, spec) == PAYLOAD
    except ValueError:
        return False


def test_blind_small_shifts():
    """±10 px 内向き透視 (mild) で全件復元できる。"""
    n = sum(_trial_blind(10.0, seed=i) for i in range(10))
    assert n == 10, f"±10 px (blind) で {n}/10"


def test_blind_medium_shifts():
    """±25 px 内向き透視 (moderate) で大半復元できる。"""
    n = sum(_trial_blind(25.0, seed=i) for i in range(10))
    assert n >= 9, f"±25 px (blind) で {n}/10"


def test_blind_large_shifts():
    """±50 px 内向き透視 (large) で過半数復元できる。"""
    n = sum(_trial_blind(50.0, seed=i) for i in range(10))
    assert n >= 7, f"±50 px (blind) で {n}/10"
