#!/usr/bin/env python3
"""Generate the ICC profile fixtures of the JPEG XL encoder tests.

Writes tests/image/jxl_encode/icc/*.icc: small, self-made profiles that
exercise every part of the ICC stream coder (tag table groups, curv
prediction, para, mluc shuffling, sf32, XYZ shortcuts, mAB/mBA curves and
CLUT prediction):

  srgb-v2.icc   ICC v2 sRGB-like: desc/text, XYZ group, shared 1024-entry curv
  srgb-v4.icc   ICC v4 sRGB-like: mluc, chad (sf32), parametric curves
  p3-v4.icc     ICC v4 Display P3-like: mluc in two languages, para
  gray.icc      ICC v4 gray: kTRC curv with 256 entries
  lut-rgb.icc   ICC v4 RGB display profile with A2B0/B2A0 lutAToB/lutBToA
                (curves, matrix, 17^3 16-bit CLUT) beside the matrix/TRC tags;
                large enough for LZ77 distances beyond 16384
  quirks.icc    RGB profile with a gbd element of 14 data bytes (an
                order-0 prediction of a run that is not a multiple of 4) and
                a private tag whose size reaches past the end of the profile
                (libjxl round-trips such profiles)

The profiles are generated (no third-party data), deterministic, and
released with the tests. Usage: python3 tests/image/jxl_encode/make_icc.py
"""
import math
import os
import struct

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "icc")


def s15(v):
    return struct.pack(">i", int(round(v * 65536)))


def xyz(x, y, z):
    return b"XYZ " + b"\0" * 4 + s15(x) + s15(y) + s15(z)


def curv_table(values):
    return b"curv" + b"\0" * 4 + struct.pack(">I", len(values)) + b"".join(struct.pack(">H", v) for v in values)


def curv_gamma(g):
    return b"curv" + b"\0" * 4 + struct.pack(">I", 1) + struct.pack(">H", int(round(g * 256)))


def srgb_eotf(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def para_srgb():
    # Function type 3: Y = (aX + b)^g for X >= d, else cX.
    params = [2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045]
    return b"para" + b"\0" * 4 + struct.pack(">HH", 3, 0) + b"".join(s15(p) for p in params)


def desc_v2(text):
    t = text.encode() + b"\0"
    return b"desc" + b"\0" * 4 + struct.pack(">I", len(t)) + t + b"\0" * 8 + b"\0\0" + b"\0" + b"\0" * 67


def text_tag(text):
    return b"text" + b"\0" * 4 + text.encode() + b"\0"


def mluc(entries):
    body = b"mluc" + b"\0" * 4 + struct.pack(">II", len(entries), 12)
    offset = 16 + 12 * len(entries)
    records = b""
    strings = b""
    for lang, country, text in entries:
        s = text.encode("utf-16-be")
        records += lang.encode() + country.encode() + struct.pack(">II", len(s), offset + len(strings))
        strings += s
    return body + records + strings


def sf32(values):
    return b"sf32" + b"\0" * 4 + b"".join(s15(v) for v in values)


def profile(tags, color_space=b"RGB ", version=(4, 0x30), pcs=b"XYZ ", cls=b"mntr"):
    """Assemble a profile; `tags` is a list of (signature, body) where body
    may be a signature already listed (shared element)."""
    offset = 128 + 4 + 12 * len(tags)
    table = struct.pack(">I", len(tags))
    data = b""
    placed = {}
    for sig, body in tags:
        if isinstance(body, str):
            start, size = placed[body]
        else:
            while (offset + len(data)) % 4:
                data += b"\0"
            start, size = offset + len(data), len(body)
            data += body
            placed[sig.decode()] = (start, size)
        table += sig + struct.pack(">II", start, size)
    while (offset + len(data)) % 4:
        data += b"\0"
    size = 128 + len(table) + len(data)
    header = (struct.pack(">I", size) + b"dodo" + bytes([version[0], version[1], 0, 0]) + cls + color_space + pcs
              + struct.pack(">HHHHHH", 2026, 11, 1, 12, 0, 0) + b"acsp" + b"APPL" + b"\0" * 4 + b"\0" * 4
              + b"\0" * 4 + b"\0" * 8 + b"\0" * 4 + s15(0.9642) + s15(1.0) + s15(0.8249) + b"dodo"
              + b"\0" * 16 + b"\0" * 28)
    assert len(header) == 128
    return header + table + data


D50 = (0.9642, 1.0, 0.8249)
SRGB_XYZ = [(0.4361, 0.2225, 0.0139), (0.3851, 0.7169, 0.0971), (0.1431, 0.0606, 0.7141)]
P3_XYZ = [(0.5151, 0.2412, -0.0011), (0.2920, 0.6922, 0.0419), (0.1571, 0.0666, 0.7841)]
CHAD = [1.0479, 0.0229, -0.0502, 0.0296, 0.9904, -0.0171, -0.0092, 0.0151, 0.7519]


def srgb_v2():
    table = [int(round(srgb_eotf(i / 1023) * 65535)) for i in range(1024)]
    return profile([
        (b"desc", desc_v2("Dodo sRGB v2 test profile")),
        (b"cprt", text_tag("No copyright, use freely")),
        (b"wtpt", xyz(*D50)),
        (b"bkpt", xyz(0, 0, 0)),
        (b"rXYZ", xyz(*SRGB_XYZ[0])),
        (b"gXYZ", xyz(*SRGB_XYZ[1])),
        (b"bXYZ", xyz(*SRGB_XYZ[2])),
        (b"rTRC", curv_table(table)),
        (b"gTRC", "rTRC"),
        (b"bTRC", "rTRC"),
    ], version=(2, 0x10))


def srgb_v4():
    return profile([
        (b"desc", mluc([("en", "US", "Dodo sRGB v4")])),
        (b"cprt", mluc([("en", "US", "No copyright, use freely")])),
        (b"wtpt", xyz(*D50)),
        (b"chad", sf32(CHAD)),
        (b"rXYZ", xyz(*SRGB_XYZ[0])),
        (b"gXYZ", xyz(*SRGB_XYZ[1])),
        (b"bXYZ", xyz(*SRGB_XYZ[2])),
        (b"rTRC", para_srgb()),
        (b"gTRC", "rTRC"),
        (b"bTRC", "rTRC"),
    ])


def p3_v4():
    return profile([
        (b"desc", mluc([("en", "US", "Dodo Display P3"), ("de", "DE", "Dodo Display-P3-Testprofil")])),
        (b"cprt", mluc([("en", "US", "No copyright, use freely")])),
        (b"wtpt", xyz(*D50)),
        (b"rXYZ", xyz(*P3_XYZ[0])),
        (b"gXYZ", xyz(*P3_XYZ[1])),
        (b"bXYZ", xyz(*P3_XYZ[2])),
        (b"rTRC", para_srgb()),
        (b"chad", sf32(CHAD)),
        (b"bTRC", "rTRC"),
        (b"gTRC", "rTRC"),
    ])


def gray():
    table = [int(round(((i / 255) ** 2.2) * 65535)) for i in range(256)]
    return profile([
        (b"desc", mluc([("en", "US", "Dodo gray gamma 2.2")])),
        (b"cprt", mluc([("en", "US", "No copyright, use freely")])),
        (b"wtpt", xyz(*D50)),
        (b"kTRC", curv_table(table)),
    ], color_space=b"GRAY")


def pad4(b):
    return b + b"\0" * (-len(b) % 4)


def lut_a_to_b(sig, grid, transform):
    """lutAToBType / lutBToAType with B curves, matrix, M curves, CLUT and A
    curves (3 in, 3 out)."""
    curve = pad4(curv_table([int(round((i / 255) ** 1.8 * 65535)) for i in range(256)]))
    b_curves = curve * 3
    matrix = b"".join(s15(v) for v in [1.0, 0.01, 0.0, 0.0, 0.98, 0.02, 0.0, 0.0, 1.01, 0.0, 0.0, 0.0])
    m_curves = pad4(curv_gamma(1.0)) * 3
    clut = bytes([grid] * 3) + b"\0" * 13 + bytes([2, 0, 0, 0])
    for r in range(grid):
        for g in range(grid):
            for b in range(grid):
                out = transform(r / (grid - 1), g / (grid - 1), b / (grid - 1))
                clut += b"".join(struct.pack(">H", max(0, min(65535, int(round(v * 65535))))) for v in out)
    clut = pad4(clut)
    a_curves = pad4(curv_table([int(round((i / 63) ** 0.9 * 65535)) for i in range(64)])) * 3
    header_size = 32
    off_b = header_size
    off_m = off_b + len(b_curves)
    off_mc = off_m + len(matrix)
    off_clut = off_mc + len(m_curves)
    off_a = off_clut + len(clut)
    head = sig + b"\0" * 4 + bytes([3, 3, 0, 0]) + struct.pack(">IIIII", off_b, off_m, off_mc, off_clut, off_a)
    return head + b_curves + matrix + m_curves + clut + a_curves


def lut_rgb():
    def forward(r, g, b):
        rl, gl, bl = srgb_eotf(r), srgb_eotf(g), srgb_eotf(b)
        x = sum(c * m for c, m in zip((rl, gl, bl), (SRGB_XYZ[0][0], SRGB_XYZ[1][0], SRGB_XYZ[2][0])))
        y = sum(c * m for c, m in zip((rl, gl, bl), (SRGB_XYZ[0][1], SRGB_XYZ[1][1], SRGB_XYZ[2][1])))
        z = sum(c * m for c, m in zip((rl, gl, bl), (SRGB_XYZ[0][2], SRGB_XYZ[1][2], SRGB_XYZ[2][2])))
        return (x / 2, y / 2, z / 2)

    def backward(x, y, z):
        return (min(1, x * 1.1 + 0.02 * math.sin(6 * y)), min(1, y), min(1, z * 0.95))

    return profile([
        (b"desc", mluc([("en", "US", "Dodo LUT-based RGB")])),
        (b"cprt", mluc([("en", "US", "No copyright, use freely")])),
        (b"wtpt", xyz(*D50)),
        (b"rXYZ", xyz(*SRGB_XYZ[0])),
        (b"gXYZ", xyz(*SRGB_XYZ[1])),
        (b"bXYZ", xyz(*SRGB_XYZ[2])),
        (b"rTRC", para_srgb()),
        (b"gTRC", "rTRC"),
        (b"bTRC", "rTRC"),
        (b"A2B0", lut_a_to_b(b"mAB ", 17, forward)),
        (b"B2A0", lut_a_to_b(b"mBA ", 17, backward)),
    ])


def quirks():
    gbd = b"gbd " + b"\0" * 4 + bytes((i * 37 + 11) & 255 for i in range(14))
    data = profile([
        (b"desc", mluc([("en", "US", "Dodo quirks")])),
        (b"wtpt", xyz(*D50)),
        (b"rXYZ", xyz(*SRGB_XYZ[0])),
        (b"gXYZ", xyz(*SRGB_XYZ[1])),
        (b"bXYZ", xyz(*SRGB_XYZ[2])),
        (b"rTRC", para_srgb()),
        (b"gTRC", "rTRC"),
        (b"bTRC", "rTRC"),
        (b"gamt", gbd),
        (b"dodo", text_tag("private")),
    ])
    # The last tag entry ("dodo") claims 4096 bytes.
    count = struct.unpack(">I", data[128:132])[0]
    entry = 132 + 12 * (count - 1)
    return data[:entry + 8] + struct.pack(">I", 4096) + data[entry + 12:]


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, data in [("srgb-v2", srgb_v2()), ("srgb-v4", srgb_v4()), ("p3-v4", p3_v4()),
                       ("gray", gray()), ("lut-rgb", lut_rgb()), ("quirks", quirks())]:
        path = os.path.join(OUT, name + ".icc")
        open(path, "wb").write(data)
        print("%s: %d bytes" % (path, len(data)))


if __name__ == "__main__":
    main()
