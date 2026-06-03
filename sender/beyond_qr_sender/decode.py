"""CLI: Phase 0 PNG フレームを復号してバイナリを取り出す。

Usage:
    python -m beyond_qr_sender.decode frame.png -o output.bin
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from PIL import Image

import beyond_qr_core as core

from .render import image_to_cells


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Phase 0 PNG フレームを復号する。"
    )
    parser.add_argument("input", type=Path, help="入力 PNG ファイル")
    parser.add_argument(
        "-o", "--output", type=Path, required=True, help="出力バイナリパス"
    )
    args = parser.parse_args(argv)

    image = Image.open(args.input)
    cells = image_to_cells(image)
    payload = core.decode(cells)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(payload)
    print(f"Decoded {args.output} ({len(payload)} byte)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
