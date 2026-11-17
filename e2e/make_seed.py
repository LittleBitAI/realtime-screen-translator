"""Writes e2e/seed.json: the API calls that fill an empty data folder with
two books, each holding one saved page, and leave translation on.

A page is a 384x768 screen (thumbnail 96x192, 4 screen pixels per thumbnail
pixel) drawn as panels with dark text-like lines. The first frame of a session
starts its picture, the second (still) one asks for the whole screen, which
the strip call then saves. Run: python -I e2e/make_seed.py
"""

import json
import struct
import zlib
from pathlib import Path

W, H, THUMB_W = 384, 768, 96
SCALE = W // THUMB_W
KEY = "seed_key_ABCDEFGHIJKLMNOP_wxyz"


def thumb_px(seed: int, col: int, row: int) -> int:
    # Panel gutters, then a few rows of "text" inside each panel.
    if col in (0, 47, 48, 95) or row % 64 in (0, 63):
        return 30
    line = (row % 64) // 6
    if 2 <= line <= 7 and (col * 7 + seed * 13 + line * 5) % 11 < 6:
        return 40 + (col * seed + row) % 40
    return 235 - (row * seed) % 15


def thumb(seed: int) -> list[int]:
    return [thumb_px(seed, c, r) for r in range(H // SCALE) for c in range(THUMB_W)]


def png(seed: int) -> list[int]:
    rows = b""
    for y in range(H):
        row = bytearray([0])
        for x in range(W):
            g = thumb_px(seed, x // SCALE, y // SCALE)
            row += bytes((g, g, g))
        rows += bytes(row)

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    head = struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
    return list(data)


def book(seed: int, picture_id: int, seq: int) -> list[dict]:
    t = thumb(seed)
    frame = lambda s: {"method": "POST", "path": "/frames", "body": {"seq": s, "width": W, "height": H, "thumb": t}}
    return [
        {"method": "POST", "path": "/translation/start", "body": None},
        frame(seq),
        frame(seq + 1),
        {
            "method": "POST",
            "path": "/strips",
            "body": {"picture_id": picture_id, "frame_seq": seq + 1, "top": 0, "bottom": H, "image": png(seed)},
        },
    ]


calls = [
    # The browser host cannot capture; the seed attaches a stand-in capturing
    # device and then pushes the frames and strips a phone would send.
    {"method": "POST", "path": "/test/simulated-device", "body": None},
    {"method": "PUT", "path": "/settings/gemini-key", "body": {"key": KEY}},
    {"method": "POST", "path": "/permissions/request", "body": None},
    *book(3, 1, 1),
    {"method": "POST", "path": "/translation/stop", "body": None},
    *book(5, 2, 1),
]

Path(__file__).with_name("seed.json").write_text(json.dumps(calls, separators=(",", ":")) + "\n", encoding="utf-8")
