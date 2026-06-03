"""Phase 0c.2 透視 unwarp の失敗解析。"""

from __future__ import annotations

import sys

import numpy as np

from beyond_qr_sender.distort import random_perspective
from beyond_qr_sender.geometry import unwarp_perspective
from beyond_qr_sender.render import (
    PHASE_0_8PX,
    cells_to_image,
    decode,
    encode,
    image_to_cells,
)

PAYLOAD = bytes((i * 31 + 7) & 0xFF for i in range(500))


def run_trial(max_shift: float, seed: int) -> tuple[bool, float]:
    spec = PHASE_0_8PX
    rng = np.random.default_rng(seed)
    cells_orig = encode(PAYLOAD, spec)
    image = cells_to_image(cells_orig, spec)
    w, h = image.size
    distorted, dst_corners = random_perspective(image, max_shift, rng)
    expected_corners = np.array([(0, 0), (w, 0), (w, h), (0, h)], dtype=np.float64)
    unwarped = unwarp_perspective(distorted, dst_corners, expected_corners, (w, h))
    cells_back = image_to_cells(unwarped, spec)
    a = np.frombuffer(cells_orig, dtype=np.uint8)
    b = np.frombuffer(cells_back, dtype=np.uint8)
    cer = float((a != b).mean())
    try:
        ok = decode(cells_back, spec) == PAYLOAD
    except ValueError:
        ok = False
    return ok, cer


def main() -> None:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    # No distortion baseline
    spec = PHASE_0_8PX
    cells = encode(PAYLOAD, spec)
    img = cells_to_image(cells, spec)
    cells_back = image_to_cells(img, spec)
    a = np.frombuffer(cells, dtype=np.uint8)
    b = np.frombuffer(cells_back, dtype=np.uint8)
    cer0 = float((a != b).mean())
    ok0 = decode(cells_back, spec) == PAYLOAD
    print(f"no distortion: ok={ok0} cer={cer0:.4%}")

    # Roundtrip identity (encode → image → identity transform back → decode)
    from PIL import Image
    img_save = img.copy()
    img_back = img_save.transform(img.size, Image.Transform.PERSPECTIVE,
                                  (1, 0, 0, 0, 1, 0, 0, 0),
                                  Image.Resampling.BILINEAR)
    cells_back2 = image_to_cells(img_back, spec)
    a2 = np.frombuffer(cells, dtype=np.uint8)
    b2 = np.frombuffer(cells_back2, dtype=np.uint8)
    cer_id = float((a2 != b2).mean())
    ok_id = decode(cells_back2, spec) == PAYLOAD
    print(f"identity transform: ok={ok_id} cer={cer_id:.4%}")

    print("\n--- ±10 px shift trials ---")
    for s in range(20):
        ok, cer = run_trial(10.0, s)
        print(f"  seed={s}: ok={ok} cer={cer:.4%}")


if __name__ == "__main__":
    main()
