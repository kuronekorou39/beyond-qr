"""Phase 0c.0 ベースライン測定スクリプト。

クリーン PNG にノイズ・明度・WB のシフトを段階的に適用し、
現状の復号器(キャリブレーション無し、sRGB ユークリッド距離)の
復号成功率を計測してテーブル出力する。

Usage:
    python sender/scripts/baseline_report.py
"""

from __future__ import annotations

import numpy as np

import beyond_qr_core as core
from beyond_qr_sender.distort import (
    add_gaussian_noise,
    shift_brightness,
    shift_white_balance,
)
from beyond_qr_sender.render import cells_to_image, image_to_cells

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))
N_TRIALS = 20


def cell_error_rate(original_cells: bytes, recovered_cells: bytes) -> float:
    a = np.frombuffer(original_cells, dtype=np.uint8)
    b = np.frombuffer(recovered_cells, dtype=np.uint8)
    return float((a != b).mean())


def trial(distort_fn) -> tuple[bool, float]:
    """1 試行: 歪みを掛けて復号、(成功?, セル誤り率) を返す。"""
    cells_orig = core.encode(PAYLOAD)
    image = cells_to_image(cells_orig)
    distorted = distort_fn(image)
    cells_back = image_to_cells(distorted)
    cer = cell_error_rate(cells_orig, cells_back)
    try:
        ok = core.decode(cells_back) == PAYLOAD
    except ValueError:
        ok = False
    return ok, cer


def measure_noise():
    print("=== Gaussian noise (σ on 0..255 scale) ===")
    print(f"{'σ':>4} | {'success':>9} | {'avg CER':>8}")
    print("-" * 32)
    rng = np.random.default_rng(seed=42)
    for sigma in [0, 5, 10, 20, 30, 40, 50, 60, 80, 100]:
        successes = 0
        cers = []
        for _ in range(N_TRIALS):
            ok, cer = trial(lambda img, s=sigma: add_gaussian_noise(img, s, rng))
            if ok:
                successes += 1
            cers.append(cer)
        rate = f"{successes}/{N_TRIALS}"
        print(f"{sigma:>4} | {rate:>9} | {np.mean(cers):>7.2%}")


def measure_brightness():
    print("\n=== Brightness factor (1.0 = no change) ===")
    print(f"{'factor':>6} | {'success':>7} | {'CER':>6}")
    print("-" * 26)
    for factor in [0.3, 0.5, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.5, 2.0]:
        ok, cer = trial(lambda img, f=factor: shift_brightness(img, f))
        mark = "OK " if ok else "FAIL"
        print(f"{factor:>6} | {mark:>7} | {cer:>5.1%}")


def measure_white_balance():
    print("\n=== White balance (R, G, B gains) ===")
    print(f"{'gains':>18} | {'success':>7} | {'CER':>6}")
    print("-" * 38)
    cases = [
        (1.0, 1.0, 1.0),
        (1.1, 1.0, 0.9),
        (1.2, 1.0, 0.8),
        (1.3, 1.0, 0.7),
        (0.7, 1.0, 1.3),
        (0.8, 1.2, 0.8),
    ]
    for r, g, b in cases:
        ok, cer = trial(lambda img, r=r, g=g, b=b: shift_white_balance(img, r, g, b))
        mark = "OK " if ok else "FAIL"
        gains_str = f"({r:.1f}, {g:.1f}, {b:.1f})"
        print(f"{gains_str:>18} | {mark:>7} | {cer:>5.1%}")


def measure_combined():
    print("\n=== Combined distortion (現実のカメラ撮影に近い条件) ===")
    print(f"{'condition':>32} | {'success':>9} | {'avg CER':>8}")
    print("-" * 56)
    rng = np.random.default_rng(seed=42)
    cases = [
        (
            "noise σ=10 のみ",
            lambda img: add_gaussian_noise(img, 10, rng),
        ),
        (
            "明 0.7× + noise σ=20",
            lambda img: add_gaussian_noise(shift_brightness(img, 0.7), 20, rng),
        ),
        (
            "明 0.7× + WB 偏り + noise σ=20",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.7), 1.2, 1.0, 0.8),
                20,
                rng,
            ),
        ),
        (
            "明 0.5× + WB 偏り + noise σ=20",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.5), 1.2, 1.0, 0.8),
                20,
                rng,
            ),
        ),
        (
            "明 0.6× + 強い WB + noise σ=30",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.6), 1.4, 1.0, 0.6),
                30,
                rng,
            ),
        ),
    ]
    for name, fn in cases:
        successes = 0
        cers = []
        for _ in range(N_TRIALS):
            ok, cer = trial(fn)
            if ok:
                successes += 1
            cers.append(cer)
        rate = f"{successes}/{N_TRIALS}"
        print(f"{name:>32} | {rate:>9} | {np.mean(cers):>7.2%}")


if __name__ == "__main__":
    measure_noise()
    measure_brightness()
    measure_white_balance()
    measure_combined()
