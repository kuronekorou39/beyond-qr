"""ブラインド検出後の unwarp + 復号の失敗原因をデバッグ。"""

from __future__ import annotations

import sys

import numpy as np

from beyond_qr_sender.distort import random_perspective
from beyond_qr_sender.geometry import estimate_br_from_three, find_finder_centers
from beyond_qr_sender.render import (
    PHASE_0_8PX,
    cells_to_image,
    decode,
    encode,
    image_to_cells_perspective,
)

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))


def main() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    spec = PHASE_0_8PX
    cells_orig = encode(PAYLOAD, spec)
    image = cells_to_image(cells_orig, spec)
    w, h = image.size

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

    for max_shift in [10.0, 25.0]:
        print(f"\n=== max_shift = {max_shift} ===")
        successes = 0
        for seed in range(10):
            rng = np.random.default_rng(seed)
            distorted, dst_corners = random_perspective(image, max_shift, rng)
            arr = np.asarray(distorted, dtype=np.float32)
            observed_3 = find_finder_centers(arr, spec.finder_size, spec.cell_px)
            observed_br = estimate_br_from_three(observed_3[0], observed_3[1], observed_3[2])
            observed = np.vstack([observed_3, observed_br[None, :]])

            # 既知のグラウンドトゥルース observed (forward から計算) と比較
            from beyond_qr_sender.geometry import perspective_matrix
            forward = perspective_matrix(
                np.array([(0, 0), (w, 0), (w, h), (0, h)], dtype=np.float64),
                dst_corners,
            )
            ground_truth = []
            for ex, ey in expected:
                homog = np.array([ex, ey, 1.0])
                t = forward @ homog
                ground_truth.append((t[0] / t[2], t[1] / t[2]))
            ground_truth = np.array(ground_truth)

            cells_back = image_to_cells_perspective(
                distorted,
                observed_corners=observed,
                expected_corners=expected,
                spec=spec,
            )
            a = np.frombuffer(cells_orig, dtype=np.uint8)
            b = np.frombuffer(cells_back, dtype=np.uint8)
            cer = float((a != b).mean())
            try:
                ok = decode(cells_back, spec) == PAYLOAD
            except ValueError:
                ok = False
            if ok:
                successes += 1
            err_tl = np.linalg.norm(observed[0] - ground_truth[0])
            err_tr = np.linalg.norm(observed[1] - ground_truth[1])
            err_bl = np.linalg.norm(observed[2] - ground_truth[2])
            err_br = np.linalg.norm(observed[3] - ground_truth[3])
            print(
                f"  seed={seed}: ok={ok}, cer={cer:.3%}, "
                f"err(TL,TR,BL,BR)=({err_tl:.2f},{err_tr:.2f},{err_bl:.2f},{err_br:.2f})"
            )
        print(f"  total: {successes}/10")


if __name__ == "__main__":
    main()
