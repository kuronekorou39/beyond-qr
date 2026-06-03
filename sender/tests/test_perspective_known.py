"""Phase 0c.2 Step 2a: 既知パラメータ下での透視 unwarp + 復号テスト。

ファインダー検出は使わず、適用した perspective の dst_corners をそのまま渡して
unwarp する。アルゴリズム (unwarp + 既存のキャリブレーション/OKLab/RS) が
正しく繋がっていることを検証する。Step 2b で blind 検出に置き換える。
"""

from __future__ import annotations

import numpy as np

from beyond_qr_sender.distort import random_perspective
from beyond_qr_sender.render import (
    PHASE_0_8PX,
    cells_to_image,
    decode,
    encode,
    image_to_cells_perspective,
)

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))


def _trial_with_known_unwarp(max_shift: float, seed: int) -> bool:
    spec = PHASE_0_8PX
    rng = np.random.default_rng(seed)

    cells = encode(PAYLOAD, spec)
    image = cells_to_image(cells, spec)
    w, h = image.size

    distorted, dst_corners = random_perspective(image, max_shift, rng)

    # 直接サンプリング: 既知 dst_corners = observed、元の 4 隅 = expected
    expected_corners = np.array([(0, 0), (w, 0), (w, h), (0, h)], dtype=np.float64)
    cells_back = image_to_cells_perspective(
        distorted,
        observed_corners=dst_corners,
        expected_corners=expected_corners,
        spec=spec,
    )
    try:
        return decode(cells_back, spec) == PAYLOAD
    except ValueError:
        return False


def test_perspective_small_shifts_known_unwarp():
    """±10 px 程度の小さな透視歪みでは 10/10 復元できる。"""
    n = sum(_trial_with_known_unwarp(10.0, seed=i) for i in range(10))
    assert n == 10, f"±10 px 透視で {n}/10"


def test_perspective_medium_shifts_known_unwarp():
    """±25 px 程度の中等度透視歪みも復元できる。"""
    n = sum(_trial_with_known_unwarp(25.0, seed=i) for i in range(10))
    assert n >= 9, f"±25 px 透視で {n}/10"


def test_perspective_large_shifts_known_unwarp():
    """±50 px (画像 5%) でも復元できる。"""
    n = sum(_trial_with_known_unwarp(50.0, seed=i) for i in range(10))
    assert n >= 8, f"±50 px 透視で {n}/10"
