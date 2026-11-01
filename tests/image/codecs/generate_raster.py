#!/usr/bin/env python3
"""Write the PPM/PAM/PFM, QOI and BMP fixtures for tests/image/codecs.

Each NAME.{ppm,pam,pfm,qoi,bmp} in fixtures/ comes with NAME.raw: the pixels
a decoder must emit in the file's native layout (top-down rows, tightly
packed, little-endian samples). Run from anywhere with Python 3 (stdlib only):

    python3 tests/image/codecs/generate_raster.py
"""
import os
import struct

HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")


def write(name, data, raw):
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)
    base = name.rsplit(".", 1)[0]
    with open(os.path.join(HERE, base + ".raw"), "wb") as f:
        f.write(bytes(raw))


class Random:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = (self.state * 1103515245 + 12345) & 0x7FFFFFFF
        return self.state >> 8


def le16(v):
    return struct.pack("<H", v)


# -- Netpbm -------------------------------------------------------------------


def scale8(v, maxval):
    if v >= maxval:
        return 255
    return (v * 255 + maxval // 2) // maxval


def scale16(v, maxval):
    if v >= maxval:
        return 65535
    return (v * 65535 + maxval // 2) // maxval


def netpbm(name, magic, width, height, channels, maxval, samples, header=None):
    if header is None:
        header = b"%s\n%d %d\n%d\n" % (magic, width, height, maxval)
    body = bytearray()
    raw = bytearray()
    for v in samples:
        if maxval > 255:
            body += struct.pack(">H", v)
            raw += le16(scale16(v, maxval))
        else:
            body.append(v)
            raw.append(scale8(v, maxval))
    write(name, header + bytes(body), raw)


def pam(name, width, height, depth, maxval, tupltype, samples, comment=False):
    lines = [b"P7"]
    if comment:
        lines.append(b"# written by generate_raster.py")
    lines += [b"WIDTH %d" % width, b"HEIGHT %d" % height, b"DEPTH %d" % depth,
              b"MAXVAL %d" % maxval]
    if tupltype:
        lines.append(b"TUPLTYPE " + tupltype)
    lines.append(b"ENDHDR")
    header = b"\n".join(lines) + b"\n"
    netpbm(name, b"P7", width, height, depth, maxval, samples, header)


def pfm(name, width, height, channels, values, little):
    magic = b"PF" if channels == 3 else b"Pf"
    scale = b"-1.0" if little else b"1.0"
    header = b"%s\n%d %d\n%s\n" % (magic, width, height, scale)
    fmt = "<f" if little else ">f"
    row = width * channels
    body = bytearray()
    for y in range(height - 1, -1, -1):
        for v in values[y * row:(y + 1) * row]:
            body += struct.pack(fmt, v)
    raw = bytearray()
    for v in values:
        raw += struct.pack("<f", v)
    write(name, header + bytes(body), raw)


# -- QOI ----------------------------------------------------------------------


def qoi(name, width, height, pixels, channels, colorspace):
    """A plain QOI encoder (RGB/RGBA/run ops only) and its native output."""
    out = bytearray(b"qoif" + struct.pack(">II", width, height) + bytes([channels, colorspace]))
    previous = (0, 0, 0, 255)
    run = 0
    for i, p in enumerate(pixels):
        if p == previous:
            run += 1
            if run == 62 or i == len(pixels) - 1:
                out.append(0xC0 | (run - 1))
                run = 0
            continue
        if run:
            out.append(0xC0 | (run - 1))
            run = 0
        if p[3] == previous[3]:
            out += bytes([0xFE, p[0], p[1], p[2]])
        else:
            out += bytes([0xFF, p[0], p[1], p[2], p[3]])
        previous = p
    out += bytes([0] * 7 + [1])
    raw = bytearray()
    for p in pixels:
        raw += bytes(p[:channels])
    write(name, bytes(out), raw)


# -- BMP ----------------------------------------------------------------------


def bmp_file(header, palette, pixels, extra=b""):
    offset = 14 + len(header) + len(palette)
    total = offset + len(pixels) + len(extra)
    return b"BM" + struct.pack("<IHHI", total, 0, 0, offset) + header + palette + pixels + extra


def info_header(width, height, bits, compression, image_size, colors=0, size=40, extra=b""):
    head = struct.pack("<IiiHHIIiiII", size, width, height, 1, bits, compression,
                       image_size, 2835, 2835, colors, 0)
    head += extra
    assert len(head) == size, (len(head), size)
    return head


def rle_expected(width, height, stream, palette, four):
    """Decode an RLE8/RLE4 stream independently: skipped pixels are black."""
    image = [[(0, 0, 0)] * width for _ in range(height)]
    x = 0
    row = 0
    i = 0

    def put(index):
        nonlocal x
        if x < width and row < height:
            image[height - 1 - row][x] = palette[index]
            x += 1

    while row < height:
        count, value = stream[i], stream[i + 1]
        i += 2
        if count:
            for k in range(count):
                if x >= width:
                    break
                if four:
                    put(value >> 4 if k % 2 == 0 else value & 15)
                else:
                    put(value)
        elif value == 0:
            x = 0
            row += 1
        elif value == 1:
            break
        elif value == 2:
            dx, dy = stream[i], stream[i + 1]
            i += 2
            x = min(x + dx, width)
            row += dy
        else:
            n = value
            nbytes = (n + 1) // 2 if four else n
            data = stream[i:i + nbytes]
            for k in range(n):
                if four:
                    b = data[k // 2]
                    put(b >> 4 if k % 2 == 0 else b & 15)
                else:
                    put(data[k])
            i += (nbytes + 1) // 2 * 2
    raw = bytearray()
    for line in image:
        for p in line:
            raw += bytes(p)
    return raw


def bmp_rle(name, width, height, palette, stream, four):
    pal = b"".join(bytes([b, g, r, 0]) for (r, g, b) in palette)
    header = info_header(width, height, 4 if four else 8, 2 if four else 1, len(stream), len(palette))
    write(name, bmp_file(header, pal, bytes(stream)), rle_expected(width, height, stream, palette, four))


def main():
    os.makedirs(HERE, exist_ok=True)
    rnd = Random(7)

    # PGM maxval 15 with a saturating sample, and a comment.
    samples = [0, 1, 7, 8, 15, 20, 3, 14, 9, 2, 11, 5]
    netpbm("p5_maxval15.ppm", b"P5", 4, 3, 1, 15, samples,
           b"P5\n# comment\n4 3\n15\n")
    # PPM with maxval 1000: 16-bit samples scaled to 65535.
    samples = [rnd.next() % 1001 for _ in range(3 * 2 * 3)]
    samples[4] = 1200
    netpbm("p6_maxval1000.ppm", b"P6", 3, 2, 3, 1000, samples)
    # Full 16-bit PPM and PGM: values kept exactly.
    netpbm("p6_16.ppm", b"P6", 5, 3, 3, 65535, [rnd.next() & 0xFFFF for _ in range(5 * 3 * 3)])
    netpbm("p5_16.ppm", b"P5", 3, 3, 1, 65535, [rnd.next() & 0xFFFF for _ in range(9)])
    netpbm("p6_8.ppm", b"P6", 4, 2, 3, 255, [rnd.next() & 255 for _ in range(4 * 2 * 3)])

    # PAM, every tuple type.
    pam("pam_gray.pam", 3, 2, 1, 255, b"GRAYSCALE", [rnd.next() & 255 for _ in range(6)], comment=True)
    pam("pam_gray_alpha16.pam", 3, 2, 2, 65535, b"GRAYSCALE_ALPHA", [rnd.next() & 0xFFFF for _ in range(12)])
    pam("pam_rgb.pam", 2, 2, 3, 255, b"RGB", [rnd.next() & 255 for _ in range(12)])
    pam("pam_rgba.pam", 2, 3, 4, 255, b"RGB_ALPHA", [rnd.next() & 255 for _ in range(24)])
    pam("pam_rgba16.pam", 2, 2, 4, 65535, b"RGB_ALPHA", [rnd.next() & 0xFFFF for _ in range(16)])
    pam("pam_bw.pam", 4, 2, 1, 1, b"BLACKANDWHITE", [0, 1, 1, 0, 1, 0, 0, 1])
    pam("pam_bw_alpha.pam", 2, 1, 2, 1, b"BLACKANDWHITE_ALPHA", [1, 1, 0, 1])
    pam("pam_notype.pam", 2, 1, 4, 255, None, [1, 2, 3, 4, 5, 6, 7, 8])
    pam("pam_maxval100.pam", 2, 1, 3, 100, b"RGB", [0, 50, 100, 25, 75, 101])

    # PFM: negative, HDR and tiny values; little-endian RGB, big-endian gray.
    values = [0.0, 0.25, 1.0, -0.5, 2.5, 1e10, 0.125, 0.75, 3.0e-5,
              1.0, 0.0, -2.0, 0.5, 0.5, 0.5, 65504.0, 1.5, 0.1]
    pfm("pfm_rgb.pfm", 3, 2, 3, values, little=True)
    pfm("pfm_gray_be.pfm", 2, 3, 1, [0.0, 0.5, 1.0, -1.0, 4.0, 0.2], little=False)

    # QOI: linear colorspace with 3 channels, and RGBA with runs.
    pixels = [(rnd.next() & 255, rnd.next() & 255, rnd.next() & 255, 255) for _ in range(6)]
    pixels += [pixels[-1]] * 3
    qoi("qoi_linear.qoi", 3, 3, pixels, 3, 1)
    pixels = []
    for i in range(100):
        p = (i * 3 & 255, 200, i & 255, 255 if i % 5 else 128)
        pixels += [p] * (1 + i % 3)
    pixels = pixels[:10 * 14]
    qoi("qoi_rgba.qoi", 10, 14, pixels, 4, 0)

    # BMP RLE8 with run, absolute, end-of-line, delta and end-of-bitmap.
    palette = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]
    stream = [3, 1, 0, 3, 2, 3, 0, 0, 0, 0,          # row 0: run, absolute (padded), EOL
              0, 2, 2, 1, 2, 3, 0, 0,                # delta (2, 1), run, EOL
              0, 4, 1, 2, 3, 1, 0, 1]                # row 3: absolute 4, EOB
    bmp_rle("rle8.bmp", 8, 5, palette, stream, False)
    palette16 = [((i * 17) & 255, (255 - i * 13) & 255, (i * 40) & 255) for i in range(16)]
    stream = [5, 0x12, 0, 0,                     # row 0: 1 2 1 2 1, EOL
              0, 3, 0x34, 0x50, 0, 2, 1, 0,      # absolute 3 (padded to 2 bytes), delta (1, 0)
              2, 0xff, 0, 0,                     # run, EOL
              0, 2, 0, 1,                        # delta (0, 1): skip a row
              7, 0xab, 0, 1]                     # row 3: 7 alternating, EOB
    bmp_rle("rle4.bmp", 7, 4, palette16, stream, True)

    # OS/2 BITMAPCOREHEADER, 8-bit with 3-byte palette entries.
    width, height = 5, 3
    palette = [((i * 30) & 255, (i * 70) & 255, (255 - i * 20) & 255) for i in range(6)]
    core = struct.pack("<IHHHH", 12, width, height, 1, 8)
    pal = b"".join(bytes([b, g, r]) for (r, g, b) in palette)
    stride = (width + 3) // 4 * 4
    indices = [[(x + y * 2) % 6 for x in range(width)] for y in range(height)]
    body = bytearray()
    for y in range(height - 1, -1, -1):
        body += bytes(indices[y]) + bytes(stride - width)
    raw = bytearray()
    for y in range(height):
        for i in indices[y]:
            raw += bytes(palette[i])
    write("os2_core8.bmp", bmp_file(core, pal, bytes(body)), raw)
    # OS/2 24-bit.
    rgb = [[(rnd.next() & 255, rnd.next() & 255, rnd.next() & 255) for _ in range(3)] for _ in range(2)]
    core = struct.pack("<IHHHH", 12, 3, 2, 1, 24)
    body = bytearray()
    for y in range(1, -1, -1):
        for (r, g, b) in rgb[y]:
            body += bytes([b, g, r])
        body += bytes(3)
    raw = b"".join(bytes(p) for line in rgb for p in line)
    write("os2_core24.bmp", bmp_file(core, b"", bytes(body)), raw)

    # V5 header with an embedded ICC profile after the pixels, 24-bit.
    width, height = 3, 2
    rgb = [[(rnd.next() & 255, rnd.next() & 255, rnd.next() & 255) for _ in range(width)] for _ in range(height)]
    body = bytearray()
    for y in range(height - 1, -1, -1):
        for (r, g, b) in rgb[y]:
            body += bytes([b, g, r])
        body += bytes(3)
    profile = b"FAKEICCPROFILE" + bytes(range(50))
    profile_data = 124 + len(body)   # relative to the header start
    extra = struct.pack("<IIII", 0, 0, 0, 0) + struct.pack("<I", 0x4D424544) + bytes(36) + bytes(12) \
        + struct.pack("<IIII", 4, profile_data, len(profile), 0)
    header = info_header(width, height, 24, 0, len(body), size=124, extra=extra)
    raw = b"".join(bytes(p) for line in rgb for p in line)
    write("v5_icc.bmp", bmp_file(header, b"", bytes(body), profile), raw)

    # V5 sRGB, 32-bit with an alpha mask, top-down.
    width, height = 2, 2
    rgba = [[(rnd.next() & 255, rnd.next() & 255, rnd.next() & 255, rnd.next() & 255) for _ in range(width)] for _ in range(height)]
    body = bytearray()
    for y in range(height):
        for (r, g, b, a) in rgba[y]:
            body += bytes([b, g, r, a])
    extra = struct.pack("<IIII", 0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000) + struct.pack("<I", 0x73524742) \
        + bytes(36) + bytes(12) + struct.pack("<IIII", 4, 0, 0, 0)
    header = info_header(width, -height, 32, 3, len(body), size=124, extra=extra)
    raw = b"".join(bytes(p) for line in rgba for p in line)
    write("v5_srgb_alpha.bmp", bmp_file(header, b"", bytes(body)), raw)

    # V4 header (108 bytes), 16-bit RGB565 bit fields.
    width, height = 3, 2
    colors = [[(rnd.next() & 31, rnd.next() & 63, rnd.next() & 31) for _ in range(width)] for _ in range(height)]
    body = bytearray()
    for y in range(height - 1, -1, -1):
        for (r, g, b) in colors[y]:
            body += le16((r << 11) | (g << 5) | b)
        body += bytes(2)
    extra = struct.pack("<IIII", 0xF800, 0x07E0, 0x001F, 0) + struct.pack("<I", 0) + bytes(36) + bytes(12)
    header = info_header(width, height, 16, 3, len(body), size=108, extra=extra)
    raw = bytearray()
    for line in colors:
        for (r, g, b) in line:
            raw += bytes([(r * 255 + 15) // 31, (g * 255 + 31) // 63, (b * 255 + 15) // 31])
    write("v4_565.bmp", bmp_file(header, b"", bytes(body)), raw)


if __name__ == "__main__":
    main()
