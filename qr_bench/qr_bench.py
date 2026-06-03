"""QR コード単一/複数構成 × 歪み条件の検出率ベンチマーク。

検出器: pyzbar (ZBar). cv2.QRCodeDetector より堅牢。
"""

from __future__ import annotations

import sys

import cv2
import numpy as np
import qrcode
from PIL import Image
from pyzbar import pyzbar
from qrcode.constants import ERROR_CORRECT_H, ERROR_CORRECT_L, ERROR_CORRECT_M, ERROR_CORRECT_Q

CANVAS_SIZE = 1024
TRIALS = 20

QR_CAPACITY = {
    10: {"L": 271, "M": 213, "Q": 151, "H": 119},
    15: {"L": 523, "M": 412, "Q": 292, "H": 220},
    20: {"L": 858, "M": 666, "Q": 482, "H": 382},
    25: {"L": 1273, "M": 991, "Q": 715, "H": 562},
    30: {"L": 1732, "M": 1373, "Q": 968, "H": 742},
    35: {"L": 2287, "M": 1812, "Q": 1286, "H": 991},
    40: {"L": 2953, "M": 2331, "Q": 1663, "H": 1273},
}


def qr_modules(version: int) -> int:
    return 21 + 4 * (version - 1)


EC_LEVELS = {"L": ERROR_CORRECT_L, "M": ERROR_CORRECT_M, "Q": ERROR_CORRECT_Q, "H": ERROR_CORRECT_H}


def _render_qr_pil(payload: bytes, version: int, ec_level: str, box_size: int) -> Image.Image:
    qr = qrcode.QRCode(version=version, error_correction=EC_LEVELS[ec_level], box_size=box_size, border=4)
    qr.add_data(payload)
    qr.make(fit=False)
    return qr.make_image(fill_color="black", back_color="white").convert("RGB")


def make_qr_image(payload: bytes, version: int, ec_level: str = "H") -> np.ndarray:
    """1 つの QR を「整数モジュール幅」で描画し、CANVAS_SIZE の白キャンバスに中央配置。"""
    n_modules = qr_modules(version) + 8  # border=4 × 2
    box_size = max(1, CANVAS_SIZE // n_modules)
    img_pil = _render_qr_pil(payload, version, ec_level, box_size)
    img = cv2.cvtColor(np.array(img_pil), cv2.COLOR_RGB2BGR)
    # 中央配置 (周りに白)
    h, w = img.shape[:2]
    canvas = np.full((CANVAS_SIZE, CANVAS_SIZE, 3), 255, dtype=np.uint8)
    y0 = (CANVAS_SIZE - h) // 2
    x0 = (CANVAS_SIZE - w) // 2
    canvas[y0:y0 + h, x0:x0 + w] = img
    return canvas


def make_qr_grid_image(
    payloads: list[bytes], version: int, grid: tuple[int, int], ec_level: str = "H"
) -> np.ndarray:
    """grid (rows, cols) で QR を並べる。各 QR は整数モジュール幅で描画。"""
    rows, cols = grid
    cell_size = CANVAS_SIZE // max(rows, cols)
    n_modules = qr_modules(version) + 8
    box_size = max(1, cell_size // n_modules)

    canvas = np.full((CANVAS_SIZE, CANVAS_SIZE, 3), 255, dtype=np.uint8)
    for i in range(rows * cols):
        payload = payloads[i % len(payloads)]
        img_pil = _render_qr_pil(payload, version, ec_level, box_size)
        img = cv2.cvtColor(np.array(img_pil), cv2.COLOR_RGB2BGR)
        h, w = img.shape[:2]
        r, c = divmod(i, cols)
        y_center = r * cell_size + cell_size // 2
        x_center = c * cell_size + cell_size // 2
        y0 = y_center - h // 2
        x0 = x_center - w // 2
        canvas[y0:y0 + h, x0:x0 + w] = img
    return canvas


def apply_distortion(
    img: np.ndarray,
    blur_sigma: float = 1.0,
    noise_sigma: float = 10.0,
    brightness: float = 1.0,
    perspective_strength: float = 0.03,
    moire_strength: float = 0.0,
    rng: np.random.Generator | None = None,
) -> np.ndarray:
    if rng is None:
        rng = np.random.default_rng()
    h, w = img.shape[:2]
    out = img.copy()
    if blur_sigma > 0:
        ksize = int(2 * np.ceil(3 * blur_sigma) + 1)
        out = cv2.GaussianBlur(out, (ksize, ksize), blur_sigma)
    if brightness != 1.0:
        out = np.clip(out.astype(np.float32) * brightness, 0, 255).astype(np.uint8)
    if moire_strength > 0:
        ys, xs = np.mgrid[0:h, 0:w]
        freq = 0.4 + rng.uniform(-0.1, 0.1)
        pattern = np.sin(2 * np.pi * freq * xs + rng.uniform(0, 2 * np.pi)) * np.sin(
            2 * np.pi * freq * ys + rng.uniform(0, 2 * np.pi)
        )
        out_f = out.astype(np.float32)
        out_f += (pattern * 255 * moire_strength)[..., None]
        out = np.clip(out_f, 0, 255).astype(np.uint8)
    if noise_sigma > 0:
        n = rng.normal(0, noise_sigma, out.shape).astype(np.float32)
        out = np.clip(out.astype(np.float32) + n, 0, 255).astype(np.uint8)
    if perspective_strength > 0:
        margin = perspective_strength * min(w, h)
        shifts = np.array(
            [
                [rng.uniform(0, margin), rng.uniform(0, margin)],
                [-rng.uniform(0, margin), rng.uniform(0, margin)],
                [-rng.uniform(0, margin), -rng.uniform(0, margin)],
                [rng.uniform(0, margin), -rng.uniform(0, margin)],
            ],
            dtype=np.float32,
        )
        src = np.array([[0, 0], [w, 0], [w, h], [0, h]], dtype=np.float32)
        dst = src + shifts
        m = cv2.getPerspectiveTransform(src, dst)
        out = cv2.warpPerspective(out, m, (w, h), borderValue=(255, 255, 255))
    return out


def decode_zbar(img: np.ndarray) -> int:
    """画像内の QR を全て検出して、復号できた個数を返す。"""
    gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
    results = pyzbar.decode(gray, symbols=[pyzbar.ZBarSymbol.QRCODE])
    return len(results)


def benchmark(
    version: int,
    grid: tuple[int, int],
    ec_level: str,
    distortion_params: dict,
    trials: int = TRIALS,
) -> tuple[float, int]:
    """成功率 (全 QR が読めた率) と平均ペイロード byte 数/フレーム を返す。"""
    rows, cols = grid
    n_qr = rows * cols
    cap_per_qr = QR_CAPACITY[version][ec_level]
    payload_per_qr = cap_per_qr - 10
    payloads = [bytes([ord("A") + (i % 26)] * payload_per_qr) for i in range(n_qr)]

    full_success = 0
    decoded_total = 0
    rng = np.random.default_rng(42)
    for _ in range(trials):
        if grid == (1, 1):
            img = make_qr_image(payloads[0], version, ec_level)
        else:
            img = make_qr_grid_image(payloads, version, grid, ec_level)
        distorted = apply_distortion(img, rng=rng, **distortion_params)
        n = decode_zbar(distorted)
        decoded_total += min(n, n_qr)
        if n >= n_qr:
            full_success += 1
    avg_decoded = decoded_total / trials
    return full_success / trials, int(avg_decoded * payload_per_qr)


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    configs = [
        (40, (1, 1), "H"),
        (40, (1, 1), "M"),
        (40, (1, 1), "L"),
        (30, (1, 1), "H"),
        (25, (1, 1), "H"),
        (20, (1, 1), "H"),
        (20, (2, 2), "H"),
        (25, (2, 2), "H"),
        (30, (2, 2), "H"),
        (15, (3, 3), "H"),
        (20, (3, 3), "H"),
        (10, (4, 4), "H"),
    ]

    distortions = [
        ("clean", dict(blur_sigma=0, noise_sigma=0, perspective_strength=0)),
        ("mild", dict(blur_sigma=0.8, noise_sigma=10, perspective_strength=0.02)),
        ("moderate", dict(blur_sigma=1.2, noise_sigma=20, perspective_strength=0.04)),
        ("camera-like", dict(blur_sigma=1.5, noise_sigma=20, perspective_strength=0.05, moire_strength=0.05)),
        ("harsh", dict(blur_sigma=2.0, noise_sigma=30, perspective_strength=0.06, moire_strength=0.12, brightness=0.9)),
    ]

    print(f"\nbenchmark (pyzbar/ZBar): {TRIALS} trials per config, canvas {CANVAS_SIZE}x{CANVAS_SIZE}\n")
    header = f"{'config':<18} " + " ".join(f"{name:>16}" for name, _ in distortions) + f" {'cap/frame':>9}"
    print(header)
    print("-" * len(header))

    results_table = []
    for version, grid, ec_level in configs:
        n_qr = grid[0] * grid[1]
        cap_per_qr = QR_CAPACITY[version][ec_level] - 10
        cap_per_frame = cap_per_qr * n_qr
        label = f"{n_qr}×V{version}-{ec_level}"
        if n_qr > 1:
            label += f" ({grid[0]}×{grid[1]})"
        row_results = {}
        for dname, dparams in distortions:
            rate, decoded = benchmark(version, grid, ec_level, dparams)
            row_results[dname] = (rate, decoded)
        row_str = f"{label:<18} " + " ".join(
            f"{row_results[dname][0]*100:>3.0f}% ({row_results[dname][1]:>5}B)"
            for dname, _ in distortions
        ) + f" {cap_per_frame:>8}B"
        print(row_str)
        results_table.append((label, cap_per_frame, row_results))

    print("\n効率比較 (1分間の実効ペイロード合計、平均復号 byte × fps × 60s):")
    print(f"{'config':<18}  {'clean 15fps':>14}  {'mild 15fps':>14}  {'moderate 15fps':>16}  {'camera 15fps':>14}")
    print("-" * 90)
    for label, _, row_results in results_table:
        d_clean = row_results["clean"][1] * 15 * 60
        d_mild = row_results["mild"][1] * 15 * 60
        d_mod = row_results["moderate"][1] * 15 * 60
        d_cam = row_results["camera-like"][1] * 15 * 60
        print(f"{label:<18}  {d_clean:>13}B  {d_mild:>13}B  {d_mod:>15}B  {d_cam:>13}B")


if __name__ == "__main__":
    main()
