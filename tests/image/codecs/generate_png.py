#!/usr/bin/env python3
"""Write the PNG and APNG fixtures for tests/image/codecs/png_test.dodo.

Each fixtures/png_NAME.png comes with fixtures/png_NAME.raw, the pixels the
decoder must emit in the header's native layout (16-bit samples
little-endian). APNG files also get png_NAME.fK.raw, frame K composited as
straight 16-bit RGBA with the same integer arithmetic as std/image/png.
Standard library only; the output is the same on every run.

    python3 tests/image/codecs/generate_png.py
"""
import os
import struct
import zlib

HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")

ADAM7 = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4),
         (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}

XMP = b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><dodo>image</dodo></x:xmpmeta>'
PROFILE = bytes((i * 7 + 3) & 255 for i in range(300))


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


def image_data(image, width, height, channels, depth, interlaced):
    """Filtered scanlines, filter types cycling so every one is used."""
    step = max(1, channels * depth // 8)
    passes = ADAM7 if interlaced else [(0, 0, 1, 1)]
    out = bytearray()
    counter = 0
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


def ihdr(width, height, depth, color, interlaced):
    return chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, 1 if interlaced else 0))


def png_file(width, height, depth, color, image, interlaced=False, before=b"", after=b""):
    data = image_data(image, width, height, CHANNELS[color], depth, interlaced)
    return (b"\x89PNG\r\n\x1a\n" + ihdr(width, height, depth, color, interlaced) + before
            + chunk(b"IDAT", zlib.compress(data, 9)) + after + chunk(b"IEND", b""))


def native(image, color, depth, palette, key, alpha):
    """The decoder's output bytes: native layout, little-endian samples."""
    out = bytearray()
    for row in image:
        for pixel in row:
            if color == 3:
                out += bytes(palette[pixel[0]])
                if alpha is not None:
                    out.append(alpha[pixel[0]] if pixel[0] < len(alpha) else 255)
                continue
            if depth < 8:
                out.append(pixel[0] * (255 // ((1 << depth) - 1)))
                if key is not None:
                    out.append(0 if pixel[0] == key[0] else 255)
                continue
            for v in pixel:
                out += bytes([v]) if depth == 8 else struct.pack("<H", v)
            if key is not None:
                transparent = tuple(pixel) == tuple(key)
                value = 0 if transparent else (1 << depth) - 1
                out += bytes([value]) if depth == 8 else struct.pack("<H", value)
    return bytes(out)


def write(name, data):
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)


def make(name, color, depth, width, height, interlaced=False, seed=1, transparency=False):
    channels = CHANNELS[color]
    image = samples(width, height, channels, depth, seed)
    before = b""
    palette = None
    alpha = None
    key = None
    if color == 3:
        count = min(1 << depth, 200)
        random = Random(seed + 99)
        palette = [(random.next() & 255, random.next() & 255, random.next() & 255) for _ in range(count)]
        image = [[(v[0] % count,) for v in row] for row in image]
        before += chunk(b"PLTE", bytes(c for entry in palette for c in entry))
        if transparency:
            alpha = [(i * 37) & 255 for i in range(count // 2 + 1)]
            before += chunk(b"tRNS", bytes(alpha))
    elif transparency:
        key = image[0][1]
        before += chunk(b"tRNS", b"".join(struct.pack(">H", v) for v in key))
    write(name + ".png", png_file(width, height, depth, color, image, interlaced, before))
    write(name + ".raw", native(image, color, depth, palette, key, alpha))


def rgb8_image(width, height, seed):
    return samples(width, height, 3, 8, seed)


def tiff_orientation(little, value):
    """A minimal EXIF payload: TIFF header and IFD0 with the orientation."""
    if little:
        return b"II*\x00" + struct.pack("<IH", 8, 1) + struct.pack("<HHIHH", 0x0112, 3, 1, value, 0) + struct.pack("<I", 0)
    return b"MM\x00*" + struct.pack(">IH", 8, 1) + struct.pack(">HHIHH", 0x0112, 3, 1, value, 0) + struct.pack(">I", 0)


def itxt_xmp(compressed):
    text = zlib.compress(XMP, 9) if compressed else XMP
    return chunk(b"iTXt", b"XML:com.adobe.xmp\x00" + bytes([1 if compressed else 0, 0]) + b"\x00\x00" + text)


def color_fixtures():
    image = rgb8_image(4, 3, 5)
    raw = native(image, 2, 8, None, None, None)
    cases = {
        # cICP wins over sRGB and gAMA.
        "png_color_cicp": chunk(b"cICP", bytes([9, 16, 0, 1])) + chunk(b"sRGB", b"\x01") + chunk(b"gAMA", struct.pack(">I", 45455)),
        # sRGB wins over gAMA.
        "png_color_srgb": chunk(b"gAMA", struct.pack(">I", 45455)) + chunk(b"sRGB", b"\x02"),
        "png_color_gama_chrm": chunk(b"gAMA", struct.pack(">I", 45455)) + chunk(b"cHRM", struct.pack(">8I", 31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000)),
        "png_color_iccp": chunk(b"iCCP", b"dodo test\x00\x00" + zlib.compress(PROFILE, 9)) + chunk(b"cLLi", struct.pack(">II", 10000000, 2000000)),
    }
    for name, before in cases.items():
        write(name + ".png", png_file(4, 3, 8, 2, image, False, before))
        write(name + ".raw", raw)
    # EXIF before the image data with compressed XMP; EXIF after it with
    # plain XMP.
    write("png_exif_before.png", png_file(4, 3, 8, 2, image, False, chunk(b"eXIf", tiff_orientation(True, 3)) + itxt_xmp(True)))
    write("png_exif_after.png", png_file(4, 3, 8, 2, image, False, itxt_xmp(False), chunk(b"eXIf", tiff_orientation(False, 6))))


# ---------------------------------------------------------------------------
# APNG

def frame_pixels(width, height, seed):
    """8-bit RGBA with opaque, translucent and transparent pixels."""
    random = Random(seed)
    image = []
    for y in range(height):
        row = []
        for x in range(width):
            v = random.next()
            alpha = [255, 128, 0, 200, 60][(x + y + seed) % 5]
            row.append((v & 255, (v >> 8) & 255, (v >> 16) & 255, alpha))
        image.append(row)
    return image


def over(dst, src):
    sa = src[3]
    if sa == 65535:
        return src
    if sa == 0:
        return dst
    da = dst[3]
    rest = (da * (65535 - sa) + 32767) // 65535
    total = sa + rest
    color = [min(65535, (src[k] * sa + dst[k] * rest + total // 2) // total) for k in range(3)]
    return (color[0], color[1], color[2], min(total, 65535))


def fctl(sequence, width, height, x, y, num, den, dispose, blend):
    return chunk(b"fcTL", struct.pack(">IIIIIHHBB", sequence, width, height, x, y, num, den, dispose, blend))


def compressed(image, width, height):
    return zlib.compress(image_data(image, width, height, 4, 8, False), 9)


def apng(name, width, height, frames, default_is_frame, default_image=None, plays=0, break_sequence=False):
    """frames: (x, y, w, h, num, den, dispose, blend, pixels)."""
    out = bytearray(b"\x89PNG\r\n\x1a\n" + ihdr(width, height, 8, 6, False))
    out += chunk(b"acTL", struct.pack(">II", len(frames), plays))
    sequence = 0
    for index, (x, y, w, h, num, den, dispose, blend, image) in enumerate(frames):
        if index == 0 and default_is_frame:
            out += fctl(sequence, w, h, x, y, num, den, dispose, blend)
            sequence += 1
            out += chunk(b"IDAT", compressed(image, w, h))
            continue
        if index == 0:
            out += chunk(b"IDAT", compressed(default_image, width, height))
        out += fctl(sequence, w, h, x, y, num, den, dispose, blend)
        sequence += 1
        data = compressed(image, w, h)
        # Split the frame's data across two fdAT chunks.
        half = len(data) // 2
        for part in (data[:half], data[half:]):
            number = sequence
            if break_sequence and index == 2:
                number += 5
            out += chunk(b"fdAT", struct.pack(">I", number) + part)
            sequence += 1
    out += chunk(b"IEND", b"")
    write(name + ".png", bytes(out))
    # The expected frames.
    canvas = [[(0, 0, 0, 0)] * width for _ in range(height)]
    for index, (x, y, w, h, num, den, dispose, blend, image) in enumerate(frames):
        if index == 0 and dispose == 2:
            dispose = 1
        saved = [row[:] for row in canvas]
        for r in range(h):
            for c in range(w):
                p = tuple(v * 257 for v in image[r][c])
                if blend == 1 and index > 0:
                    canvas[y + r][x + c] = over(canvas[y + r][x + c], p)
                else:
                    canvas[y + r][x + c] = p
        write("%s.f%d.raw" % (name, index), b"".join(struct.pack("<4H", *p) for row in canvas for p in row))
        if dispose == 1:
            for r in range(h):
                for c in range(w):
                    canvas[y + r][x + c] = (0, 0, 0, 0)
        elif dispose == 2:
            canvas = saved
    if default_image is not None:
        write(name + ".raw", bytes(v for row in default_image for p in row for v in p))
    else:
        write(name + ".raw", bytes(v for row in frames[0][8] for p in row for v in p))


def apng_fixtures():
    width, height = 6, 5
    frames = [
        (0, 0, 6, 5, 1, 10, 0, 0, frame_pixels(6, 5, 1)),
        (1, 1, 3, 2, 20, 0, 1, 1, frame_pixels(3, 2, 2)),
        (2, 0, 4, 3, 3, 4, 2, 1, frame_pixels(4, 3, 3)),
        (0, 2, 2, 3, 1, 1, 0, 1, frame_pixels(2, 3, 4)),
        (1, 0, 5, 5, 7, 100, 0, 0, frame_pixels(5, 5, 5)),
    ]
    apng("png_apng_default", width, height, frames, True, plays=3)
    hidden = [
        (0, 0, 6, 5, 2, 25, 2, 1, frame_pixels(6, 5, 6)),
        (2, 1, 3, 3, 0, 0, 0, 1, frame_pixels(3, 3, 7)),
    ]
    apng("png_apng_hidden", width, height, hidden, False, default_image=frame_pixels(6, 5, 8))
    apng("png_apng_badseq", width, height, frames[:3], True, break_sequence=True)


def main():
    os.makedirs(HERE, exist_ok=True)
    make("png_gray1", 0, 1, 13, 5)
    make("png_gray2_trns", 0, 2, 9, 4, transparency=True)
    make("png_gray4", 0, 4, 7, 3)
    make("png_gray8_trns", 0, 8, 5, 4, transparency=True)
    make("png_gray16", 0, 16, 9, 5, seed=2)
    make("png_gray16_trns", 0, 16, 9, 5, seed=3, transparency=True)
    make("png_gray16_adam7_trns", 0, 16, 11, 9, interlaced=True, seed=4, transparency=True)
    make("png_gray_alpha8", 4, 8, 6, 4)
    make("png_gray_alpha16", 4, 16, 6, 4, seed=5)
    make("png_rgb8_trns", 2, 8, 7, 3, seed=6, transparency=True)
    make("png_rgb16", 2, 16, 7, 6, seed=7)
    make("png_rgb16_trns", 2, 16, 7, 6, seed=8, transparency=True)
    make("png_rgba16", 6, 16, 5, 5, seed=9)
    make("png_rgba16_adam7", 6, 16, 10, 9, interlaced=True, seed=10)
    make("png_palette2", 3, 2, 11, 3, seed=11)
    make("png_palette4_trns", 3, 4, 9, 7, seed=12, transparency=True)
    make("png_palette8_adam7_trns", 3, 8, 12, 10, interlaced=True, seed=13, transparency=True)
    color_fixtures()
    apng_fixtures()


if __name__ == "__main__":
    main()
