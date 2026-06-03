"""Phase 0c.1 受け入れテスト: キャリブレーション + 領域平均化 + OKLab で
中等度の組み合わせ歪みから復元できる。
"""

from __future__ import annotations

import numpy as np

from beyond_qr_sender.distort import (
    add_gaussian_noise,
    shift_brightness,
    shift_white_balance,
)
from beyond_qr_sender.render import (
    PHASE_0_8PX,
    cells_to_image,
    decode,
    encode,
    image_to_cells,
)

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))


def _run(distort_fn, n_trials: int = 10) -> int:
    """歪みを適用して n_trials 試行し、復号成功数を返す。"""
    successes = 0
    for _ in range(n_trials):
        cells = encode(PAYLOAD, PHASE_0_8PX)
        image = cells_to_image(cells, PHASE_0_8PX)
        distorted = distort_fn(image)
        cells_back = image_to_cells(distorted, PHASE_0_8PX)
        try:
            if decode(cells_back, PHASE_0_8PX) == PAYLOAD:
                successes += 1
        except ValueError:
            pass
    return successes


def test_recovers_from_dim_only():
    n = _run(lambda img: shift_brightness(img, 0.5))
    assert n == 10, f"明 0.5x で {n}/10 — キャリブレーションが効いていない"


def test_recovers_from_dim_plus_wb():
    n = _run(lambda img: shift_white_balance(shift_brightness(img, 0.7), 1.2, 1.0, 0.8))
    assert n == 10, f"明 0.7x + WB で {n}/10"


def test_recovers_from_dim_plus_wb_plus_noise():
    """Phase 0c.1 の主目標。0c.0 では 0/20 だった条件が回復していること。"""
    rng = np.random.default_rng(seed=42)
    n = _run(
        lambda img: add_gaussian_noise(
            shift_white_balance(shift_brightness(img, 0.7), 1.2, 1.0, 0.8),
            20,
            rng,
        )
    )
    assert n == 10, f"dim+WB+noise で {n}/10 — 0c.1 の主目標未達"


def test_recovers_from_harsh_combined():
    """harsh ケース (x0.6 + strong WB + noise s=30) も全件復元できる。"""
    rng = np.random.default_rng(seed=42)
    n = _run(
        lambda img: add_gaussian_noise(
            shift_white_balance(shift_brightness(img, 0.6), 1.4, 1.0, 0.6),
            30,
            rng,
        )
    )
    assert n >= 9, f"harsh で {n}/10 — 期待 9 以上"
