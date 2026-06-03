"""セル列 ↔ PNG 画像の変換 (可変仕様 + キャリブレーション対応)。

Phase 0c.1 から:
- FrameSpec に calibration_rows を追加
- image_to_cells はキャリブレーションパッチから 3×3 アフィン変換を最小二乗フィットし、
  データセルに逆適用してから OKLab 距離で量子化する
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from PIL import Image

import beyond_qr_core as core

from .color import srgb_to_oklab
from .geometry import sample_cells_through_perspective

_PALETTE_ARRAY = np.array(core.palette_rgb(), dtype=np.uint8)  # (8, 3)
_PALETTE_OKLAB = srgb_to_oklab(_PALETTE_ARRAY.astype(np.float32))  # (8, 3)


@dataclass(frozen=True)
class FrameSpec:
    grid_width: int = 128
    grid_height: int = 128
    cell_px: int = 8
    finder_size: int = 7
    calibration_row_start: int = 64
    calibration_rows: int = 1

    @property
    def total_cells(self) -> int:
        return self.grid_width * self.grid_height

    @property
    def image_dimensions(self) -> tuple[int, int]:
        return (self.grid_width * self.cell_px, self.grid_height * self.cell_px)

    @property
    def max_payload_bytes(self) -> int:
        _, _, _, _, _, _, max_payload, _ = core.frame_spec(
            grid_width=self.grid_width,
            grid_height=self.grid_height,
            cell_px=self.cell_px,
            finder_size=self.finder_size,
            calibration_row_start=self.calibration_row_start,
            calibration_rows=self.calibration_rows,
        )
        return max_payload

    @property
    def rs_blocks(self) -> int:
        _, _, _, _, _, _, _, blocks = core.frame_spec(
            grid_width=self.grid_width,
            grid_height=self.grid_height,
            cell_px=self.cell_px,
            finder_size=self.finder_size,
            calibration_row_start=self.calibration_row_start,
            calibration_rows=self.calibration_rows,
        )
        return blocks

    def calibration_patch_col_range(self, patch_idx: int) -> tuple[int, int]:
        """パッチ i (0..=7) の col 範囲 (start_inclusive, end_exclusive) を返す。"""
        return (
            (patch_idx * self.grid_width) // 8,
            ((patch_idx + 1) * self.grid_width) // 8,
        )

    def finder_corners_in_pixels(self) -> list[tuple[int, int, int, int]]:
        """3 つのファインダーの (top, left, bottom, right) を px 単位で返す (TL, TR, BL)。"""
        fp = self.finder_size * self.cell_px
        w, h = self.image_dimensions
        return [
            (0, 0, fp, fp),  # TL
            (0, w - fp, fp, w),  # TR
            (h - fp, 0, h, fp),  # BL
        ]


# Phase 0 系列のプリセット (1024×1024 image を維持、キャリブレーション行は画像中央)
PHASE_0_16PX = FrameSpec(64, 64, 16, finder_size=7, calibration_row_start=32, calibration_rows=1)
PHASE_0_8PX = FrameSpec(128, 128, 8, finder_size=7, calibration_row_start=64, calibration_rows=1)
PHASE_0_6PX = FrameSpec(170, 170, 6, finder_size=7, calibration_row_start=85, calibration_rows=1)
PHASE_0_4PX = FrameSpec(256, 256, 4, finder_size=7, calibration_row_start=128, calibration_rows=1)

# 既定値
DEFAULT_SPEC = PHASE_0_8PX


def cells_to_image(cells: bytes, spec: FrameSpec = DEFAULT_SPEC) -> Image.Image:
    """セル列を spec に応じた RGB 画像に変換する。"""
    if len(cells) != spec.total_cells:
        raise ValueError(f"expected {spec.total_cells} cells, got {len(cells)}")
    arr = np.frombuffer(cells, dtype=np.uint8).reshape(spec.grid_height, spec.grid_width)
    rgb_grid = _PALETTE_ARRAY[arr]  # (H, W, 3)
    image_array = np.repeat(np.repeat(rgb_grid, spec.cell_px, axis=0), spec.cell_px, axis=1)
    return Image.fromarray(image_array, mode="RGB")


def image_to_cells(image: Image.Image, spec: FrameSpec = DEFAULT_SPEC) -> bytes:
    """グリッド整合 RGB 画像からセル列を復元する。"""
    if image.mode != "RGB":
        image = image.convert("RGB")
    expected = spec.image_dimensions
    if image.size != expected:
        raise ValueError(f"expected image size {expected}, got {image.size}")
    arr = np.asarray(image, dtype=np.float32)
    centers = _sample_cell_centers(arr, spec)
    return _quantize_with_calibration(centers, spec)


def image_to_cells_perspective(
    image: Image.Image,
    observed_corners: np.ndarray,
    expected_corners: np.ndarray,
    spec: FrameSpec = DEFAULT_SPEC,
    samples_per_axis: int = 4,
) -> bytes:
    """透視歪みのある画像から、観測/期待コーナーを使って直接サンプリングする。

    PIL の unwarp による二重 bilinear 補間を避け、forward perspective で各セル中央領域
    を歪み画像から直接 1 回だけサンプリングして平均する。
    """
    if image.mode != "RGB":
        image = image.convert("RGB")
    arr = np.asarray(image, dtype=np.float32)
    centers = sample_cells_through_perspective(
        arr,
        observed_corners=np.asarray(observed_corners, dtype=np.float64),
        expected_corners=np.asarray(expected_corners, dtype=np.float64),
        grid_width=spec.grid_width,
        grid_height=spec.grid_height,
        cell_px=spec.cell_px,
        samples_per_axis=samples_per_axis,
    )
    return _quantize_with_calibration(centers, spec)


def _quantize_with_calibration(centers: np.ndarray, spec: FrameSpec) -> bytes:
    """中心色配列にキャリブレーション補正 + OKLab 量子化を適用してセル列を返す。"""
    if spec.calibration_rows > 0:
        observed = _sample_calibration_patches(centers, spec)  # (8, 3)
        true_palette = _PALETTE_ARRAY.astype(np.float32)  # (8, 3)
        c_matrix, *_ = np.linalg.lstsq(observed, true_palette, rcond=None)
        corrected = centers @ c_matrix
    else:
        corrected = centers

    flat = corrected.reshape(-1, 3)
    flat_oklab = srgb_to_oklab(flat)
    diffs = flat_oklab[:, None, :] - _PALETTE_OKLAB[None, :, :]
    dists = (diffs * diffs).sum(axis=2)
    indices = dists.argmin(axis=1).astype(np.uint8)
    return indices.tobytes()


def _sample_cell_centers(image_arr: np.ndarray, spec: FrameSpec) -> np.ndarray:
    """各セル中心の (cell_px/2)×(cell_px/2) 領域を平均して (grid_h, grid_w, 3) を返す。

    cell_px が 2 未満の場合は中心 1 ピクセルにフォールバックする。
    """
    cp = spec.cell_px
    if cp < 2:
        half = cp // 2
        return image_arr[half::cp, half::cp]
    # 中心の cp/2 × cp/2 領域を取り出す
    border = cp // 4
    # (grid_h, cp, grid_w, cp, 3) に reshape
    reshaped = image_arr.reshape(spec.grid_height, cp, spec.grid_width, cp, 3)
    center_block = reshaped[:, border : cp - border, :, border : cp - border, :]
    return center_block.mean(axis=(1, 3))


def _sample_calibration_patches(centers: np.ndarray, spec: FrameSpec) -> np.ndarray:
    """各キャリブレーションパッチの観測色 (パッチ内平均) を (8, 3) で返す。

    キャリブレーション行はファインダー直下 (finder_size 行目) から始まる。
    各パッチはファインダー領域 (左右両端の cols) を除外して平均をとる方が安定だが、
    Phase 0c.1 時点では grid_width が 128 と十分大きく、最左/最右のパッチでもファインダー
    幅 (7 cells) の影響は限定的なので、まずは単純な col 範囲で平均する。
    """
    samples = []
    cal_start = spec.calibration_row_start
    cal_end = cal_start + spec.calibration_rows
    for i in range(8):
        col_start, col_end = spec.calibration_patch_col_range(i)
        patch = centers[cal_start:cal_end, col_start:col_end]
        avg = patch.reshape(-1, 3).mean(axis=0)
        samples.append(avg)
    return np.array(samples, dtype=np.float32)


def encode(payload: bytes, spec: FrameSpec = DEFAULT_SPEC) -> bytes:
    """spec に応じてペイロードをセル列に符号化する。"""
    return core.encode(
        payload,
        grid_width=spec.grid_width,
        grid_height=spec.grid_height,
        cell_px=spec.cell_px,
        finder_size=spec.finder_size,
        calibration_row_start=spec.calibration_row_start,
        calibration_rows=spec.calibration_rows,
    )


def decode(cells: bytes, spec: FrameSpec = DEFAULT_SPEC) -> bytes:
    """spec に応じてセル列をペイロードに復号する。"""
    return core.decode(
        cells,
        grid_width=spec.grid_width,
        grid_height=spec.grid_height,
        cell_px=spec.cell_px,
        finder_size=spec.finder_size,
        calibration_row_start=spec.calibration_row_start,
        calibration_rows=spec.calibration_rows,
    )
