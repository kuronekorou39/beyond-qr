"""Phase 0b: PNG round-trip テスト。

クリーン (歪み・ノイズなし) な PNG を介した bytes → PNG → bytes の往復が
完全一致することを検証する。
"""

from __future__ import annotations

import io
import random
from pathlib import Path

import pytest
from PIL import Image

import beyond_qr_core as core
from beyond_qr_sender.decode import main as decode_main
from beyond_qr_sender.encode import main as encode_main
from beyond_qr_sender.render import cells_to_image, image_to_cells


def _save_load_png(image: Image.Image) -> Image.Image:
    buf = io.BytesIO()
    image.save(buf, format="PNG")
    buf.seek(0)
    return Image.open(buf)


def test_cells_to_image_dimensions():
    grid_w, grid_h, cell_px, finder, cal_start, cal_rows, _, _ = core.frame_spec()
    assert (grid_w, grid_h, cell_px, finder, cal_start, cal_rows) == (128, 128, 8, 7, 64, 1)
    payload = b"hello"
    cells = core.encode(payload)
    image = cells_to_image(cells)
    assert image.size == (1024, 1024)
    assert image.mode == "RGB"


def test_clean_png_roundtrip_500_byte():
    payload = bytes((i * 31 + 7) & 0xFF for i in range(500))
    cells_enc = core.encode(payload)
    image = cells_to_image(cells_enc)
    image_loaded = _save_load_png(image)
    cells_dec = image_to_cells(image_loaded)
    assert cells_enc == cells_dec, "セル列の往復が一致しない"
    payload_back = core.decode(cells_dec)
    assert payload_back == payload, "ペイロード往復が一致しない"


def test_empty_payload():
    cells = core.encode(b"")
    image = cells_to_image(cells)
    cells_back = image_to_cells(_save_load_png(image))
    assert cells == cells_back
    assert core.decode(cells_back) == b""


def test_at_capacity():
    _, _, _, _, _, _, max_payload, _ = core.frame_spec()
    rng = random.Random(42)
    payload = bytes(rng.randint(0, 255) for _ in range(max_payload))
    cells = core.encode(payload)
    image = cells_to_image(cells)
    cells_back = image_to_cells(_save_load_png(image))
    assert cells == cells_back
    assert core.decode(cells_back) == payload


@pytest.mark.parametrize("size", [1, 100, 200, 500, 1000])
def test_various_sizes(size):
    rng = random.Random(size)
    payload = bytes(rng.randint(0, 255) for _ in range(size))
    cells = core.encode(payload)
    image = cells_to_image(cells)
    cells_back = image_to_cells(_save_load_png(image))
    assert core.decode(cells_back) == payload


def test_cli_roundtrip(tmp_path: Path):
    payload = bytes((i * 73) & 0xFF for i in range(200))
    inp = tmp_path / "input.bin"
    png = tmp_path / "frame.png"
    out = tmp_path / "output.bin"
    inp.write_bytes(payload)

    assert encode_main([str(inp), "-o", str(png)]) == 0
    assert png.exists()
    assert decode_main([str(png), "-o", str(out)]) == 0
    assert out.read_bytes() == payload
