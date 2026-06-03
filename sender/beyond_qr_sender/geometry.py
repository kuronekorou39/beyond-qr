"""透視変換 (perspective transform) と逆変換 (unwarp) のユーティリティ。

Phase 0c.2 の幾何歪み試験で使用する。PIL.Image.transform(PERSPECTIVE) を薄くラップし、
3x3 ホモグラフィー行列の正方向/逆方向を扱う。
"""

from __future__ import annotations

import numpy as np
from PIL import Image


def perspective_matrix(src: np.ndarray, dst: np.ndarray) -> np.ndarray:
    """src → dst の 3x3 ホモグラフィーを SVD で解く。

    src, dst: (4, 2) の対応点。返り値は H/H[2,2] で正規化された 3x3 行列。
    """
    a_rows = []
    for (sx, sy), (dx, dy) in zip(src, dst):
        a_rows.append([sx, sy, 1, 0, 0, 0, -sx * dx, -sy * dx, -dx])
        a_rows.append([0, 0, 0, sx, sy, 1, -sx * dy, -sy * dy, -dy])
    a = np.array(a_rows, dtype=np.float64)
    # A @ h = 0 を解く (h は 9 要素、unit-norm null vector)
    _, _, vt = np.linalg.svd(a)
    h = vt[-1]
    matrix = h.reshape(3, 3)
    if abs(matrix[2, 2]) > 1e-12:
        matrix = matrix / matrix[2, 2]
    return matrix


def matrix_to_pil_coeffs(matrix: np.ndarray) -> tuple[float, ...]:
    """3x3 ホモグラフィー → PIL.Image.Transform.PERSPECTIVE 用 8 要素タプル。

    PIL は output → input マッピングを期待する: matrix は output 座標 (x_out, y_out) を
    input 座標 (x_in, y_in) に写像する。
    """
    return (
        float(matrix[0, 0]),
        float(matrix[0, 1]),
        float(matrix[0, 2]),
        float(matrix[1, 0]),
        float(matrix[1, 1]),
        float(matrix[1, 2]),
        float(matrix[2, 0]),
        float(matrix[2, 1]),
    )


def apply_perspective(image: Image.Image, dst_corners: np.ndarray) -> Image.Image:
    """元画像の 4 隅 (0,0), (w,0), (w,h), (0,h) を dst_corners に写像する歪みを適用する。

    dst_corners: (4, 2) で [TL, TR, BR, BL] の順。
    返り値は元と同サイズ。歪みで枠外に出た領域は黒で埋まる。
    """
    w, h = image.size
    src = np.array([(0, 0), (w, 0), (w, h), (0, h)], dtype=np.float64)
    h_forward = perspective_matrix(src, np.asarray(dst_corners, dtype=np.float64))
    # PIL は output→input を期待するので逆行列の係数を渡す。
    h_inverse = np.linalg.inv(h_forward)
    coeffs = matrix_to_pil_coeffs(h_inverse)
    return image.transform(
        image.size,
        Image.Transform.PERSPECTIVE,
        coeffs,
        Image.Resampling.BILINEAR,
    )


def unwarp_perspective(
    image: Image.Image,
    observed: np.ndarray,
    expected: np.ndarray,
    output_size: tuple[int, int],
) -> Image.Image:
    """歪んだ画像から、observed→expected で 4 点を合わせるように unwarp する。

    observed: (4, 2) 歪んだ画像上の点 (例: ファインダー中心 + BR 推定値)。
    expected: (4, 2) unwarp 後の理想位置。
    output_size: (W, H) unwarp 後の画像サイズ。
    """
    observed = np.asarray(observed, dtype=np.float64)
    expected = np.asarray(expected, dtype=np.float64)
    # output(=unwarped) → input(=distorted) のマッピング = expected → observed の写像
    matrix = perspective_matrix(expected, observed)
    coeffs = matrix_to_pil_coeffs(matrix)
    return image.transform(
        output_size,
        Image.Transform.PERSPECTIVE,
        coeffs,
        Image.Resampling.BILINEAR,
    )


def estimate_br_from_three(tl: np.ndarray, tr: np.ndarray, bl: np.ndarray) -> np.ndarray:
    """3 つの隅から BR を平行四辺形補完で推定する (緩やかな透視歪みなら近似可)。"""
    return tr + bl - tl


def bilinear_sample(image_arr: np.ndarray, xs: np.ndarray, ys: np.ndarray) -> np.ndarray:
    """(N,) の浮動小数点座標で image_arr (H, W, C) を bilinear サンプリングする。返り値 (N, C)。"""
    h, w = image_arr.shape[:2]
    xs = np.clip(xs, 0.0, w - 1.0)
    ys = np.clip(ys, 0.0, h - 1.0)
    x0 = np.floor(xs).astype(np.int64)
    y0 = np.floor(ys).astype(np.int64)
    x1 = np.minimum(x0 + 1, w - 1)
    y1 = np.minimum(y0 + 1, h - 1)
    wx = (xs - x0).astype(np.float32)
    wy = (ys - y0).astype(np.float32)
    v00 = image_arr[y0, x0]
    v01 = image_arr[y0, x1]
    v10 = image_arr[y1, x0]
    v11 = image_arr[y1, x1]
    v0 = v00 * (1.0 - wx[:, None]) + v01 * wx[:, None]
    v1 = v10 * (1.0 - wx[:, None]) + v11 * wx[:, None]
    return v0 * (1.0 - wy[:, None]) + v1 * wy[:, None]


def find_finder_centers(
    image_arr: np.ndarray,
    finder_size_cells: int,
    cell_px: int,
    dark_threshold: float = 64.0,
    light_threshold: float = 160.0,
) -> np.ndarray:
    """歪み画像から 3 つのファインダー (TL/TR/BL) の中心座標 (px) を検出する。

    検出スコア = (中心 3×3 セルの暗領域カウント) + (中心の外側 1 セル幅リングの明領域カウント)。
    中心が暗く、かつそれを取り囲む白リングが存在する位置を選ぶことで、データ領域に
    たまたま現れる暗クラスタや、キャリブレーション行の黒パッチを誤検出しにくくする。

    返り値: shape (4, 2) で [TL, TR, BL, BR] の (x, y) を float64 で返す。
    """
    gray = image_arr.mean(axis=2)
    dark = (gray < dark_threshold).astype(np.int64)
    light = (gray > light_threshold).astype(np.int64)
    h, w = dark.shape

    dark_int = np.zeros((h + 1, w + 1), dtype=np.int64)
    dark_int[1:, 1:] = dark.cumsum(0).cumsum(1)
    light_int = np.zeros((h + 1, w + 1), dtype=np.int64)
    light_int[1:, 1:] = light.cumsum(0).cumsum(1)

    def box_sum(integral: np.ndarray, y0: np.ndarray, x0: np.ndarray, y1: np.ndarray, x1: np.ndarray) -> np.ndarray:
        return (
            integral[np.ix_(y1, x1)]
            - integral[np.ix_(y0, x1)]
            - integral[np.ix_(y1, x0)]
            + integral[np.ix_(y0, x0)]
        )

    finder_pixel = finder_size_cells * cell_px
    inner_half = (3 * cell_px) // 2  # 中心 3×3 セル = 24 px → 半径 12
    middle_half = (5 * cell_px) // 2  # 中心 5×5 セル = 40 px → 半径 20

    expected = [
        (finder_pixel // 2, finder_pixel // 2),  # TL
        (w - finder_pixel // 2, finder_pixel // 2),  # TR
        (finder_pixel // 2, h - finder_pixel // 2),  # BL
        (w - finder_pixel // 2, h - finder_pixel // 2),  # BR
    ]
    search_radius = finder_pixel

    detected = []
    for ex, ey in expected:
        x_start = max(middle_half, ex - search_radius)
        x_end = min(w - middle_half, ex + search_radius)
        y_start = max(middle_half, ey - search_radius)
        y_end = min(h - middle_half, ey + search_radius)

        ys = np.arange(y_start, y_end)
        xs = np.arange(x_start, x_end)

        # 中心 3×3 セルの暗カウント
        center_dark = box_sum(dark_int, ys - inner_half, xs - inner_half, ys + inner_half, xs + inner_half)
        # 中心 5×5 セルの暗カウント / 明カウント
        middle_dark = box_sum(dark_int, ys - middle_half, xs - middle_half, ys + middle_half, xs + middle_half)
        middle_light = box_sum(light_int, ys - middle_half, xs - middle_half, ys + middle_half, xs + middle_half)
        # 内側白リング (5×5 中心 - 3×3 中心): 中心は暗、その外側 1 セル幅は白
        ring_light = middle_light  # 明カウントは 5×5 中の合計 (中心は通常暗なので 0 → 明はリングに集中)
        ring_dark = middle_dark - center_dark  # 5×5 リングの暗カウント (低いほど良い)

        scores = center_dark + ring_light - ring_dark
        best = np.unravel_index(np.argmax(scores), scores.shape)
        detected.append((float(xs[best[1]]), float(ys[best[0]])))

    return np.array(detected, dtype=np.float64)


def sample_cells_through_perspective(
    image_arr: np.ndarray,
    observed_corners: np.ndarray,
    expected_corners: np.ndarray,
    grid_width: int,
    grid_height: int,
    cell_px: int,
    samples_per_axis: int = 4,
) -> np.ndarray:
    """歪み画像から各セルの中心領域 (samples_per_axis × samples_per_axis 点) を直接サンプリングする。

    expected_corners (unwarped 座標) で各セルの中心領域を取り、forward perspective で
    歪み画像座標に変換、bilinear で1回だけ補間して平均する。
    PIL の unwarp による二重補間を避けることで境界の混色を最小化する。

    返り値: (grid_height, grid_width, 3) のセル色 (sRGB float32)。
    """
    # expected → observed の forward homography
    forward = perspective_matrix(
        np.asarray(expected_corners, dtype=np.float64),
        np.asarray(observed_corners, dtype=np.float64),
    )

    # 各セル内の samples_per_axis × samples_per_axis サンプル点を中央寄せで配置
    # 中心 (cell_px/2) を中心に、cell_px/2 × cell_px/2 の領域内に均等配置
    border = cell_px / 4.0
    offsets = np.linspace(border, cell_px - border - 1, samples_per_axis)

    # unwarped 座標系での各セルの sample 点 (gh, gw, sp, sp, 2)
    gys, gxs = np.meshgrid(np.arange(grid_height), np.arange(grid_width), indexing="ij")
    cell_origin_x = (gxs.astype(np.float64) * cell_px).reshape(grid_height, grid_width, 1, 1)
    cell_origin_y = (gys.astype(np.float64) * cell_px).reshape(grid_height, grid_width, 1, 1)
    ox = offsets.reshape(1, 1, 1, samples_per_axis)
    oy = offsets.reshape(1, 1, samples_per_axis, 1)
    sample_x = cell_origin_x + ox  # (gh, gw, 1, sp) → broadcast
    sample_y = cell_origin_y + oy  # (gh, gw, sp, 1)
    sample_x_full = np.broadcast_to(sample_x, (grid_height, grid_width, samples_per_axis, samples_per_axis))
    sample_y_full = np.broadcast_to(sample_y, (grid_height, grid_width, samples_per_axis, samples_per_axis))
    flat_x = sample_x_full.flatten()
    flat_y = sample_y_full.flatten()

    # forward 変換で歪み画像座標へ
    homog = np.stack([flat_x, flat_y, np.ones_like(flat_x)], axis=1)  # (N, 3)
    transformed = homog @ forward.T
    dx = transformed[:, 0] / transformed[:, 2]
    dy = transformed[:, 1] / transformed[:, 2]

    samples = bilinear_sample(image_arr, dx, dy)  # (N, 3)
    n_samples = samples_per_axis * samples_per_axis
    samples = samples.reshape(grid_height, grid_width, n_samples, 3)
    return samples.mean(axis=2)
