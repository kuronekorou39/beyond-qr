"""Phase 0c.1 キャリブレーションの効果計測。

同じ歪み条件で `calibration_rows=0` (補正無し) と `calibration_rows=1` (補正有り) を
並べて成功率を比較する。0c.0 で破綻していた「dim+WB+noise」が回復するかを見る。

Usage:
    python sender/scripts/calibration_impact.py
"""

from __future__ import annotations

import sys

import numpy as np

from beyond_qr_sender.distort import (
    add_gaussian_noise,
    shift_brightness,
    shift_white_balance,
)
from beyond_qr_sender.render import (
    FrameSpec,
    cells_to_image,
    decode,
    encode,
    image_to_cells,
)

PAYLOAD_SIZE = 500
N_TRIALS = 20
PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(PAYLOAD_SIZE))


def trial(spec: FrameSpec, payload: bytes, distort_fn) -> tuple[bool, float]:
    cells = encode(payload, spec)
    image = cells_to_image(cells, spec)
    distorted = distort_fn(image)
    cells_back = image_to_cells(distorted, spec)
    a = np.frombuffer(cells, dtype=np.uint8)
    b = np.frombuffer(cells_back, dtype=np.uint8)
    cer = float((a != b).mean())
    try:
        ok = decode(cells_back, spec) == payload
    except ValueError:
        ok = False
    return ok, cer


def measure(spec: FrameSpec, name: str) -> None:
    print(f"\n=== {name} (calibration_rows={spec.calibration_rows}, payload={PAYLOAD_SIZE}B) ===")
    print(f"{'condition':>32} | {'success':>9} | {'avg CER':>8}")
    print("-" * 56)
    rng = np.random.default_rng(seed=42)
    cases = [
        ("baseline (no distortion)", lambda img: img),
        ("noise s=20", lambda img: add_gaussian_noise(img, 20, rng)),
        ("dim x0.7", lambda img: shift_brightness(img, 0.7)),
        ("dim x0.5", lambda img: shift_brightness(img, 0.5)),
        ("WB (1.3, 1.0, 0.7)", lambda img: shift_white_balance(img, 1.3, 1.0, 0.7)),
        (
            "dim x0.7 + WB + noise s=20",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.7), 1.2, 1.0, 0.8),
                20,
                rng,
            ),
        ),
        (
            "dim x0.5 + WB + noise s=20",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.5), 1.2, 1.0, 0.8),
                20,
                rng,
            ),
        ),
        (
            "harsh: x0.6 + strong WB + s=30",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.6), 1.4, 1.0, 0.6),
                30,
                rng,
            ),
        ),
        (
            "very harsh: x0.4 + WB + s=40",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.4), 1.3, 1.0, 0.7),
                40,
                rng,
            ),
        ),
    ]
    for cond, fn in cases:
        succ = 0
        cers = []
        for _ in range(N_TRIALS):
            ok, cer = trial(spec, PAYLOAD, fn)
            if ok:
                succ += 1
            cers.append(cer)
        rate = f"{succ:2d}/{N_TRIALS}"
        print(f"{cond:>32} | {rate:>9} | {np.mean(cers):>7.2%}")


def main() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    spec_no_cal = FrameSpec(128, 128, 8, calibration_rows=0)
    spec_with_cal = FrameSpec(128, 128, 8, calibration_rows=1)

    measure(spec_no_cal, "8px / no calibration")
    measure(spec_with_cal, "8px / with calibration")


if __name__ == "__main__":
    main()
