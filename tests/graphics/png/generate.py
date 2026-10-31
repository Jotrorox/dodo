#!/usr/bin/env python3
"""Write the PNG fixtures for tests/graphics/png_test.dodo.

Each NAME.png comes with NAME.qoi, the 8-bit RGBA image a decoder must
produce, so the test can compare them with the QOI decoder. bad_*.png files
are invalid in one specific way. Standard library only; the output is the
same on every run (zlib's output can differ between zlib versions, which only
changes the compressed bytes, not the images).

    python3 tests/graphics/png/generate.py
"""
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))

ADAM7 = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4),
         (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


class Random:
    """xorshift32, so the fixtures do not depend on Python's generator."""

    def __init__(self, seed):
        self.state = seed or 1

    def next(self):
        x = self.state
        x ^= (x << 13) & 0xffffffff
        x ^= x >> 17
        x ^= (x << 5) & 0xffffffff
        self.state = x
        return x


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xffffffff)


def samples(width, height, channels, depth, seed):
    """Per pixel sample tuples: gradients, flat areas, extremes and noise."""
    top = (1 << depth) - 1
    random = Random(seed * 7919 + 13)
    image = []
    for y in range(height):
        row = []
        for x in range(width):
            pixel = []
            for c in range(channels):
                kind = (x // 3 + y // 2 + c) % 4
                if kind == 0:
                    value = (x * top) // max(1, width - 1)
                elif kind == 1:
                    value = top if (x + y) % 2 else 0
                elif kind == 2:
                    value = random.next() % (top + 1)
                else:
                    value = (y * top) // max(1, height - 1)
                pixel.append(value)
            row.append(tuple(pixel))
        image.append(row)
    return image


def pack_row(row, depth):
    if depth == 8:
        return bytes(v for pixel in row for v in pixel)
    if depth == 16:
        return b"".join(struct.pack(">H", v) for pixel in row for v in pixel)
    out = bytearray()
    bits = 0
    count = 0
    for pixel in row:
        for v in pixel:
            bits = (bits << depth) | v
            count += depth
            if count == 8:
                out.append(bits)
                bits = 0
                count = 0
    if count:
        out.append(bits << (8 - count))
    return bytes(out)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def filter_row(kind, raw, previous, step):
    out = bytearray([kind])
    for i, x in enumerate(raw):
        a = raw[i - step] if i >= step else 0
        b = previous[i]
        c = previous[i - step] if i >= step else 0
        if kind == 0:
            value = x
        elif kind == 1:
            value = x - a
        elif kind == 2:
            value = x - b
        elif kind == 3:
            value = x - ((a + b) >> 1)
        else:
            value = x - paeth(a, b, c)
        out.append(value & 255)
    return bytes(out)


def image_data(image, width, height, channels, depth, interlaced, filter_seed):
    """Filtered scanlines, filter types cycling so every one is used."""
    step = max(1, channels * depth // 8)
    passes = ADAM7 if interlaced else [(0, 0, 1, 1)]
    out = bytearray()
    counter = filter_seed
    for x0, y0, dx, dy in passes:
        columns = range(x0, width, dx)
        rows = range(y0, height, dy)
        if not columns or not rows:
            continue
        previous = None
        for y in rows:
            raw = pack_row([image[y][x] for x in columns], depth)
            if previous is None:
                previous = bytes(len(raw))
            out += filter_row(counter % 5, raw, previous, step)
            counter += 1
            previous = raw
    return bytes(out)


def narrow(value, depth):
    if depth == 16:
        return (value * 255 + 32767) // 65535
    return value * 255 // ((1 << depth) - 1)


def expected(image, color, depth, palette, transparency):
    pixels = []
    for row in image:
        for pixel in row:
            if color == 3:
                r, g, b = palette[pixel[0]]
                a = transparency[pixel[0]] if transparency and pixel[0] < len(transparency) else 255
            elif color in (0, 4):
                r = g = b = narrow(pixel[0], depth)
                a = narrow(pixel[1], depth) if color == 4 else 255
                if color == 0 and transparency is not None and pixel[0] == transparency[0]:
                    a = 0
            else:
                r, g, b = (narrow(v, depth) for v in pixel[:3])
                a = narrow(pixel[3], depth) if color == 6 else 255
                if color == 2 and transparency is not None and tuple(pixel) == tuple(transparency):
                    a = 0
            pixels.append((r, g, b, a))
    return pixels


def qoi(width, height, pixels):
    out = bytearray(b"qoif" + struct.pack(">II", width, height) + bytes([4, 0]))
    index = [(0, 0, 0, 0)] * 64
    previous = (0, 0, 0, 255)
    run = 0
    for n, pixel in enumerate(pixels):
        if pixel == previous:
            run += 1
            if run == 62 or n == len(pixels) - 1:
                out.append(0xc0 | (run - 1))
                run = 0
            continue
        if run:
            out.append(0xc0 | (run - 1))
            run = 0
        r, g, b, a = pixel
        slot = (r * 3 + g * 5 + b * 7 + a * 11) % 64
        if index[slot] == pixel:
            out.append(slot)
        else:
            index[slot] = pixel
            if a == previous[3]:
                dr = (r - previous[0] + 128) % 256 - 128
                dg = (g - previous[1] + 128) % 256 - 128
                db = (b - previous[2] + 128) % 256 - 128
                if -2 <= dr <= 1 and -2 <= dg <= 1 and -2 <= db <= 1:
                    out.append(0x40 | (dr + 2) << 4 | (dg + 2) << 2 | (db + 2))
                elif -32 <= dg <= 31 and -8 <= dr - dg <= 7 and -8 <= db - dg <= 7:
                    out += bytes([0x80 | (dg + 32), (dr - dg + 8) << 4 | (db - dg + 8)])
                else:
                    out += bytes([0xfe, r, g, b])
            else:
                out += bytes([0xff, r, g, b, a])
        previous = pixel
    return bytes(out + bytes([0] * 7 + [1]))


def make(name, color, depth, width, height, interlaced=False, seed=1,
         palette_size=None, transparency=None, split=None, level=9, window=15,
         before=(), after=(), trailing=b"", ihdr=None, idat=None,
         palette_override=None, end=True, write_expected=True):
    channels = CHANNELS[color]
    palette = None
    if color == 3:
        size = palette_size or (1 << depth)
        palette = [((i * 67 + 11) % 256, (i * 151 + 3) % 256, (255 - i * 29) % 256) for i in range(size)]
        image = samples(width, height, 1, depth, seed)
        image = [[(p[0] % size,) for p in row] for row in image]
    else:
        image = samples(width, height, channels, depth, seed)
    if transparency is not None and color in (0, 2):
        # Make sure some pixels match the transparent color.
        for y in range(height):
            for x in range(width):
                if (x * 3 + y) % 5 == 0:
                    image[y][x] = tuple(transparency)
    raw = image_data(image, width, height, channels, depth, interlaced, seed)
    compressor = zlib.compressobj(level, zlib.DEFLATED, window)
    stream = compressor.compress(raw) + compressor.flush()
    if idat is not None:
        stream = idat(stream)
    header = struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, 1 if interlaced else 0)
    if ihdr is not None:
        header = ihdr(header)
    out = bytearray(b"\x89PNG\r\n\x1a\n")
    out += chunk(b"IHDR", header)
    for kind, data in before:
        out += chunk(kind, data)
    if palette_override is not None:
        out += palette_override
    elif palette is not None:
        out += chunk(b"PLTE", bytes(v for entry in palette for v in entry))
    if transparency is not None:
        if color == 0:
            data = struct.pack(">H", transparency[0])
        elif color == 2:
            data = struct.pack(">HHH", *transparency)
        else:
            data = bytes(transparency)
        out += chunk(b"tRNS", data)
    if split is None:
        out += chunk(b"IDAT", stream)
    else:
        sizes = split
        position = 0
        n = 0
        while position < len(stream):
            size = sizes[n % len(sizes)]
            out += chunk(b"IDAT", stream[position:position + size])
            position += size
            n += 1
    for kind, data in after:
        out += chunk(kind, data)
    if end:
        out += chunk(b"IEND", b"")
    out += trailing
    with open(os.path.join(HERE, name + ".png"), "wb") as f:
        f.write(out)
    if write_expected:
        pixels = expected(image, color, depth, palette, transparency)
        with open(os.path.join(HERE, name + ".qoi"), "wb") as f:
            f.write(qoi(width, height, pixels))


def main():
    for name in os.listdir(HERE):
        if name.endswith(".png") or name.endswith(".qoi"):
            os.remove(os.path.join(HERE, name))
    # Every color type and bit depth.
    make("gray1", 0, 1, 13, 7, seed=1)
    make("gray2", 0, 2, 13, 7, seed=2)
    make("gray4", 0, 4, 13, 7, seed=3)
    make("gray8", 0, 8, 17, 9, seed=4)
    make("gray16", 0, 16, 9, 5, seed=5)
    make("rgb8", 2, 8, 13, 7, seed=6)
    make("rgb16", 2, 16, 7, 5, seed=7)
    make("palette1", 3, 1, 13, 7, seed=8)
    make("palette2", 3, 2, 11, 6, seed=9, palette_size=3)
    make("palette4", 3, 4, 13, 7, seed=10)
    make("palette8", 3, 8, 17, 9, seed=11, palette_size=200)
    make("gray_alpha8", 4, 8, 13, 7, seed=12)
    make("gray_alpha16", 4, 16, 7, 5, seed=13)
    make("rgba8", 6, 8, 13, 7, seed=14)
    make("rgba16", 6, 16, 7, 5, seed=15)
    # Odd sizes: one pixel, and sub-byte rows that end mid-byte.
    make("gray1_1x1", 0, 1, 1, 1, seed=16)
    make("rgba8_1x1", 6, 8, 1, 1, seed=17)
    make("gray1_wide", 0, 1, 33, 3, seed=18)
    make("gray2_odd", 0, 2, 5, 4, seed=19)
    make("palette4_odd", 3, 4, 3, 5, seed=20)
    # Transparency for palette, gray and RGB images.
    make("palette4_trns", 3, 4, 13, 7, seed=21, transparency=[0, 64, 128, 200, 255, 10])
    make("palette8_trns", 3, 8, 9, 9, seed=22, palette_size=40, transparency=list(range(0, 240, 6)))
    make("gray2_trns", 0, 2, 9, 5, seed=23, transparency=[1])
    make("gray8_trns", 0, 8, 9, 5, seed=24, transparency=[255])
    make("gray16_trns", 0, 16, 9, 5, seed=25, transparency=[40000])
    make("rgb8_trns", 2, 8, 9, 5, seed=26, transparency=[10, 20, 30])
    make("rgb16_trns", 2, 16, 9, 5, seed=27, transparency=[1000, 65535, 0])
    # Adam7 interlacing, including images too small for some passes.
    make("gray1_adam7", 0, 1, 13, 7, interlaced=True, seed=28)
    make("gray2_adam7", 0, 2, 3, 9, interlaced=True, seed=29)
    make("gray8_adam7_1x1", 0, 8, 1, 1, interlaced=True, seed=30)
    make("rgb8_adam7", 2, 8, 13, 7, interlaced=True, seed=31)
    make("palette4_adam7", 3, 4, 10, 10, interlaced=True, seed=32)
    make("gray_alpha8_adam7", 4, 8, 5, 3, interlaced=True, seed=33)
    make("rgba16_adam7", 6, 16, 9, 9, interlaced=True, seed=34)
    make("rgb16_trns_adam7", 2, 16, 8, 8, interlaced=True, seed=35, transparency=[0, 0, 0])
    # Image data split over many IDAT chunks, some empty or one byte long.
    make("rgba8_split", 6, 8, 13, 7, seed=36, split=[7, 0, 1, 30])
    make("gray4_split", 0, 4, 13, 7, seed=37, split=[1])
    make("palette8_adam7_split", 3, 8, 11, 7, interlaced=True, seed=38, split=[5, 0])
    # Stored deflate blocks, and a small deflate window.
    make("rgb8_stored", 2, 8, 13, 7, seed=39, level=0)
    make("gray8_window", 0, 8, 40, 20, seed=40, window=9)
    # Ancillary chunks before and after the image data are skipped, a
    # suggested palette in an RGB image is ignored, and so is anything after
    # IEND.
    make("rgb8_ancillary", 2, 8, 13, 7, seed=41,
         before=[(b"gAMA", struct.pack(">I", 45455)), (b"tEXt", b"Comment\x00made by generate.py"),
                 (b"PLTE", bytes([1, 2, 3, 4, 5, 6])), (b"prVt", b"\x00\x01")],
         after=[(b"tEXt", b"After\x00image data"), (b"zzZZ", b"")],
         trailing=b"trailing garbage")
    make("rgba8_no_iend", 6, 8, 5, 4, seed=42, end=False)

    # Invalid files.
    def bad_crc(path):
        with open(path, "rb") as f:
            data = bytearray(f.read())
        data[33 + 8 + 3] ^= 0x10
        with open(path, "wb") as f:
            f.write(data)

    def cut(path, length):
        with open(path, "rb") as f:
            data = f.read()
        with open(path, "wb") as f:
            f.write(data[:length])

    make("bad_crc", 6, 8, 5, 4, seed=43, write_expected=False)
    bad_crc(os.path.join(HERE, "bad_crc.png"))
    make("bad_truncated", 6, 8, 5, 4, seed=44, level=0, write_expected=False)
    cut(os.path.join(HERE, "bad_truncated.png"), 60)
    make("bad_zlib_short", 2, 8, 6, 5, seed=45, level=0, idat=lambda s: s[:40], write_expected=False)
    make("bad_critical", 2, 8, 4, 4, seed=46, before=[(b"CRIT", b"\x01")], write_expected=False)
    make("bad_ihdr_depth", 2, 8, 4, 4, seed=47, ihdr=lambda h: h[:8] + bytes([4]) + h[9:], write_expected=False)
    make("bad_ihdr_zero", 0, 8, 4, 4, seed=48, ihdr=lambda h: struct.pack(">I", 0) + h[4:], write_expected=False)
    make("bad_ihdr_interlace", 0, 8, 4, 4, seed=49, ihdr=lambda h: h[:12] + bytes([2]), write_expected=False)

    def bad_filter(stream):
        raw = bytearray(zlib.decompress(stream))
        raw[0] = 5
        return zlib.compress(bytes(raw), 9)

    make("bad_filter", 0, 8, 4, 4, seed=50, idat=bad_filter, write_expected=False)

    def bad_adler(stream):
        return stream[:-1] + bytes([stream[-1] ^ 1])

    make("bad_adler", 0, 8, 4, 4, seed=51, idat=bad_adler, write_expected=False)
    make("bad_index", 3, 4, 4, 4, seed=52, palette_override=chunk(b"PLTE", bytes([9, 9, 9])), write_expected=False)
    make("bad_no_palette", 3, 4, 4, 4, seed=53, palette_override=b"", write_expected=False)
    make("bad_palette_gray", 0, 8, 4, 4, seed=54, before=[(b"PLTE", bytes([1, 2, 3]))], write_expected=False)
    make("bad_trns_length", 2, 8, 4, 4, seed=55, before=[(b"tRNS", bytes([1, 2]))], write_expected=False)


if __name__ == "__main__":
    main()
