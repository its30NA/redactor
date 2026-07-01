"""Generate the redactor app icon (PNG + ICO) with the standard library only.

No Pillow, no design tools — we rasterize a navy rounded tile with a gold padlock,
supersampled for smooth edges, then hand-encode a PNG and wrap it in an ICO. Run:

    python assets/make_icon.py

Outputs assets/redactor.png and assets/redactor.ico.
"""

from __future__ import annotations

import struct
import zlib
from pathlib import Path

SS = 3          # supersample factor (render big, shrink down = anti-aliasing)
S = 256         # final icon size
H = S * SS

NAVY = (17, 23, 38, 255)     # tile background (matches the web UI)
GOLD = (242, 200, 108, 255)  # the padlock

buf = bytearray(H * H * 4)   # RGBA, starts fully transparent


def _R(v: float) -> int:
    return int(round(v * SS))


def _put(x: int, y: int, c: tuple[int, int, int, int]) -> None:
    if 0 <= x < H and 0 <= y < H:
        i = (y * H + x) * 4
        buf[i], buf[i + 1], buf[i + 2], buf[i + 3] = c


def fill_rounded(x0, y0, x1, y1, r, c) -> None:
    X0, Y0, X1, Y1, Rr = _R(x0), _R(y0), _R(x1), _R(y1), _R(r)
    for y in range(Y0, Y1):
        for x in range(X0, X1):
            ccx = X0 + Rr if x < X0 + Rr else (X1 - 1 - Rr if x > X1 - 1 - Rr else x)
            ccy = Y0 + Rr if y < Y0 + Rr else (Y1 - 1 - Rr if y > Y1 - 1 - Rr else y)
            if (x - ccx) ** 2 + (y - ccy) ** 2 <= Rr * Rr:
                _put(x, y, c)


def fill_ring(cx, cy, ro, ri, c, ytop=1e9) -> None:
    CX, CY, RO, RI, YT = _R(cx), _R(cy), _R(ro), _R(ri), _R(ytop)
    for y in range(CY - RO, CY + RO + 1):
        if y > YT:
            continue
        for x in range(CX - RO, CX + RO + 1):
            d = (x - CX) ** 2 + (y - CY) ** 2
            if RI * RI <= d <= RO * RO:
                _put(x, y, c)


def fill_disc(cx, cy, rad, c) -> None:
    CX, CY, RR = _R(cx), _R(cy), _R(rad)
    for y in range(CY - RR, CY + RR + 1):
        for x in range(CX - RR, CX + RR + 1):
            if (x - CX) ** 2 + (y - CY) ** 2 <= RR * RR:
                _put(x, y, c)


def draw() -> None:
    fill_rounded(8, 8, 248, 248, 46, NAVY)          # tile
    fill_ring(128, 104, 40, 25, GOLD, ytop=104)     # shackle (top arc)
    fill_rounded(88, 100, 103, 126, 2, GOLD)        # left leg
    fill_rounded(153, 100, 168, 126, 2, GOLD)       # right leg
    fill_rounded(72, 118, 184, 202, 20, GOLD)       # lock body
    fill_disc(128, 150, 12, NAVY)                   # keyhole (round)
    fill_rounded(122, 150, 134, 180, 4, NAVY)       # keyhole (stem)


def downsample() -> bytearray:
    out = bytearray(S * S * 4)
    n = SS * SS
    for oy in range(S):
        for ox in range(S):
            r = g = b = a = 0
            for dy in range(SS):
                base = ((oy * SS + dy) * H + ox * SS) * 4
                for dx in range(SS):
                    i = base + dx * 4
                    r += buf[i]
                    g += buf[i + 1]
                    b += buf[i + 2]
                    a += buf[i + 3]
            j = (oy * S + ox) * 4
            out[j], out[j + 1], out[j + 2], out[j + 3] = r // n, g // n, b // n, a // n
    return out


def encode_png(width: int, height: int, rgba: bytes) -> bytes:
    stride = width * 4
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter byte: none
        raw += rgba[y * stride : (y + 1) * stride]

    def chunk(typ: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + typ + data
            + struct.pack(">I", zlib.crc32(typ + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def wrap_ico(png: bytes, size: int) -> bytes:
    dim = 0 if size >= 256 else size  # 0 means 256 in the ICO spec
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), 6 + 16)
    return header + entry + png


def main() -> None:
    draw()
    rgba = bytes(downsample())
    png = encode_png(S, S, rgba)
    here = Path(__file__).parent
    (here / "redactor.png").write_bytes(png)
    (here / "redactor.ico").write_bytes(wrap_ico(png, S))
    print(f"wrote {here/'redactor.png'} and {here/'redactor.ico'}")


if __name__ == "__main__":
    main()
