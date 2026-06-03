"""CLI: バイナリファイルを Phase 0 PNG フレームに符号化する。

Usage:
    python -m beyond_qr_sender.encode input.bin -o frame.png
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import beyond_qr_core as core

from .render import cells_to_image


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="バイナリを Phase 0 PNG フレームに符号化する。"
    )
    parser.add_argument("input", type=Path, help="入力バイナリファイル")
    parser.add_argument(
        "-o", "--output", type=Path, required=True, help="出力 PNG パス"
    )
    args = parser.parse_args(argv)

    payload = args.input.read_bytes()
    cells = core.encode(payload)
    image = cells_to_image(cells)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    image.save(args.output, format="PNG")
    print(f"Encoded {len(payload)} byte → {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
