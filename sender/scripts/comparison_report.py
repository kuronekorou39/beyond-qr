"""Phase 0c.0 セル粒度比較。

4px / 6px / 8px / 16px のセル粒度で容量と歪み耐性を計測する。
画像サイズはおおむね 1024×1024 に揃え、ペイロード 500 byte 固定で比較。

Usage:
    python sender/scripts/comparison_report.py
"""

from __future__ import annotations

import numpy as np

from beyond_qr_sender.distort import (
    add_gaussian_noise,
    shift_brightness,
    shift_white_balance,
)
from beyond_qr_sender.render import (
    PHASE_0_4PX,
    PHASE_0_6PX,
    PHASE_0_8PX,
    PHASE_0_16PX,
    FrameSpec,
    cells_to_image,
    decode,
    encode,
    image_to_cells,
)

SPECS: list[FrameSpec] = [PHASE_0_4PX, PHASE_0_6PX, PHASE_0_8PX, PHASE_0_16PX]
PAYLOAD_SIZE = 500
N_TRIALS = 20
PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(PAYLOAD_SIZE))


def cell_error_rate(a: bytes, b: bytes) -> float:
    arr_a = np.frombuffer(a, dtype=np.uint8)
    arr_b = np.frombuffer(b, dtype=np.uint8)
    return float((arr_a != arr_b).mean())


def run_trial(spec: FrameSpec, payload: bytes, distort_fn) -> tuple[bool, float]:
    cells = encode(payload, spec)
    image = cells_to_image(cells, spec)
    distorted = distort_fn(image)
    cells_back = image_to_cells(distorted, spec)
    cer = cell_error_rate(cells, cells_back)
    try:
        ok = decode(cells_back, spec) == payload
    except ValueError:
        ok = False
    return ok, cer


def spec_label(s: FrameSpec) -> str:
    w, h = s.image_dimensions
    return f"{s.cell_px}px {s.grid_width}x{s.grid_height} ({w}x{h})"


def show_capacity():
    print("=== capacity (1024x1024 image target) ===")
    print(f"{'spec':>26} | {'cells':>7} | {'rs blocks':>9} | {'max payload':>12}")
    print("-" * 64)
    for s in SPECS:
        print(
            f"{spec_label(s):>26} | "
            f"{s.total_cells:>7} | "
            f"{s.rs_blocks:>9} | "
            f"{s.max_payload_bytes:>11}B"
        )


def show_noise_tolerance():
    print(f"\n=== noise tolerance ({N_TRIALS} trials, {PAYLOAD_SIZE}B payload) ===")
    sigmas = [10, 30, 50, 60, 80]
    header = " | ".join([f"{f's={s}':>7}" for s in sigmas])
    print(f"{'spec':>26} | {header}")
    print("-" * (28 + len(sigmas) * 10))
    rng = np.random.default_rng(seed=42)
    for s in SPECS:
        results = []
        for sigma in sigmas:
            succ = 0
            for _ in range(N_TRIALS):
                ok, _ = run_trial(
                    s, PAYLOAD, lambda img, sig=sigma: add_gaussian_noise(img, sig, rng)
                )
                if ok:
                    succ += 1
            results.append(f"{succ:2d}/{N_TRIALS}")
        row = " | ".join(f"{r:>7}" for r in results)
        print(f"{spec_label(s):>26} | {row}")


def show_brightness_tolerance():
    print(f"\n=== brightness tolerance ({PAYLOAD_SIZE}B payload, 1 trial each) ===")
    factors = [0.3, 0.5, 0.7, 1.0, 1.5, 2.0]
    header = " | ".join([f"{f'x{f}':>4}" for f in factors])
    print(f"{'spec':>26} | {header}")
    print("-" * (28 + len(factors) * 7))
    for s in SPECS:
        results = []
        for factor in factors:
            ok, _ = run_trial(s, PAYLOAD, lambda img, f=factor: shift_brightness(img, f))
            results.append("OK" if ok else " X")
        row = " | ".join(f"{r:>4}" for r in results)
        print(f"{spec_label(s):>26} | {row}")


def show_combined():
    print(f"\n=== combined distortion (realistic camera, {N_TRIALS} trials) ===")
    rng = np.random.default_rng(seed=42)
    cases = [
        ("mild (noise s=10)", lambda img: add_gaussian_noise(img, 10, rng)),
        (
            "dim+noise (x0.7 + s=20)",
            lambda img: add_gaussian_noise(shift_brightness(img, 0.7), 20, rng),
        ),
        (
            "dim+WB+noise (x0.7+WB+s=20)",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.7), 1.2, 1.0, 0.8),
                20,
                rng,
            ),
        ),
        (
            "harsh (x0.6+WB+s=30)",
            lambda img: add_gaussian_noise(
                shift_white_balance(shift_brightness(img, 0.6), 1.4, 1.0, 0.6),
                30,
                rng,
            ),
        ),
    ]
    header = " | ".join([f"{name:>26}" for name, _ in cases])
    print(f"{'spec':>26} | {header}")
    print("-" * (28 + len(cases) * 29))
    for s in SPECS:
        results = []
        for _, fn in cases:
            succ = 0
            for _ in range(N_TRIALS):
                ok, _ = run_trial(s, PAYLOAD, fn)
                if ok:
                    succ += 1
            results.append(f"{succ:2d}/{N_TRIALS}")
        row = " | ".join(f"{r:>26}" for r in results)
        print(f"{spec_label(s):>26} | {row}")


def main() -> None:
    import sys

    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    show_capacity()
    show_noise_tolerance()
    show_brightness_tolerance()
    show_combined()


if __name__ == "__main__":
    main()
