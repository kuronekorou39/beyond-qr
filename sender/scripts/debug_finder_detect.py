"""Phase 0c.2 ファインダー検出のデバッグ。"""

from __future__ import annotations

import sys

import numpy as np

from beyond_qr_sender.distort import random_perspective
from beyond_qr_sender.geometry import find_finder_centers
from beyond_qr_sender.render import (
    PHASE_0_8PX,
    cells_to_image,
    encode,
)

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))


def main() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    spec = PHASE_0_8PX
    cells = encode(PAYLOAD, spec)
    image = cells_to_image(cells, spec)
    w, h = image.size

    fp_half = spec.finder_size * spec.cell_px / 2
    expected = np.array(
        [
            (fp_half, fp_half),
            (w - fp_half, fp_half),
            (fp_half, h - fp_half),
        ],
        dtype=np.float64,
    )
    print(f"expected: {expected.tolist()}")

    # まず歪み無しで検出が当たるか確認
    arr0 = np.asarray(image, dtype=np.float32)
    detected_0 = find_finder_centers(arr0, spec.finder_size, spec.cell_px)
    print(f"\nno distortion detected: {detected_0.tolist()}")

    # 手動で box score を BL center 候補位置で計算
    gray = arr0.mean(axis=2)
    dark = (gray < 64.0).astype(np.int64)

    def box_sum(cy, cx, half=12):
        y0, y1 = cy - half, cy + half
        x0, x1 = cx - half, cx + half
        return int(dark[y0:y1, x0:x1].sum())

    print("\n  BL box scores at key positions:")
    print(f"    (28, 996) [BL finder center]: {box_sum(996, 28)}")
    print(f"    (12, 940) [search corner]:    {box_sum(940, 12)}")
    print(f"    (12, 996) [TR side at BL row]: {box_sum(996, 12)}")
    print(f"    (84, 996) [further right BL]:  {box_sum(996, 84)}")
    print(f"    (40, 996):                     {box_sum(996, 40)}")

    # 軽い歪み
    for seed in range(3):
        rng = np.random.default_rng(seed)
        distorted, dst_corners = random_perspective(image, 10, rng)
        arr = np.asarray(distorted, dtype=np.float32)
        detected = find_finder_centers(arr, spec.finder_size, spec.cell_px)
        print(f"\nseed={seed}, dst_corners={dst_corners.tolist()}")
        print(f"  detected: {detected.tolist()}")

        # 歪み画像で TL 領域のスコア比較
        gray_d = arr.mean(axis=2)
        dark_d = (gray_d < 64.0).astype(np.int64)
        def bs(cy, cx, half=12):
            y0, y1 = cy - half, cy + half
            x0, x1 = cx - half, cx + half
            return int(dark_d[y0:y1, x0:x1].sum())
        print(f"  TL box scores: (28,28)={bs(28, 28)}, (34,30)={bs(30, 34)}, (12,63)={bs(63, 12)}, (12,84)={bs(84, 12)}")
        # dst_corners[0] が TL の元シフト先 — それに finder の (28, 28) を加えたあたりに検出されるはず
        # forward 変換で expected[0] = (28,28) がどこに行くかを直接計算
        from beyond_qr_sender.geometry import perspective_matrix
        original_corners = np.array([(0, 0), (w, 0), (w, h), (0, h)], dtype=np.float64)
        forward = perspective_matrix(original_corners, dst_corners)
        ground_truth_observed = []
        for ex, ey in expected:
            homog = np.array([ex, ey, 1.0])
            transformed = forward @ homog
            ground_truth_observed.append((transformed[0] / transformed[2], transformed[1] / transformed[2]))
        print(f"  ground truth observed (from forward): {ground_truth_observed}")


if __name__ == "__main__":
    main()
