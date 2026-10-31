#!/usr/bin/env python3
"""Writes the TrueType fixtures std/graphics/truetype is tested against.

The glyphs are drawn here, so the fonts carry no third-party licence. Each
outline exercises one part of the format: polygons with holes (.notdef, A,
N), a contour of off-curve points only (O), a contour starting off-curve
with an on-curve last point (D), and composite glyphs with offsets (Ä), a
uniform scale (o) and a rotation matrix (Z).

    dodo-test.ttf       short loca, cmap format 4, kern table
    dodo-test-long.ttf  long loca, cmap formats 4 and 12, no kern table
    dodo-test.ttc       both fonts as a TrueType collection

Run from anywhere; the output is deterministic:

    python3 tests/graphics/fonts/generate.py

`--dodo` prints dodo-test.ttf as the Dodo array that
tests/stdlib/graphics_checks.dodo embeds, since that fixture has no file
system.
"""
import os
import struct
import sys

UNITS_PER_EM = 1000
ASCENT, DESCENT, LINE_GAP = 800, -200, 90
ON = 1

# Contours are lists of (x, y, on_curve) in font units, y up, clockwise
# for filled areas and counterclockwise for holes.
def poly(*points):
    return [(x, y, True) for x, y in points]

NOTDEF = [poly((50, 0), (50, 700), (450, 700), (450, 0)),
          poly((100, 50), (400, 50), (400, 650), (100, 650))]
LETTER_A = [poly((0, 0), (250, 700), (350, 700), (600, 0), (480, 0), (420, 180), (180, 180), (120, 0)),
            poly((220, 300), (380, 300), (300, 560))]
LETTER_O = [[(50, -10, False), (50, 710, False), (550, 710, False), (550, -10, False)],
            [(150, 90, False), (450, 90, False), (450, 610, False), (150, 610, False)]]
LETTER_D = [[(560, 700, False), (560, 350, True), (560, 0, False), (300, 0, True), (60, 0, True), (60, 700, True), (300, 700, True)],
            [(160, 100, True), (300, 100, True), (460, 100, False), (460, 350, True), (460, 600, False), (300, 600, True), (160, 600, True)]]
PERIOD = [poly((0, 0), (0, 120), (120, 120), (120, 0))]
LETTER_N = [poly((60, 0), (60, 700), (160, 700), (440, 180), (440, 700), (540, 700), (540, 0), (440, 0), (160, 520), (160, 0))]

ARG_WORDS, XY_VALUES, HAVE_SCALE, MORE, HAVE_XY_SCALE, HAVE_2X2 = 0x1, 0x2, 0x8, 0x20, 0x40, 0x80

# (name, advance, outline) where a composite outline is a list of
# (glyph, dx, dy, transform) and transform is None, a scale, or (xx, yx, xy, yy).
GLYPHS = [
    ('.notdef', 500, NOTDEF),
    ('space', 250, None),
    ('A', 600, LETTER_A),
    ('O', 600, LETTER_O),
    ('D', 620, LETTER_D),
    ('period', 220, PERIOD),
    ('Adieresis', 600, [(2, 0, 0, None), (5, 150, 780, None), (5, 330, 780, None)]),
    ('o', 350, [(3, 0, 0, 0.5)]),
    ('N', 600, LETTER_N),
    ('Z', 600, [(8, 600, 0, (0.0, 0.8, -0.8, 0.0))]),
]
# The last glyphs share the final advance, so hmtx lists fewer metrics.
METRICS = 9
CHARACTERS = {0x20: 1, 0x2E: 5, 0x41: 2, 0x44: 4, 0x4E: 8, 0x4F: 3, 0x5A: 9, 0x6F: 7, 0xC4: 6}
ASTRAL = {0x1F600: 3}
KERNING = {(2, 4): -80, (4, 2): -40, (2, 3): -30}


def f2dot14(value):
    return struct.pack('>h', int(round(value * 16384)))


def bbox_of(contours):
    xs = [p[0] for c in contours for p in c]
    ys = [p[1] for c in contours for p in c]
    return min(xs), min(ys), max(xs), max(ys)


def simple_glyph(contours):
    points = [p for c in contours for p in c]
    ends, total = [], 0
    for contour in contours:
        total += len(contour)
        ends.append(total - 1)
    flags, xs, ys = [], b'', b''
    px = py = 0
    for x, y, on in points:
        dx, dy = x - px, y - py
        px, py = x, y
        flag = ON if on else 0
        if dx == 0:
            flag |= 0x10
        elif -255 <= dx <= 255:
            flag |= 0x02 | (0x10 if dx > 0 else 0)
            xs += bytes([abs(dx)])
        else:
            xs += struct.pack('>h', dx)
        if dy == 0:
            flag |= 0x20
        elif -255 <= dy <= 255:
            flag |= 0x04 | (0x20 if dy > 0 else 0)
            ys += bytes([abs(dy)])
        else:
            ys += struct.pack('>h', dy)
        flags.append(flag)
    # Run-length encode equal flags with the repeat bit.
    encoded, i = b'', 0
    while i < len(flags):
        run = 1
        while i + run < len(flags) and flags[i + run] == flags[i] and run < 256:
            run += 1
        if run > 1:
            encoded += bytes([flags[i] | 0x08, run - 1])
        else:
            encoded += bytes([flags[i]])
        i += run
    head = struct.pack('>h4h', len(contours), *bbox_of(contours))
    return head + b''.join(struct.pack('>H', e) for e in ends) + struct.pack('>H', 0) + encoded + xs + ys


def composite_glyph(components, boxes):
    body = b''
    for index, (glyph, dx, dy, transform) in enumerate(components):
        flags = XY_VALUES
        if index + 1 < len(components):
            flags |= MORE
        if -128 <= dx <= 127 and -128 <= dy <= 127:
            args = struct.pack('>bb', dx, dy)
        else:
            flags |= ARG_WORDS
            args = struct.pack('>hh', dx, dy)
        extra = b''
        if isinstance(transform, float):
            flags |= HAVE_SCALE
            extra = f2dot14(transform)
        elif transform is not None:
            flags |= HAVE_2X2
            extra = b''.join(f2dot14(v) for v in transform)
        body += struct.pack('>HH', flags, glyph) + args + extra
    x0 = min(b[0] for b in boxes); y0 = min(b[1] for b in boxes)
    x1 = max(b[2] for b in boxes); y1 = max(b[3] for b in boxes)
    return struct.pack('>h4h', -1, x0, y0, x1, y1) + body


def glyph_table(long_loca):
    data, offsets, boxes = b'', [], {}
    for index, (_, _, outline) in enumerate(GLYPHS):
        offsets.append(len(data))
        if outline is None:
            boxes[index] = (0, 0, 0, 0)
            continue
        if isinstance(outline[0], tuple):
            parts = []
            for glyph, dx, dy, transform in outline:
                x0, y0, x1, y1 = boxes[glyph]
                corners = [(x0, y0), (x0, y1), (x1, y0), (x1, y1)]
                if isinstance(transform, float):
                    corners = [(x * transform, y * transform) for x, y in corners]
                elif transform is not None:
                    xx, yx, xy, yy = transform
                    corners = [(xx * x + xy * y, yx * x + yy * y) for x, y in corners]
                xs = [x + dx for x, _ in corners]; ys = [y + dy for _, y in corners]
                parts.append((int(min(xs)), int(min(ys)), int(max(xs)), int(max(ys))))
            boxes[index] = (min(p[0] for p in parts), min(p[1] for p in parts), max(p[2] for p in parts), max(p[3] for p in parts))
            data += composite_glyph(outline, parts)
        else:
            boxes[index] = bbox_of(outline)
            data += simple_glyph(outline)
        while len(data) % 4:
            data += b'\0'
    offsets.append(len(data))
    if long_loca:
        loca = b''.join(struct.pack('>I', o) for o in offsets)
    else:
        loca = b''.join(struct.pack('>H', o // 2) for o in offsets)
    return data, loca


def cmap_format4():
    # 'A' through 'D' use idRangeOffset into a glyph array (B and C are
    # missing); every other character is its own idDelta segment.
    segments = [(0x41, 0x44, None)]
    for code in sorted(CHARACTERS):
        if not 0x41 <= code <= 0x44:
            segments.append((code, code, (CHARACTERS[code] - code) & 0xFFFF))
    segments.sort()
    segments.append((0xFFFF, 0xFFFF, 1))
    count = len(segments)
    ends = b''.join(struct.pack('>H', s[1]) for s in segments)
    starts = b''.join(struct.pack('>H', s[0]) for s in segments)
    deltas = b''.join(struct.pack('>H', s[2] or 0) for s in segments)
    array = [CHARACTERS.get(c, 0) for c in range(0x41, 0x45)]
    ranges = b''
    for i, s in enumerate(segments):
        if s[2] is None:
            # Bytes from this idRangeOffset entry to the glyph array.
            ranges += struct.pack('>H', 2 * (count - i))
        else:
            ranges += struct.pack('>H', 0)
    body = ends + b'\0\0' + starts + deltas + ranges + b''.join(struct.pack('>H', g) for g in array)
    search = 2 ** (count.bit_length() - 1)
    header = struct.pack('>HHHHHHH', 4, 14 + len(body), 0, 2 * count, 2 * search, search.bit_length() - 1, 2 * count - 2 * search)
    return header + body


def cmap_format12():
    mapping = dict(CHARACTERS)
    mapping.update(ASTRAL)
    groups = b''.join(struct.pack('>III', c, c, mapping[c]) for c in sorted(mapping))
    return struct.pack('>HHIII', 12, 0, 16 + len(groups), 0, len(mapping)) + groups


def cmap_table(with_format12):
    subtables = [(3, 1, cmap_format4())]
    if with_format12:
        subtables.append((3, 10, cmap_format12()))
    offset = 4 + 8 * len(subtables)
    records, bodies = b'', b''
    for platform, encoding, body in subtables:
        records += struct.pack('>HHI', platform, encoding, offset + len(bodies))
        bodies += body
    return struct.pack('>HH', 0, len(subtables)) + records + bodies


def kern_table():
    pairs = sorted(KERNING.items())
    count = len(pairs)
    search = 2 ** (count.bit_length() - 1)
    body = struct.pack('>HHHH', count, 6 * search, search.bit_length() - 1, 6 * (count - search))
    body += b''.join(struct.pack('>HHh', l, r, v) for (l, r), v in pairs)
    return struct.pack('>HH', 0, 1) + struct.pack('>HHH', 0, 6 + len(body), 0x0001) + body


def tables(long_loca, with_format12, with_kern):
    glyf, loca = glyph_table(long_loca)
    advance_max = max(g[1] for g in GLYPHS)
    head = struct.pack('>IIIIHH', 0x00010000, 0x00010000, 0, 0x5F0F3CF5, 0x000B, UNITS_PER_EM)
    head += b'\0' * 16 + struct.pack('>4h', -750, -200, 650, 900)
    head += struct.pack('>HHhhh', 0, 8, 2, 1 if long_loca else 0, 0)
    hhea = struct.pack('>Ihhh', 0x00010000, ASCENT, DESCENT, LINE_GAP)
    hhea += struct.pack('>Hhhh', advance_max, 0, 0, 650) + struct.pack('>hhh', 1, 0, 0) + b'\0' * 8 + struct.pack('>hH', 0, METRICS)
    maxp = struct.pack('>IH', 0x00010000, len(GLYPHS)) + struct.pack('>13H', 64, 4, 64, 4, 2, 0, 0, 0, 0, 0, 0, 3, 2)
    hmtx = b''
    for index, (_, advance, outline) in enumerate(GLYPHS):
        lsb = 0
        if index < METRICS:
            hmtx += struct.pack('>Hh', advance, lsb)
        else:
            hmtx += struct.pack('>h', lsb)
    result = {b'cmap': cmap_table(with_format12), b'glyf': glyf, b'head': head, b'hhea': hhea,
              b'hmtx': hmtx, b'loca': loca, b'maxp': maxp}
    if with_kern:
        result[b'kern'] = kern_table()
    return result


def checksum(data):
    data += b'\0' * (-len(data) % 4)
    return sum(struct.unpack('>%dI' % (len(data) // 4), data)) & 0xFFFFFFFF


def font_file(table_map, base=0):
    """An sfnt whose table offsets count from `base` bytes before it."""
    tags = sorted(table_map)
    count = len(tags)
    search = 2 ** (count.bit_length() - 1)
    header = struct.pack('>IHHHH', 0x00010000, count, 16 * search, search.bit_length() - 1, 16 * (count - search))
    offset = base + 12 + 16 * count
    records, bodies = b'', b''
    for tag in tags:
        body = table_map[tag]
        records += struct.pack('>4sIII', tag, checksum(body), offset + len(bodies), len(body))
        bodies += body + b'\0' * (-len(body) % 4)
    return header + records + bodies


def dodo_array(data):
    lines = ['const TEST_FONT: [%d]u8 = [' % len(data)]
    for i in range(0, len(data), 20):
        chunk = ', '.join(str(b) for b in data[i:i + 20])
        lines.append('    ' + chunk + (',' if i + 20 < len(data) else ''))
    lines.append(']')
    return '\n'.join(lines)


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    short = tables(False, False, True)
    if '--dodo' in sys.argv[1:]:
        print(dodo_array(font_file(short)))
        return
    long = tables(True, True, False)
    with open(os.path.join(here, 'dodo-test.ttf'), 'wb') as out:
        out.write(font_file(short))
    with open(os.path.join(here, 'dodo-test-long.ttf'), 'wb') as out:
        out.write(font_file(long))
    first_offset = 12 + 8
    first = font_file(short, first_offset)
    second = font_file(long, first_offset + len(first))
    collection = struct.pack('>4sIIII', b'ttcf', 0x00010000, 2, first_offset, first_offset + len(first))
    with open(os.path.join(here, 'dodo-test.ttc'), 'wb') as out:
        out.write(collection + first + second)


if __name__ == '__main__':
    main()
