"""色空間変換ユーティリティ (sRGB ↔ OKLab)。

OKLab (Björn Ottosson, 2020) は知覚的にほぼ等距離な色空間で、
パレット最近傍判定における色距離計算に適している。
"""

from __future__ import annotations

import numpy as np

_M1 = np.array(
    [
        [0.4122214708, 0.5363325363, 0.0514459929],
        [0.2119034982, 0.6806995451, 0.1073969566],
        [0.0883024619, 0.2817188376, 0.6299787005],
    ],
    dtype=np.float32,
)

_M2 = np.array(
    [
        [0.2104542553, 0.7936177850, -0.0040720468],
        [1.9779984951, -2.4285922050, 0.4505937099],
        [0.0259040371, 0.7827717662, -0.8086757660],
    ],
    dtype=np.float32,
)


def srgb_to_oklab(rgb: np.ndarray) -> np.ndarray:
    """sRGB (0..255, shape (..., 3)) → OKLab (shape (..., 3))。

    キャリブレーション補正後の値は範囲外 (<0 or >255) を取り得るため、内部で [0, 1] にクリップする。
    """
    arr = np.clip(rgb.astype(np.float32) / 255.0, 0.0, 1.0)
    linear = np.where(
        arr <= 0.04045,
        arr / 12.92,
        np.power((arr + 0.055) / 1.055, 2.4),
    )
    lms = linear @ _M1.T
    # cbrt は負値も扱える (np.cbrt は正負どちらも対応)。
    lms_cbrt = np.cbrt(lms)
    return lms_cbrt @ _M2.T
