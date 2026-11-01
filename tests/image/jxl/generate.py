#!/usr/bin/env python3
"""Regenerate the JPEG XL decoder fixtures in this directory.

Synthesizes deterministic source images (PNG, APNG, PFM), encodes them with
cjxl (libjxl 0.12) in many configurations, decodes each with djxl, and writes:

  NAME.jxl          the encoded file
  NAME.png          for lossy files: djxl's 8-bit decode, the comparison
                    reference
  cases.dodo        a table of every case with its expected size, layout and
                    a 64-bit FNV-1a hash of the decoded samples in the
                    decoder's native layout (lossless cases)

  NAME[-K]-ecE_ref.jxl  extra channel E of (displayed frame K of) NAME as
                    djxl decodes it (float output, stored as lossless 16-bit
                    gray)

Run from anywhere:  python3 tests/image/jxl/generate.py [OUTDIR]
Needs cjxl, djxl and ffmpeg (JPEG sources) on PATH; the tests themselves do
not. Bitstreams cjxl never emits (splines, patches or splines in upsampled
frames, upsampled LF frames, chroma-subsampled modular and VarDCT frames
with filters, many and non-alpha extra channels, frames larger than the
image or starting outside it) are written by this script (see
codestream.py) and decoded by djxl like the others (the frame_huge and
ref_huge cases, too large for djxl, decode like frame_big and
ref_patches). libjxl 0.12 cannot decode chroma-subsampled VarDCT frames
with adaptive LF smoothing (only libjxl main smooths each channel at its
size); the reference of that case comes from the djxl named by
$DJXL_MAIN (a libjxl main build), and the committed reference is kept
when it is not set.
"""
import math, os, shutil, struct, subprocess, sys, zlib, random, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import codestream as cs
OUT = sys.argv[1] if len(sys.argv) > 1 else HERE
TMP = tempfile.mkdtemp(prefix='jxlgen')

# ----------------------------------------------------------------------------
# Source images

def chunk(t, d):
    return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)

def png_bytes(w, h, ch, bits, pix, extra=b''):
    ct = {1: 0, 2: 4, 3: 2, 4: 6}[ch]
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            for c in range(ch):
                v = pix(x, y, c)
                if bits == 16:
                    raw += struct.pack('>H', v)
                else:
                    raw.append(v)
    ihdr = struct.pack('>IIBBBBB', w, h, bits, ct, 0, 0, 0)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr) + extra + chunk(b'IDAT', zlib.compress(bytes(raw), 9)) + chunk(b'IEND', b'')

def write_png(name, w, h, ch, bits, pix, extra=b''):
    path = os.path.join(TMP, name + '.png')
    open(path, 'wb').write(png_bytes(w, h, ch, bits, pix, extra))
    return path

def write_apng(name, w, h, frames, delays, blend=0, dispose=0, offsets=None):
    """frames: list of (fw, fh, pix) RGBA8; offsets: list of (x, y)."""
    out = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
    out += chunk(b'acTL', struct.pack('>II', len(frames), 0))
    seq = 0
    for i, (fw, fh, pix) in enumerate(frames):
        ox, oy = offsets[i] if offsets else (0, 0)
        out += chunk(b'fcTL', struct.pack('>IIIIIHHBB', seq, fw, fh, ox, oy, delays[i], 100, dispose, blend))
        seq += 1
        raw = bytearray()
        for y in range(fh):
            raw.append(0)
            for x in range(fw):
                for c in range(4):
                    raw.append(pix(x, y, c))
        data = zlib.compress(bytes(raw), 9)
        if i == 0:
            out += chunk(b'IDAT', data)
        else:
            out += chunk(b'fdAT', struct.pack('>I', seq) + data)
            seq += 1
    out += chunk(b'IEND', b'')
    path = os.path.join(TMP, name + '.png')
    open(path, 'wb').write(out)
    return path

def write_pfm(name, w, h, ch, pix):
    path = os.path.join(TMP, name + '.pfm')
    data = bytearray(('PF' if ch == 3 else 'Pf').encode() + b'\n%d %d\n-1.0\n' % (w, h))
    for y in range(h - 1, -1, -1):
        for x in range(w):
            for c in range(ch):
                data += struct.pack('<f', pix(x, y, c))
    open(path, 'wb').write(bytes(data))
    return path

def noise(seed):
    rng = random.Random(seed)
    cache = {}
    def f(x, y, c, scale=256):
        key = (x, y, c)
        if key not in cache:
            cache[key] = rng.random()
        return int(cache[key] * scale) % scale
    return f

N1 = noise(1)
def gradient(x, y, c):
    return (x * 3 + y * 5 + c * 70) & 255
def smooth_noise(x, y, c):
    return ((x * 2 + y + c * 60) + N1(x // 3, y // 3, c, 24)) & 255
def few_colors(x, y, c):
    palette = [(200, 30, 30), (20, 180, 60), (40, 40, 220), (240, 240, 240), (10, 10, 10)]
    k = ((x // 7) + (y // 5) * 3) % len(palette)
    return palette[k][c % 3]
def text_like(x, y, c):
    on = ((x * 7 + y * 13) % 17 < 3) or ((x // 4 + y // 6) % 5 == 0 and x % 4 == 1)
    return 15 if on else 235
PN = noise(7)
def photo(x, y, c):
    """Photographic-like: smooth shading, texture, blocks and a disc."""
    v = 0.5 + 0.25 * math.sin(x * 0.071 + c) * math.cos(y * 0.053 - c * 0.7)
    v += 0.12 * math.sin((x + y) * 0.21 + c * 2)
    if (x // 13 + y // 11) % 5 == 0:
        v += 0.18
    if (x - 40) ** 2 + (y - 30) ** 2 < 300:
        v = 0.85 - 0.25 * c
    v += 0.08 * (PN(x, y, c, 1000) / 1000.0 - 0.5)
    return max(0, min(255, int(v * 255 + 0.5)))
def edges(x, y, c):
    """Synthetic edges: gradients, discs, bars and thin lines."""
    v = (x * 0.5 + y * 0.35 + c * 50) % 200 + 20
    if (x - 150) ** 2 + (y - 120) ** 2 < 60 ** 2:
        v = [230, 60, 40][c]
    if 20 < x % 97 < 30:
        v = 255 - v
    if (x + 2 * y) % 41 == 0:
        v = 10
    if 200 < x < 260 and 30 < y < 200 and (x // 4 + y // 4) % 2 == 0:
        v = [20, 200, 90][c]
    return int(v) & 255
def alpha_ramp(x, y, c, w=64):
    if c == 3 or c == 1:
        return (x * 255 // max(1, w - 1)) & 255
    return gradient(x, y, c)

# A hand-built codestream with splines: a 64x64 modular XYB frame of zeros
# (one-leaf tree, zero predictor) plus splines. Every entropy-coded part
# uses one cluster with a 16-symbol prefix code of 4-bit codes and hybrid
# split 0, so token t > 0 carries 2^(t-1) + (t-1) raw bits.

class BitWriter:
    def __init__(self):
        self.bits = []
    def u(self, n, v):
        for i in range(n):
            self.bits.append((v >> i) & 1)
    def pad(self):
        while len(self.bits) % 8:
            self.bits.append(0)
    def data(self):
        self.pad()
        return bytes(sum(b << k for k, b in enumerate(self.bits[i:i + 8])) for i in range(0, len(self.bits), 8))

def spline_code(w, ndist):
    w.u(1, 0)                         # no LZ77
    if ndist > 1:
        w.u(1, 1); w.u(2, 0)          # simple context map: all cluster 0
    w.u(1, 1)                         # prefix codes
    w.u(4, 0)                         # hybrid config split 0
    w.u(1, 1); w.u(4, 3); w.u(3, 7)   # 16 symbols
    w.u(2, 0)                         # HSKIP 0
    lengths = {0: [0, 0], 1: [1, 1, 1, 0]}
    for v in [0, 0, 0, 1] + [0] * 14:  # only code length 4 used
        w.bits.extend(lengths[v])

def spline_uint(w, v):
    def sym(t):
        for i in range(3, -1, -1):
            w.bits.append((t >> i) & 1)
    if v == 0:
        sym(0)
        return
    n = v.bit_length() - 1
    sym(n + 1)
    w.u(n, v - (1 << n))

def packed(v):
    return 2 * v if v >= 0 else -2 * v - 1

def splines_jxl(path, splines, qadj, size=64):
    w = BitWriter()
    w.u(8, 0xFF); w.u(8, 0x0A)
    w.u(1, 1); w.u(5, size // 8 - 1); w.u(3, 1)  # SizeHeader
    w.u(1, 1); w.u(1, 1)              # default metadata (XYB), transform data
    w.pad()
    w.u(1, 0); w.u(2, 0); w.u(1, 1)   # frame header: regular, modular
    w.u(2, 1); w.u(4, 15)             # flags: splines
    w.u(2, 0); w.u(2, 1); w.u(2, 0)   # upsampling 1, group shift 1, 1 pass
    w.u(1, 0); w.u(2, 0); w.u(1, 1)   # no crop, replace, last
    w.u(2, 0)                         # no name
    w.u(1, 0); w.u(1, 0); w.u(2, 0); w.u(2, 0)  # no Gaborish, no EPF
    w.u(2, 0)                         # no extensions
    s = BitWriter()
    spline_code(s, 6)
    spline_uint(s, len(splines) - 1)
    last = (0, 0)
    for i, sp in enumerate(splines):
        x, y = sp[0]
        if i == 0:
            spline_uint(s, x); spline_uint(s, y)
        else:
            spline_uint(s, packed(x - last[0])); spline_uint(s, packed(y - last[1]))
        last = (x, y)
    spline_uint(s, packed(qadj))
    for start, deltas, dct in splines:
        spline_uint(s, len(deltas))
        for dx, dy in deltas:
            spline_uint(s, packed(dx)); spline_uint(s, packed(dy))
        for c in range(4):
            for v in dct[c] + [0] * (32 - len(dct[c])):
                spline_uint(s, packed(v))
    s.u(1, 1)                         # default LF dequantization
    s.u(1, 1)                         # global tree:
    spline_code(s, 6)
    for _ in range(5):
        spline_uint(s, 0)             # one leaf, zero predictor
    spline_code(s, 1)
    s.u(1, 1); s.u(1, 1); s.u(2, 0)   # global tree, default WP, no transforms
    for _ in range(3 * size * size):
        spline_uint(s, 0)
    section = s.data()
    w.u(1, 0); w.pad()                # TOC
    w.u(2, 1); w.u(14, len(section) - 1024)
    w.pad()
    open(path, 'wb').write(w.data() + section)

def ycbcr_jxl(path, w, h, modes, extra=0):
    """A modular 8-bit YCbCr frame with chroma subsampling (cjxl never
    writes one): channels Cb, Y, Cr at their subsampled sizes, coded with
    the zero predictor so the residuals are the samples."""
    hs_of = [0, 1, 1, 0]
    vs_of = [0, 1, 0, 1]
    maxh = max(hs_of[m] for m in modes)
    maxv = max(vs_of[m] for m in modes)
    wr = BitWriter()
    wr.u(8, 0xFF); wr.u(8, 0x0A)
    wr.u(1, 0); wr.u(2, 1); wr.u(13, h - 1)   # SizeHeader: height U32 (1 + u(13))
    wr.u(3, 0); wr.u(2, 1); wr.u(13, w - 1)   # explicit width
    wr.u(1, 0); wr.u(1, 0)            # metadata: not default, no extra fields
    wr.u(1, 0); wr.u(2, 0)            # integer samples, 8 bits
    wr.u(1, 1)                        # 16-bit buffers
    if extra == 0:
        wr.u(2, 0)
    else:
        wr.u(2, 3); wr.u(12, extra - 1)  # extra channels (default: 8-bit alpha)
        for _ in range(extra):
            wr.u(1, 1)
    wr.u(1, 0)                        # not XYB
    wr.u(1, 1)                        # default (sRGB) colour encoding
    wr.u(2, 0)                        # no extensions
    wr.u(1, 1)                        # default transform data
    wr.pad()
    wr.u(1, 0); wr.u(2, 0); wr.u(1, 1)  # regular modular frame
    wr.u(2, 0)                        # no flags
    wr.u(1, 1)                        # YCbCr
    for m in modes:
        wr.u(2, m)
    wr.u(2, 0)                        # upsampling 1
    for _ in range(extra):
        wr.u(2, 0)
    wr.u(2, 1); wr.u(2, 0)            # group shift 1, 1 pass
    wr.u(1, 0); wr.u(2, 0)            # no crop, replace
    for _ in range(extra):
        wr.u(2, 0)
    wr.u(1, 1)                        # last
    wr.u(2, 0)
    wr.u(1, 0); wr.u(1, 0); wr.u(2, 0); wr.u(2, 0)  # no filters
    wr.u(2, 0)
    s = BitWriter()
    s.u(1, 1)                         # default LF dequantization
    s.u(1, 1)                         # global tree: one leaf, zero predictor
    spline_code(s, 6)
    for _ in range(5):
        spline_uint(s, 0)
    spline_code(s, 1)
    s.u(1, 1); s.u(1, 1); s.u(2, 0)
    for c in range(3):
        cw = (w + (1 << (maxh - hs_of[modes[c]])) - 1) >> (maxh - hs_of[modes[c]])
        ch = (h + (1 << (maxv - vs_of[modes[c]])) - 1) >> (maxv - vs_of[modes[c]])
        for y in range(ch):
            for x in range(cw):
                if c == 1:
                    v = int(90 * math.sin(x * 0.4) * math.cos(y * 0.3)) + (x * 3 - y * 2) % 30
                else:
                    v = int(60 * math.sin(x * 0.7 + c + y * 0.5))
                spline_uint(s, packed(v))
    for e in range(extra):
        for y in range(h):
            for x in range(w):
                spline_uint(s, (x * (e + 3) + y * 5 + e * 40) % 256)
    section = s.data()
    wr.u(1, 0); wr.pad()
    if len(section) < 1024:
        wr.u(2, 0); wr.u(10, len(section))
    else:
        wr.u(2, 1); wr.u(14, len(section) - 1024)
    wr.pad()
    open(path, 'wb').write(wr.data() + section)

# Multi-frame codestreams with frames larger than the image, offset frames
# and reference-only frames read by patches (cjxl writes none of these).
# Modular 8-bit RGB frames whose samples come from an MA tree with zero
# residuals: the leaves' code has a single symbol, so channel data costs
# no bits and a frame of any size takes a few bytes per group.

CROP_DIST = [(0, 8), (256, 11), (2304, 14), (18688, 30)]
TOC_DIST = [(0, 10), (1024, 14), (17408, 22), (4211712, 30)]

def u32d(w, v, dists):
    for sel, (off, bits) in enumerate(dists):
        if v >= off and v - off < (1 << bits):
            w.u(2, sel); w.u(bits, v - off)
            return
    raise ValueError(v)

def zero_code(w, ndist):
    """A Code whose single cluster always yields symbol 0 (no bits)."""
    w.u(1, 0)                         # no LZ77
    if ndist > 1:
        w.u(1, 1); w.u(2, 0)          # simple context map: all cluster 0
    w.u(1, 1)                         # prefix codes
    w.u(4, 0)                         # hybrid config split 0
    w.u(1, 0)                         # alphabet of one symbol

def tree_tokens(s, tree):
    """An MA tree from nested tuples ('split', property, value, gt, le) and
    ('leaf', predictor, offset), in breadth-first order."""
    spline_code(s, 6)
    queue = [tree]
    leaves = 0
    while queue:
        node = queue.pop(0)
        if node[0] == 'split':
            spline_uint(s, node[1] + 1); spline_uint(s, packed(node[2]))
            queue.append(node[3]); queue.append(node[4])
        else:
            spline_uint(s, 0); spline_uint(s, node[1]); spline_uint(s, packed(node[2]))
            spline_uint(s, 0); spline_uint(s, 0)
            leaves += 1
    zero_code(s, leaves)

def frame_jxl_header(w, iw, ih):
    w.u(8, 0xFF); w.u(8, 0x0A)
    w.u(1, 0); w.u(2, 1); w.u(13, ih - 1)
    w.u(3, 0); w.u(2, 1); w.u(13, iw - 1)
    w.u(1, 0); w.u(1, 0)              # metadata: not default, no extra fields
    w.u(1, 0); w.u(2, 0)              # integer samples, 8 bits
    w.u(1, 1); w.u(2, 0)              # 16-bit buffers, no extra channels
    w.u(1, 0); w.u(1, 1); w.u(2, 0)   # not XYB, sRGB, no extensions
    w.u(1, 1)                         # default transform data
    w.pad()

def frame_jxl_frame(w, iw, ih, f):
    """One frame. f: dict with type (0 regular, 2 reference-only), crop
    (x0, y0, width, height) or None, last, save (slot), before (save before
    the color transform), tree, patches (list of (slot, rx, ry, pw, ph,
    [(x, y)...])), group_shift."""
    ftype = f.get('type', 0)
    gshift = f.get('group_shift', 1)
    crop = f.get('crop')
    fw, fh = (crop[2], crop[3]) if crop else (iw, ih)
    x0, y0 = (crop[0], crop[1]) if crop else (0, 0)
    last = f.get('last', False)
    patches = f.get('patches')
    w.u(1, 0); w.u(2, ftype); w.u(1, 1)
    if patches:
        w.u(2, 1); w.u(4, 1)          # flags: patches
    else:
        w.u(2, 0)
    w.u(1, 0)                         # not YCbCr
    w.u(2, 0)                         # upsampling 1
    w.u(2, gshift)
    if ftype != 2:
        w.u(2, 0)                     # one pass
    w.u(1, 1 if crop else 0)
    if crop:
        if ftype != 2:
            u32d(w, packed(x0), CROP_DIST); u32d(w, packed(y0), CROP_DIST)
        u32d(w, fw, CROP_DIST); u32d(w, fh, CROP_DIST)
    partial = crop is not None and ftype != 2 and (x0 > 0 or y0 > 0 or x0 + fw < iw or y0 + fh < ih)
    if ftype in (0, 3):
        w.u(2, 0)                     # replace
        if partial:
            w.u(2, f.get('source', 0))
        w.u(1, 1 if last else 0)
    if not last:
        w.u(2, f.get('save', 0))
        if (ftype in (0, 3) and not partial) or ftype == 2:
            w.u(1, 1 if f.get('before') else 0)
    w.u(2, 0)                         # no name
    w.u(1, 0); w.u(1, 0); w.u(2, 0); w.u(2, 0)  # no Gaborish, no EPF
    w.u(2, 0)                         # no extensions
    # Sections.
    gd = 128 << gshift
    gx = (fw + gd - 1) // gd
    gy = (fh + gd - 1) // gd
    lgx = (fw + 8 * gd - 1) // (8 * gd)
    lgy = (fh + 8 * gd - 1) // (8 * gd)
    s = BitWriter()
    if patches:
        spline_code(s, 10)
        spline_uint(s, len(patches))
        for slot, rx, ry, pw, ph, places in patches:
            spline_uint(s, slot); spline_uint(s, rx); spline_uint(s, ry)
            spline_uint(s, pw - 1); spline_uint(s, ph - 1)
            spline_uint(s, len(places) - 1)
            px, py = 0, 0
            for k, (x, y) in enumerate(places):
                if k == 0:
                    spline_uint(s, x); spline_uint(s, y)
                else:
                    spline_uint(s, packed(x - px)); spline_uint(s, packed(y - py))
                px, py = x, y
                spline_uint(s, 1)     # replace
    s.u(1, 1)                         # default LF dequantization
    s.u(1, 1)                         # global tree
    tree_tokens(s, f['tree'])
    s.u(1, 1); s.u(1, 1); s.u(2, 0)   # global tree, default WP, no transforms
    sections = [s.data()]
    if gx * gy > 1:
        sections += [b''] * (lgx * lgy)   # LF groups: no channels
        sections += [b'']                 # HfGlobal
        g = BitWriter()
        g.u(1, 1); g.u(1, 1); g.u(2, 0)
        sections += [g.data()] * (gx * gy)
    w.u(1, 0); w.pad()                # TOC, not permuted
    for sec in sections:
        u32d(w, len(sec), TOC_DIST)
    w.pad()
    return b''.join(sections)

def frames_jxl(path, iw, ih, frames):
    w = BitWriter()
    frame_jxl_header(w, iw, ih)
    out = b''
    for f in frames:
        # Frames start byte aligned: header + TOC, then the sections.
        hw = BitWriter()
        data = frame_jxl_frame(hw, iw, ih, f)
        out += hw.data() + data
    open(path, 'wb').write(w.data() + out)

# Sample patterns: group-local coordinates (properties 2 = y, 3 = x).
BASE_TREE = ('split', 0, 1, ('leaf', 0, 90), ('split', 0, 0, ('leaf', 0, 140), ('leaf', 0, 60)))

def stripes(prop, thresholds, values):
    """Constant leaves over the intervals of `prop` split at `thresholds`."""
    if len(values) == 1:
        return ('leaf', 0, values[0])
    mid = len(thresholds) // 2
    return ('split', prop, thresholds[mid], stripes(prop, thresholds[mid + 1:], values[mid + 1:]), stripes(prop, thresholds[:mid], values[:mid + 1]))

SPRITE_TREE = ('split', 0, 1,
               ('split', 3, 110, stripes(2, [62, 66, 152, 158], [20, 230, 70, 250, 140]), stripes(3, [20, 104, 108], [200, 40, 180, 90])),
               ('split', 0, 0, stripes(2, [10, 40, 64, 70, 74, 155, 160, 170], [15, 95, 175, 35, 255, 115, 55, 205, 135]),
                stripes(3, [16, 48, 96, 112, 124, 140, 205, 215, 300], [240, 30, 160, 80, 210, 10, 130, 60, 190, 100])))

def frame_cases():
    """The frame window cases; returns names to add as lossless cases."""
    iw, ih = 64, 48
    base = {'tree': BASE_TREE, 'save': 0}
    def over(crop, tree=SPRITE_TREE):
        return [dict(base), {'tree': tree, 'crop': crop, 'last': True}]
    # A frame much larger than the image (decoded over its visible part),
    # the same visible part from a frame of 6e8 pixels, and a frame starting
    # left of and above the image.
    frames_jxl(os.path.join(OUT, 'frame_big.jxl'), iw, ih, over((10, 6, 3000, 2000)))
    frames_jxl(os.path.join(OUT, 'frame_huge.jxl'), iw, ih, over((10, 6, 30000, 20000)))
    frames_jxl(os.path.join(OUT, 'frame_negative.jxl'), iw, ih, over((-40, -30, 1500, 1100)))
    # A reference-only frame larger than the image, read by patches near
    # its origin; and the same from a 6e8-pixel reference frame.
    def ref(size):
        return [dict(base), {'type': 2, 'tree': SPRITE_TREE, 'crop': (0, 0, size[0], size[1]), 'save': 1, 'before': True},
                {'tree': BASE_TREE, 'last': True, 'patches': [(1, 100, 60, 30, 20, [(5, 5), (30, 26)]), (1, 200, 150, 20, 18, [(40, 20)])]}]
    frames_jxl(os.path.join(OUT, 'ref_patches.jxl'), iw, ih, ref((4000, 3000)))
    frames_jxl(os.path.join(OUT, 'ref_huge.jxl'), iw, ih, ref((30000, 20000)))


class BitReader:
    def __init__(self, data, pos=0):
        self.data = data
        self.bit = pos * 8
    def u(self, n):
        v = 0
        for i in range(n):
            byte = self.data[self.bit >> 3] if (self.bit >> 3) < len(self.data) else 0
            v |= ((byte >> (self.bit & 7)) & 1) << i
            self.bit += 1
        return v
    def u32(self, dists):
        off, bits = dists[self.u(2)]
        return off + self.u(bits)
    def u64(self):
        sel = self.u(2)
        if sel == 0:
            return 0
        if sel == 1:
            return 1 + self.u(4)
        if sel == 2:
            return 17 + self.u(8)
        v = self.u(12)
        shift = 12
        while self.u(1):
            if shift == 60:
                v |= self.u(4) << 60
                break
            v |= self.u(8) << shift
            shift += 8
        return v
    def pad(self):
        self.bit = (self.bit + 7) & ~7

def unpack_signed(v):
    return v >> 1 if v % 2 == 0 else -((v + 1) >> 1)

def parse_frame(data, pos, iw, ih, xyb=True, animated=True):
    """Parse a frame header and TOC (no extra channels, no timecodes) of a
    cjxl file; returns (frame end, header dict)."""
    r = BitReader(data, pos)
    f = {'type': 0, 'modular': 0, 'w': iw, 'h': ih, 'x0': 0, 'y0': 0, 'passes': 1, 'up': 1, 'gshift': 1, 'lf_level': 0}
    if not r.u(1):
        f['type'] = r.u(2)
        f['modular'] = r.u(1)
        f['flags'] = r.u64()
        if not xyb:
            f['ycbcr'] = r.u(1)
            assert not f['ycbcr']
        if not (f['flags'] & 32):
            f['up'] = [1, 2, 4, 8][r.u(2)]
        if f['modular']:
            f['gshift'] = r.u(2)
        if not f['modular'] and xyb:
            r.u(3); r.u(3)
        if f['type'] != 2:
            f['passes'] = r.u32([(1, 0), (2, 0), (3, 0), (4, 3)])
            if f['passes'] != 1:
                nds = r.u32([(0, 0), (1, 0), (2, 0), (3, 1)])
                for _ in range(f['passes'] - 1):
                    r.u(2)
                for _ in range(nds):
                    r.u32([(1, 0), (2, 0), (4, 0), (8, 0)])
                for _ in range(nds):
                    r.u32([(0, 0), (1, 0), (2, 0), (0, 3)])
        if f['type'] == 1:
            f['lf_level'] = r.u32([(1, 0), (2, 0), (3, 0), (4, 0)])
        if f['type'] != 1:
            if r.u(1):
                if f['type'] != 2:
                    f['x0'] = unpack_signed(r.u32(CROP_DIST))
                    f['y0'] = unpack_signed(r.u32(CROP_DIST))
                f['w'] = r.u32(CROP_DIST)
                f['h'] = r.u32(CROP_DIST)
        partial = f['x0'] > 0 or f['y0'] > 0 or f['x0'] + f['w'] < iw or f['y0'] + f['h'] < ih
        last = False
        mode = 0
        if f['type'] in (0, 3):
            mode = r.u32([(0, 0), (1, 0), (2, 0), (3, 2)])
            if mode == 4:
                r.u(1)
            if mode != 0 or partial:
                r.u(2)
            if animated:
                f['duration'] = r.u32([(0, 0), (1, 0), (0, 8), (0, 32)])
            last = r.u(1)
        f['last'] = last
        save = 0
        if f['type'] != 1 and not last:
            save = r.u(2)
        can_ref = not last and f['type'] != 1 and (f.get('duration', 0) == 0 or save != 0)
        if can_ref and f['type'] in (0, 3) and mode == 0 and not partial:
            r.u(1)
        elif f['type'] == 2:
            r.u(1)
        name_len = r.u32([(0, 0), (0, 4), (16, 5), (48, 10)])
        r.u(8 * name_len)
        if not r.u(1):                # restoration filter
            gab = r.u(1)
            if gab and r.u(1):
                r.u(16 * 6)
            epf = r.u(2)
            if epf:
                if not f['modular'] and r.u(1):
                    r.u(16 * 8)
                if r.u(1):
                    r.u(16 * 5)
                if r.u(1):
                    if not f['modular']:
                        r.u(16)
                    r.u(16 * 3)
                if f['modular']:
                    r.u(16)
            ext = r.u64()
            assert ext == 0
        ext = r.u64()
        assert ext == 0
    else:
        f['last'] = True
    return r, f

def toc_end(data, bit, f):
    """The end of the frame whose TOC starts at `bit`, or None."""
    r = BitReader(data)
    r.bit = bit
    gd = 128 << f['gshift'] if f['modular'] else 256
    sw = (f['w'] + f['up'] - 1) // f['up']
    sh = (f['h'] + f['up'] - 1) // f['up']
    gx, gy = (sw + gd - 1) // gd, (sh + gd - 1) // gd
    lgx, lgy = (sw + 8 * gd - 1) // (8 * gd), (sh + 8 * gd - 1) // (8 * gd)
    n = 1 if gx * gy == 1 and f['passes'] == 1 else 2 + lgx * lgy + gx * gy * f['passes']
    if r.u(1):
        return None                   # permuted
    r.pad()
    total = 0
    for _ in range(n):
        total += r.u32(TOC_DIST)
    r.pad()
    return (r.bit >> 3) + total

def split_frames(data, start, iw, ih):
    """Byte ranges and headers of the frames of a cjxl animation."""
    frames = []
    pos = start
    while True:
        r, f = parse_frame(data, pos, iw, ih)
        end = toc_end(data, r.bit, f)
        if end is None or end > len(data):
            raise ValueError('cannot split frames')
        frames.append((pos, end, f))
        pos = end
        if f['last']:
            return frames

def write_apng_rgb(name, w, h, frames, delays, offsets):
    """An RGB APNG: frames of (fw, fh, pix), replace blending, no disposal."""
    out = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
    out += chunk(b'acTL', struct.pack('>II', len(frames), 0))
    seq = 0
    for i, (fw, fh, pix) in enumerate(frames):
        ox, oy = offsets[i]
        out += chunk(b'fcTL', struct.pack('>IIIIIHHBB', seq, fw, fh, ox, oy, delays[i], 100, 0, 0))
        seq += 1
        raw = bytearray()
        for y in range(fh):
            raw.append(0)
            for x in range(fw):
                for c in range(3):
                    raw.append(pix(x, y, c))
        data = zlib.compress(bytes(raw), 9)
        if i == 0:
            out += chunk(b'IDAT', data)
        else:
            out += chunk(b'fdAT', struct.pack('>I', seq) + data)
            seq += 1
    out += chunk(b'IEND', b'')
    path = os.path.join(TMP, name + '.png')
    open(path, 'wb').write(out)
    return path

def vardct_window_jxl(path, canvas, frame, origin, distance):
    """A VarDCT frame larger than the image: cjxl encodes an animation whose
    second frame is cropped (frame size at origin within a larger first
    frame); that frame is kept alone under a small XYB animation header."""
    big = (origin[0] + frame[0] + 16, origin[1] + frame[1] + 16)
    apng = write_apng_rgb('vd_window_src', big[0], big[1],
                          [(big[0], big[1], lambda x, y, c: photo(x, y, c)),
                           (frame[0], frame[1], lambda x, y, c: int(128 + 90 * math.sin(x / 37.0 + c) * math.cos(y / 23.0 - c)) if x < 300 and y < 260 else int(128 + 100 * math.sin(x / 140.0 + c) * math.cos(y / 110.0)))],
                          [10, 10], [(0, 0), origin])
    tmp = os.path.join(TMP, 'vd_window_src.jxl')
    run(['cjxl', apng, tmp, '--quiet', '-d', str(distance)])
    data = open(tmp, 'rb').read()
    assert data[:2] == b'\xff\x0a'
    frames = None
    for start in range(3, 200):
        try:
            got = split_frames(data, start, big[0], big[1])
        except Exception:
            continue
        if len(got) == 2 and got[-1][1] == len(data) and got[1][2]['x0'] == origin[0] and got[1][2]['w'] == frame[0] and not got[1][2]['modular']:
            frames = got
            break
    if frames is None:
        raise SystemExit('cannot split the cjxl animation')
    w = BitWriter()
    w.u(8, 0xFF); w.u(8, 0x0A)
    w.u(1, 0); w.u(2, 1); w.u(13, canvas[1] - 1)
    w.u(3, 0); w.u(2, 1); w.u(13, canvas[0] - 1)
    w.u(1, 0); w.u(1, 1)              # metadata: extra fields
    w.u(3, 0); w.u(1, 0); w.u(1, 0)   # orientation, no intrinsic size or preview
    w.u(1, 1); w.u(2, 0); w.u(2, 0); w.u(2, 0); w.u(1, 0)  # animation: 100/1 tps, no timecodes
    w.u(1, 0); w.u(2, 0)              # integer samples, 8 bits
    w.u(1, 1); w.u(2, 0)              # 16-bit buffers, no extra channels
    w.u(1, 1); w.u(1, 1)              # XYB, sRGB
    w.u(1, 1); w.u(2, 0)              # default tone mapping, no extensions
    w.u(1, 1)                         # default transform data
    w.pad()
    start, end, f = frames[1]
    open(path, 'wb').write(w.data() + data[start:end])

# ----------------------------------------------------------------------------
# Encoding and references

CASES = []

def run(args):
    r = subprocess.run(args, capture_output=True)
    if r.returncode != 0:
        raise SystemExit('failed: %s\n%s' % (' '.join(args), r.stderr.decode()))
    return r

def read_pam(path):
    d = open(path, 'rb').read()
    end = d.index(b'ENDHDR\n') + 7
    f = {}
    for line in d[:end].decode().split('\n'):
        p = line.split()
        if len(p) >= 2:
            f[p[0]] = p[1]
    w, h, depth, maxval = int(f['WIDTH']), int(f['HEIGHT']), int(f['DEPTH']), int(f['MAXVAL'])
    body = d[end:]
    vals = []
    if maxval < 256:
        vals = list(body[:w * h * depth])
    else:
        vals = [(body[2 * i] << 8) | body[2 * i + 1] for i in range(w * h * depth)]
    return w, h, depth, maxval, vals

def read_pfm(path):
    d = open(path, 'rb').read()
    parts = d.split(b'\n', 3)
    kind = parts[0].strip()
    w, h = [int(v) for v in parts[1].split()]
    scale = float(parts[2])
    ch = 3 if kind == b'PF' else 1
    fmt = '<f' if scale < 0 else '>f'
    body = parts[3]
    rows = []
    for y in range(h):
        row = []
        for x in range(w * ch):
            row.append(struct.unpack(fmt, body[(y * w * ch + x) * 4:(y * w * ch + x) * 4 + 4])[0])
        rows.append(row)
    rows.reverse()
    vals = [v for row in rows for v in row]
    return w, h, ch, vals

def fnv(data):
    h = 0xcbf29ce484222325
    for b in data:
        h = ((h ^ b) * 0x100000001b3) & 0xffffffffffffffff
    return h

def native_bytes(vals, maxval, sample):
    out = bytearray()
    for v in vals:
        if sample == 0:
            out.append(v if maxval == 255 else (v * 255 + maxval // 2) // maxval)
        elif sample == 1:
            s = v if maxval == 65535 else (v * 65535 + maxval // 2) // maxval
            out += struct.pack('<H', s)
        else:
            out += struct.pack('<f', v / maxval)
    return bytes(out)

def encode(name, source, args):
    jxl = os.path.join(OUT, name + '.jxl')
    run(['cjxl', source, jxl, '--quiet'] + args)
    size = os.path.getsize(jxl)
    if size > 20000:
        print('warning: %s is %d bytes' % (name, size))
    return jxl

def add(name, source, args, sample, pfm=False, float_alpha=False):
    """A lossless case: the decoded native samples must hash to djxl's."""
    if source is None:
        jxl = os.path.join(OUT, name + '.jxl')
    else:
        jxl = encode(name, source, args)
    if float_alpha is not False:
        # 8-bit samples from djxl's float decode (exact for 8-bit files).
        w, h, depth, vals = float_frames(jxl, 1, float_alpha)[0]
        CASES.append((name, '', w, h, depth, sample, 0, 0, fnv(native_bytes(vals, 255, sample))))
        return
    ref = os.path.join(TMP, name)
    if pfm:
        run(['djxl', jxl, ref + '.pfm', '--quiet'])
        w, h, ch, vals = read_pfm(ref + '.pfm')
        data = b''.join(struct.pack('<f', v) for v in vals)
        CASES.append((name, '', w, h, ch, 3, 0, 0, fnv(data)))
        return
    run(['djxl', jxl, ref + '.pam', '--quiet'])
    w, h, depth, maxval, vals = read_pam(ref + '.pam')
    CASES.append((name, '', w, h, depth, sample, 0, 0, fnv(native_bytes(vals, maxval, sample))))

def ec_file(base, ec):
    """djxl's file for extra channel ec (1-based, zero-padded to the digits
    of the channel count)."""
    for digits in range(1, 6):
        path = '%s-ec%0*d.pfm' % (base, digits, ec + 1)
        if os.path.exists(path):
            return path
    raise SystemExit('missing extra channel output %s-ec%d' % (base, ec + 1))

def float_frames(jxl, frames, alpha):
    """djxl's float decode of each frame as 8-bit samples (rounded), with
    the alpha extra channel `alpha` (or none): (w, h, depth, values)."""
    tmp = os.path.join(TMP, os.path.basename(jxl) + '.pfm')
    run(['djxl', jxl, tmp, '--quiet', '--output_extra_channels'] + (['--output_frames'] if frames > 1 else []))
    out = []
    for k in range(frames):
        base = tmp if frames == 1 else '%s-%d' % (tmp, k)
        w, h, ch, color = read_pfm(base if frames == 1 else base + '.pfm')
        planes = [color[c::ch] for c in range(ch)]
        if alpha is not None:
            planes.append(read_pfm(ec_file(base, alpha))[3])
        depth = len(planes)
        vals = [int(round(min(1.0, max(0.0, planes[c][i])) * 255)) for i in range(w * h) for c in range(depth)]
        out.append((w, h, depth, vals))
    return out

def add_png(name, source, args, sample, frames=1, srgb=False, jxlref=False, decoder='djxl', float_alpha=False):
    """A lossy or blended case: each displayed frame is compared with djxl's
    8-bit decode, NAME.png (or NAME-K.png per animation frame). With jxlref
    the reference is stored losslessly as NAME_ref.jxl (NAME-K_ref.jxl),
    which is smaller; the lossless fixtures check that path bit-exactly."""
    if source is None:
        jxl = os.path.join(OUT, name + '.jxl')
    else:
        jxl = encode(name, source, args)
    tmp = os.path.join(TMP, name + '.pam')
    extra = ['--color_space=RGB_D65_SRG_Rel_SRG'] if srgb else []
    if float_alpha is False:
        run([decoder, jxl, tmp, '--quiet', '--bits_per_sample=8', '--output_frames'] + extra)
    else:
        # djxl cannot write PAM with deeper extra channels: its float decode
        # (alpha from extra channel float_alpha, or None) rounded instead.
        decoded = float_frames(jxl, frames, float_alpha)
    for k in range(frames):
        if float_alpha is False:
            pam = tmp if frames == 1 else tmp + '-%d.pam' % k
            if frames > 1 and not os.path.exists(pam):
                raise SystemExit('missing frame %s' % pam)
            w, h, depth, maxval, vals = read_pam(pam)
        else:
            w, h, depth, vals = decoded[k]
            maxval = 255
        if depth > 4:
            # Color and the first (alpha) extra channel are output.
            vals = [vals[i * depth + c] for i in range(w * h) for c in range(4)]
            depth = 4
        ref = name if frames == 1 else '%s-%d' % (name, k)
        ch = {1: 0, 2: 4, 3: 2, 4: 6}[depth]
        if maxval != 255:
            vals = [(v * 255 + maxval // 2) // maxval for v in vals]
        rows = [vals[y * w * depth:(y + 1) * w * depth] for y in range(h)]
        data = png_bytes(w, h, depth, 8, lambda x, y, c: rows[y][x * depth + c])
        if jxlref:
            png = os.path.join(TMP, ref + '_ref.png')
            open(png, 'wb').write(data)
            run(['cjxl', png, os.path.join(OUT, ref + '_ref.jxl'), '--quiet', '-d', '0', '-e', '9'])
            CASES.append((name, ref + '_ref', w, h, depth, sample, 2, k, 0))
        else:
            open(os.path.join(OUT, ref + '.png'), 'wb').write(data)
            CASES.append((name, ref, w, h, depth, sample, 1, k, 0))

def add_file(name, sample=0):
    """A lossless case for a codestream written here."""
    jxl = os.path.join(OUT, name + '.jxl')
    ref = os.path.join(TMP, name)
    run(['djxl', jxl, ref + '.pam', '--quiet'])
    w, h, depth, maxval, vals = read_pam(ref + '.pam')
    CASES.append((name, '', w, h, depth, sample, 0, 0, fnv(native_bytes(vals, maxval, sample))))

def add_alias(name, other):
    """A case decoding like `other` (too large for djxl to decode)."""
    c = [c for c in CASES if c[0] == other][0]
    CASES.append((name,) + c[1:])

def add_ec(name, ec, sample, frames=1):
    """Extra channel `ec` of each displayed frame of NAME.jxl against djxl's
    float decode, stored as a lossless 16-bit gray NAME[-K]-ecE_ref.jxl."""
    jxl = os.path.join(OUT, name + '.jxl')
    tmp = os.path.join(TMP, name + '-ec.pfm')
    run(['djxl', jxl, tmp, '--quiet', '--output_extra_channels'] + (['--output_frames'] if frames > 1 else []))
    for k in range(frames):
        base = tmp if frames == 1 else '%s-%d' % (tmp, k)
        w, h, ch, vals = read_pfm(ec_file(base, ec))
        ref = '%s-ec%d' % (name, ec) if frames == 1 else '%s-%d-ec%d' % (name, k, ec)
        levels = [int(round(min(1.0, max(0.0, v)) * 65535)) for v in vals]
        png = os.path.join(TMP, ref + '_ref.png')
        open(png, 'wb').write(png_bytes(w, h, 1, 16, lambda x, y, c: levels[y * w + x]))
        run(['cjxl', png, os.path.join(OUT, ref + '_ref.jxl'), '--quiet', '-d', '0', '-e', '9'])
        CASES.append((name, ref + '_ref', w, h, 1, sample, 3, k, 0, ec))

def grid(w, h, f):
    return [[f(x, y) for x in range(w)] for y in range(h)]

def ycbcr_planes(w, h, modes):
    """Cb, Y, Cr samples at their subsampled sizes (8-bit, centered)."""
    hs_of, vs_of = [0, 1, 1, 0], [0, 1, 0, 1]
    maxh = max(hs_of[m] for m in modes)
    maxv = max(vs_of[m] for m in modes)
    out = []
    for c in range(3):
        cw = cs.ceil_div(w, 1 << (maxh - hs_of[modes[c]]))
        ch = cs.ceil_div(h, 1 << (maxv - vs_of[modes[c]]))
        if c == 1:
            out.append(grid(cw, ch, lambda x, y: int(90 * math.sin(x * 0.4) * math.cos(y * 0.3)) + (x * 3 - y * 2) % 30))
        else:
            out.append(grid(cw, ch, lambda x, y, c=c: int(60 * math.sin(x * 0.7 + c + y * 0.5))))
    return out

def hand_written():
    """Codestreams cjxl never writes, decoded by djxl for references."""
    path = lambda name: os.path.join(OUT, name + '.jxl')
    # Patches and splines in an upsampled frame; the alpha channel shares
    # the color upsampling, so patches blend it too (before upsampling).
    w, h = 32, 24
    ref = [grid(w, h, lambda x, y, c=c: 40 + c * 50 + x * 4 + y * 2) for c in range(3)] + [grid(w, h, lambda x, y: 255 - x * 6)]
    low = [grid(16, 12, lambda x, y, c=c: 60 + c * 40 + int(30 * math.sin(x * 0.5 + y * 0.3 + c))) for c in range(3)] + [grid(16, 12, lambda x, y: 200)]
    refs = [(1, 3, 2, 8, 6, [(1, 1, [(1, 0, False), (1, 0, False)]), (8, 6, [(2, 0, False), (2, 0, False)])]),
            (1, 20, 10, 9, 7, [(4, 2, [(4, 0, True), (4, 0, False)]), (6, 5, [(6, 0, False), (1, 0, False)])])]
    spl = ([((4, 5), [(8, 3), (2, 6)], [[3, 1], [6, 4, -2], [4, 0, 1], [5, 2]])], 0)
    cs.write_modular(path('patches_up'), w, h, [
        dict(channels=ref, ftype=2, save_as=1, save_before_ct=True),
        dict(channels=low, up=2, ec_up=[2], patches=refs, splines=spl)], ecs=[cs.EC(default=True)])
    add_png('patches_up', None, [], 0)
    # Splines in an upsampled XYB frame.
    spline_set = [((5, 6), [(8, 3), (-2, 4), (1, -5)], [[3, 1], [8, 4, -2], [5, 0, 1], [6, 2]]),
                  ((25, 4), [(-10, 10), (5, 0)], [[-2], [6, 0, 3], [-4, 2], [4, -1, 1]])]
    zeros = [grid(32, 32, lambda x, y: 0) for _ in range(3)]
    cs.write_modular(path('splines_up'), 64, 64, [dict(channels=zeros, up=2, splines=(spline_set, 0))], xyb=True)
    add_png('splines_up', None, [], 0, jxlref=True)
    # Modular YCbCr: 4:2:0 with Gaborish, EPF and upsampling; 4:2:2 with
    # custom Gaborish weights and three EPF steps.
    cs.write_modular(path('ycbcr420_filters'), 37, 29, [dict(channels=ycbcr_planes(19, 15, [0, 1, 0]), ycbcr=True, jup=(0, 1, 0), up=2, gab=True, epf=2, epf_sigma=0.7)], predictor=0)
    add_png('ycbcr420_filters', None, [], 0)
    cs.write_modular(path('ycbcr422_epf'), 33, 21, [dict(channels=ycbcr_planes(33, 21, [0, 2, 0]), ycbcr=True, jup=(0, 2, 0), gab=True, gab_weights=[0.1, 0.05, 0.12, 0.04, 0.08, 0.06], epf=3, epf_sigma=1.3)], predictor=0)
    add_png('ycbcr422_epf', None, [], 0)
    # VarDCT YCbCr 4:2:0 with adaptive LF smoothing, chroma from luma,
    # Gaborish and EPF (libjxl main reference); 4:2:2 upsampled without LF
    # smoothing. 8x8-sized varblocks only (larger ones are invalid there).
    small = [0, 0, 0, 1, 2, 3, 12, 13, 14, 15, 16, 17]
    rng = random.Random(5)
    wr = cs.Writer()
    cs.image_header(wr, 64, 48, xyb=False)
    cs.vardct_frame(wr, [], False, 64, 48, {(x, y): rng.choice(small) for y in range(6) for x in range(8)}, rng,
                    lf_values=lambda c, x, y: int(40 * math.sin(x * 0.9 + y * 0.7 + c)) + (20 if c == 1 else 0),
                    cfl=(84, 0.05, 0.2, 10, -20, [[30]], [[-40]]), ycbcr=True, jup=(0, 1, 0), gab=True, epf=1, coeff_range=4, density=0.12)
    open(path('vd_ycbcr420'), 'wb').write(wr.data())
    main = os.environ.get('DJXL_MAIN')
    if main:
        add_png('vd_ycbcr420', None, [], 0, jxlref=True, decoder=main)
    else:
        print('note: DJXL_MAIN not set, keeping the vd_ycbcr420 reference')
        if os.path.abspath(OUT) != HERE:
            shutil.copy(os.path.join(HERE, 'vd_ycbcr420_ref.jxl'), OUT)
        CASES.append(('vd_ycbcr420', 'vd_ycbcr420_ref', 64, 48, 3, 0, 2, 0, 0))
    wr = cs.Writer()
    cs.image_header(wr, 70, 50, xyb=False)
    cs.vardct_frame(wr, [], False, 70, 50, {(x, y): rng.choice(small) for y in range(4) for x in range(6)}, rng,
                    lf_values=lambda c, x, y: int(30 * math.cos(x * 0.8 + y + c)), ycbcr=True, jup=(0, 2, 0), up=2, epf=2, flags=128, coeff_range=4, density=0.12)
    open(path('vd_ycbcr422_up'), 'wb').write(wr.data())
    add_png('vd_ycbcr422_up', None, [], 0, jxlref=True)
    # An LF frame with 2x upsampling as the LF image of a VarDCT frame.
    w, h = 64, 48
    lw, lh = cs.ceil_div(cs.ceil_div(w, 8), 2), cs.ceil_div(cs.ceil_div(h, 8), 2)
    lf = [grid(lw, lh, lambda x, y: 256 + int(60 * math.sin(x * 0.9 + y * 0.6))),
          grid(lw, lh, lambda x, y: int(15 * math.cos(x * 0.7 - y * 0.8))),
          grid(lw, lh, lambda x, y: -150 + int(20 * math.sin(y * 1.1)))]
    wr = cs.Writer()
    cs.image_header(wr, w, h, xyb=True)
    cs.frame_header(wr, [], xyb=True, ftype=1, modular=True, lf_level=1, up=2)
    sec = cs.Writer()
    sec.u(1, 1)
    cs.global_tree(sec)
    cs.modular_stream(sec, lf)
    data = sec.data()
    cs.toc(wr, [len(data)])
    wr.bits.extend(cs.bits_of(data))
    rng = random.Random(9)
    layout = {}
    for y in range(0, 6, 2):
        for x in range(0, 8, 2):
            if (x // 2 + y // 2) % 3 == 0:
                layout[(x, y)] = 4
            else:
                for dy in range(2):
                    for dx in range(2):
                        layout[(x + dx, y + dy)] = rng.choice([0, 3, 12])
    cs.vardct_frame(wr, [], True, w, h, layout, rng, use_lf=True, coeff_range=2, density=0.05)
    open(path('lf_upsampled'), 'wb').write(wr.data())
    add_png('lf_upsampled', None, [], 0, jxlref=True)
    # A modular frame flagged to use an LF frame (which it ignores) after
    # a modular LF frame, in a file without VarDCT frames.
    rgb = [grid(16, 12, lambda x, y, c=c: x * 10 + y * 3 + c * 40) for c in range(3)]
    wr = cs.Writer()
    cs.image_header(wr, 16, 12, xyb=False)
    for header, channels in [(dict(ftype=1, lf_level=1), [grid(2, 2, lambda x, y: 50) for _ in range(3)]), (dict(flags=32), rgb)]:
        cs.frame_header(wr, [], xyb=False, **header)
        sec = cs.Writer()
        sec.u(1, 1)
        cs.global_tree(sec, 5)
        cs.modular_stream(sec, channels, 5)
        data = sec.data()
        cs.toc(wr, [len(data)])
        wr.bits.extend(cs.bits_of(data))
    open(path('modular_lf_flag'), 'wb').write(wr.data())
    add('modular_lf_flag', None, None, 0)
    # Extra channels of every kind: alpha, a 16-bit depth map at half size,
    # a spot color (rendered into the color image), a selection mask,
    # 12-bit thermal, CMYK black, a CFA channel and an optional half-float
    # channel (F32 output).
    w, h = 24, 20
    ecs = [cs.EC(default=True), cs.EC(kind=1, bits=16, dim_shift=1, name=b'depth'),
           cs.EC(kind=2, name=b'spot', spot=(0.9, 0.1, 0.2, 0.8)), cs.EC(kind=3, bits=1, name=b'selection'),
           cs.EC(kind=6, bits=12, name=b'thermal'), cs.EC(kind=4, name=b'black'), cs.EC(kind=5, cfa=2),
           cs.EC(kind=16, bits=16, exp_bits=5, name=b'half')]
    def half(v):
        return struct.unpack('<H', struct.pack('<e', v))[0]
    chans = [grid(w, h, lambda x, y, c=c: (x * 9 + y * 5 + c * 70) % 256) for c in range(3)]
    chans += [grid(w, h, lambda x, y: x * 11), grid(12, 10, lambda x, y: x * 5000 + y * 1000),
              grid(w, h, lambda x, y: 255 if (x // 6 + y // 5) % 2 else 40), grid(w, h, lambda x, y: 1 if x > y else 0),
              grid(w, h, lambda x, y: x * 150 + y * 20), grid(w, h, lambda x, y: x * 10 + y * 2),
              grid(w, h, lambda x, y: x * 7 + y * 3), grid(w, h, lambda x, y: half(x / 23.0 * (y / 19.0)))]
    cs.write_modular(path('extra_channels'), w, h, [dict(channels=chans)], ecs=ecs)
    add_png('extra_channels', None, [], 0, float_alpha=0)
    for ec, sample in [(0, 0), (1, 1), (2, 0), (3, 0), (4, 1), (5, 0), (6, 0), (7, 3)]:
        add_ec('extra_channels', ec, sample)
    # Extra channels composited across animation frames (a cropped frame
    # blends alpha over and adds depth).
    w, h = 20, 16
    ecs = [cs.EC(default=True), cs.EC(kind=1, bits=16, name=b'z')]
    f1 = [grid(w, h, lambda x, y, c=c: (x * 9 + c * 60) % 256) for c in range(3)] + [grid(w, h, lambda x, y: 255), grid(w, h, lambda x, y: x * 3000)]
    f2 = [grid(8, 6, lambda x, y, c=c: (y * 30 + c * 50) % 256) for c in range(3)] + [grid(8, 6, lambda x, y: 128 + x * 10), grid(8, 6, lambda x, y: 4000 + y * 1000)]
    cs.write_modular(path('ec_anim'), w, h, [
        dict(channels=f1, duration=1, is_last=False),
        dict(channels=f2, crop=(5, 4, 8, 6), image_size=(w, h), blend=(2, 0, False, 0), ec_blend=[(2, 0, False, 0), (1, 0, False, 0)], duration=1)],
        ecs=ecs, animated=True)
    add_png('ec_anim', None, [], 0, frames=2, float_alpha=0)
    add_ec('ec_anim', 1, 1, frames=2)
    # Many extra channels: 100 of mixed kinds and depths, some at half size.
    w, h = 5, 4
    ecs = []
    chans = [grid(w, h, lambda x, y, c=c: (x * 40 + y * 20 + c * 80) % 256) for c in range(3)]
    for i in range(100):
        kind = [1, 6, 3, 16][i % 4]
        bits = [8, 10, 16, 1][i % 4]
        shift = 1 if i % 7 == 3 else 0
        ecs.append(cs.EC(kind=kind, bits=bits, dim_shift=shift))
        cw, ch = cs.ceil_div(w, 1 << shift), cs.ceil_div(h, 1 << shift)
        chans.append(grid(cw, ch, lambda x, y, i=i, bits=bits: ((x * 3 + y + i) * ((1 << bits) - 1)) // (4 * 3 + 4 + 100)))
    cs.write_modular(path('many_ec'), w, h, [dict(channels=chans)], ecs=ecs)
    add_png('many_ec', None, [], 0, float_alpha=None)
    for ec, sample in [(2, 1), (3, 0), (98, 1), (99, 0)]:
        add_ec('many_ec', ec, sample)
    # The format's maximum of 4096 extra channels (all 8-bit alpha).
    w, h = 1, 1
    chans = [grid(w, h, lambda x, y, c=c: 100 + c * 20) for c in range(3)]
    chans += [grid(w, h, lambda x, y, i=i: (i * 5) % 7) for i in range(4096)]
    cs.write_modular(path('max_ec'), w, h, [dict(channels=chans)], ecs=[cs.EC(default=True)] * 4096)
    add('max_ec', None, None, 0, float_alpha=0)
    add_ec('max_ec', 4095, 0)

def make_icc(big=False):
    """A small RGB matrix/TRC ICC v2 profile with unusual primaries and a
    gamma curve, so cjxl keeps it as an ICC profile. `big` makes the curves
    4096-entry tables and adds 20 KB of text (an entropy-coded profile with
    many clusters and long LZ77 matches)."""
    def s15(v):
        return struct.pack('>i', int(round(v * 65536)))
    def xyz(x, y, z):
        return b'XYZ ' + b'\0' * 4 + s15(x) + s15(y) + s15(z)
    def curve(g):
        if big:
            n = 4096
            body = b''.join(struct.pack('>H', int(65535 * (i / (n - 1)) ** (g + 0.05 * math.sin(i / 300.0)))) for i in range(n))
            return b'curv' + b'\0' * 4 + struct.pack('>I', n) + body
        return b'curv' + b'\0' * 4 + struct.pack('>I', 1) + struct.pack('>H', int(g * 256)) + b'\0\0'
    desc_text = b'Dodo test RGB\0'
    desc = b'desc' + b'\0' * 4 + struct.pack('>I', len(desc_text)) + desc_text + b'\0' * 4 + b'\0' * 4 + b'\0' * 2 + b'\0' + b'\0' * 67
    text = b'text' + b'\0' * 4 + b'Public domain\0'
    if big:
        rng = random.Random(7)
        words = [bytes(rng.choice(b'abcdefghijklmnopqrstuvwxyz0123456789.') for _ in range(rng.randrange(2, 9))) for _ in range(400)]
        prose = b' '.join(rng.choice(words) for _ in range(6500))
        text = b'text' + b'\0' * 4 + prose[:20000] + b'\0'
    tags = [(b'desc', desc), (b'cprt', text), (b'wtpt', xyz(0.9642, 1.0, 0.8249)),
            (b'rXYZ', xyz(0.48, 0.26, 0.02)), (b'gXYZ', xyz(0.33, 0.66, 0.11)), (b'bXYZ', xyz(0.15, 0.08, 0.69)),
            (b'rTRC', curve(2.1)), (b'gTRC', curve(2.1)), (b'bTRC', curve(2.1))]
    offset = 128 + 4 + 12 * len(tags)
    table = struct.pack('>I', len(tags))
    data = b''
    for sig, body in tags:
        while (offset + len(data)) % 4:
            data += b'\0'
        table += sig + struct.pack('>II', offset + len(data), len(body))
        data += body
    size = 128 + len(table) + len(data)
    header = struct.pack('>I', size) + b'none' + bytes([2, 0x20, 0, 0]) + b'mntrRGB XYZ ' + b'\0' * 12 + b'acsp' + b'APPL' + b'\0' * 4 + b'\0' * 4 + b'\0' * 4 + b'\0' * 8 + b'\0' * 4 + s15(0.9642) + s15(1.0) + s15(0.8249) + b'\0' * 4 + b'\0' * 16 + b'\0' * 28
    assert len(header) == 128
    return header + table + data

def main():
    os.makedirs(OUT, exist_ok=True)
    rgb = write_png('rgb', 61, 47, 3, 8, smooth_noise)
    tiny = write_png('tiny', 1, 1, 3, 8, lambda x, y, c: [12, 200, 77][c])
    odd = write_png('odd', 3, 5, 3, 8, gradient)
    wide = write_png('wide', 300, 37, 3, 8, smooth_noise)
    big = write_png('big', 257, 130, 3, 8, lambda x, y, c: (x + 2 * y + c * 40 + N1(x // 16, y // 16, c, 8)) & 255)
    gray = write_png('gray', 70, 33, 1, 8, lambda x, y, c: smooth_noise(x, y, 0))
    graya = write_png('graya', 40, 23, 2, 8, lambda x, y, c: alpha_ramp(x, y, c * 3, 40))
    rgba = write_png('rgba', 64, 31, 4, 8, lambda x, y, c: alpha_ramp(x, y, c, 64))
    rgb16 = write_png('rgb16', 33, 21, 3, 16, lambda x, y, c: (smooth_noise(x, y, c) * 257 + x * 31 + y) & 65535)
    gray16 = write_png('gray16', 29, 17, 1, 16, lambda x, y, c: (x * 2000 + y * 777) & 65535)
    pal = write_png('palette', 90, 41, 3, 8, few_colors)
    text = write_png('text', 120, 60, 3, 8, lambda x, y, c: text_like(x, y, c))
    flt = write_pfm('float', 19, 13, 3, lambda x, y, c: (x + y * 0.5 + c) / 40.0 - 0.1)

    # Lossless modular at every effort and with every predictor/transform.
    for e in [1, 2, 3, 7, 9]:
        add('rgb_e%d' % e, rgb, ['-d', '0', '-e', str(e)], 0)
    add('rgb_e9_i100_g3', rgb, ['-d', '0', '-e', '9', '-I', '100', '-g', '3'], 0)
    for p in range(16):
        add('rgb_pred%d' % p, rgb, ['-d', '0', '-e', '7', '--modular_predictor', str(p)], 0)
    add('rgb_p15', rgb, ['-d', '0', '-e', '7', '-P', '15'], 0)
    add('tiny_e7', tiny, ['-d', '0', '-e', '7'], 0)
    add('odd_e3', odd, ['-d', '0', '-e', '3'], 0)
    add('odd_e9', odd, ['-d', '0', '-e', '9'], 0)
    add('wide_e2', wide, ['-d', '0', '-e', '2'], 0)
    add('wide_e7_g0', wide, ['-d', '0', '-e', '7', '-g', '0'], 0)
    add('big_e1', big, ['-d', '0', '-e', '1'], 0)
    add('big_e3', big, ['-d', '0', '-e', '3'], 0)
    add('big_e7', big, ['-d', '0', '-e', '7'], 0)
    add('gray_e3', gray, ['-d', '0', '-e', '3'], 0)
    add('gray_e9', gray, ['-d', '0', '-e', '9'], 0)
    add('graya_e7', graya, ['-d', '0', '-e', '7'], 0)
    add('rgba_e1', rgba, ['-d', '0', '-e', '1'], 0)
    add('rgba_e7', rgba, ['-d', '0', '-e', '7'], 0)
    add('rgb16_e3', rgb16, ['-d', '0', '-e', '3'], 1)
    add('rgb16_e7', rgb16, ['-d', '0', '-e', '7'], 1)
    add('gray16_e7', gray16, ['-d', '0', '-e', '7'], 1)
    add('palette_e7', pal, ['-d', '0', '-e', '7'], 0)
    add('palette_e9', pal, ['-d', '0', '-e', '9'], 0)
    add('palette_c1', pal, ['-d', '0', '-e', '7', '-C', '1'], 0)
    add('palette_delta', rgb, ['-d', '0', '-e', '7', '--modular_lossy_palette', '--modular_palette_colors=0'], 0)
    add('palette_x', gray, ['-d', '0', '-e', '7', '-X', '100', '-Y', '0'], 0)
    add('palette_y', big, ['-d', '0', '-e', '7', '-Y', '100', '-X', '0'], 0)
    for k in [0, 1, 6, 13, 20, 27, 34, 41]:
        add('rct_c%d' % k, rgb, ['-d', '0', '-e', '3', '-C', str(k)], 0)
    add('prev_channels', rgb, ['-d', '0', '-e', '9', '-E', '3'], 0)
    add('text_e9', text, ['-d', '0', '-e', '9'], 0)
    add('squeeze_rgb', rgb, ['-d', '0', '-e', '7', '--responsive=1'], 0)
    add('squeeze_big', big, ['-d', '0', '-e', '3', '--responsive=1'], 0)
    add('container_rgb', rgb, ['-d', '0', '-e', '7', '--container=1'], 0)
    add('float_e7', flt, ['-d', '0', '-e', '7'], 3, pfm=True)
    add('depth12', rgb16, ['-d', '0', '--override_bitdepth=12'], 1)
    add('premultiplied', rgba, ['-d', '0', '--premultiply=1'], 0)
    add('patches_text', text, ['-d', '0', '-e', '7', '--patches=1'], 0)
    # Center-first group order: permuted TOCs.
    add('big_group_order', big, ['-d', '0', '-e', '3', '--group_order=1'], 0)

    # The float pipeline: XYB, upsampling, blending, animation.
    add_png('lossy_d1', rgb, ['-m', '1', '-d', '1'], 0)
    add_png('lossy_d3_e3', rgb, ['-m', '1', '-d', '3', '-e', '3'], 0)
    add_png('lossy_gray', gray, ['-m', '1', '-d', '2'], 0)
    add_png('lossy_rgba', rgba, ['-m', '1', '-d', '1'], 0)
    smooth_wide = write_png('smooth_wide', 300, 37, 3, 8, lambda x, y, c: (x // 2 + y * 3 + c * 50) & 255)
    add_png('lossy_wide', smooth_wide, ['-m', '1', '-d', '4'], 0)
    add_png('lossy_16', rgb16, ['-m', '1', '-d', '1'], 1)
    add_png('lossy_10', rgb16, ['-m', '1', '-d', '1', '--override_bitdepth=10'], 1)
    small = write_png('small', 30, 20, 3, 8, smooth_noise)
    add_png('epf3_gaborish', rgb, ['-m', '1', '-d', '3', '--epf=3', '--gaborish=1'], 0)
    add_png('epf1', rgb, ['-m', '1', '-d', '2', '--epf=1'], 0)
    add_png('gray_filters', gray, ['-m', '1', '-d', '0', '--epf=2', '--gaborish=1'], 0)
    add_png('upsample2', small, ['-d', '0', '--resampling=2', '--already_downsampled'], 0)
    add_png('upsample4', odd, ['-d', '0', '--resampling=4', '--already_downsampled'], 0)
    add_png('upsample8', tiny, ['-d', '0', '--resampling=8', '--already_downsampled'], 0)
    add_png('ec_upsample', rgba, ['-d', '0', '--ec_resampling=2'], 0)
    frames = [(32, 24, lambda x, y, c, k=k: (x * 8 + k * 40) & 255 if c < 3 else 255) for k in range(3)]
    anim = write_apng('anim', 32, 24, frames, [10, 20, 30])
    add_png('anim', anim, ['-d', '0'], 0, frames=3)
    blend_frames = [(32, 24, lambda x, y, c: [200, 50, 50, 255][c]), (10, 8, lambda x, y, c: [20, 200, 20, 128][c]), (12, 6, lambda x, y, c: [20, 20, 220, 255][c])]
    anim_blend = write_apng('anim_blend', 32, 24, blend_frames, [10, 10, 10], blend=1, dispose=0, offsets=[(0, 0), (5, 4), (15, 12)])
    add_png('anim_blend', anim_blend, ['-d', '0'], 0, frames=3)
    anim_dispose = write_apng('anim_dispose', 32, 24, blend_frames, [10, 10, 10], blend=1, dispose=1, offsets=[(0, 0), (5, 4), (15, 12)])
    add_png('anim_dispose', anim_dispose, ['-d', '0'], 0, frames=3)
    add_png('anim_lossy', anim_blend, ['-m', '1', '-d', '1'], 0, frames=3)

    # VarDCT: distances, efforts, progressive modes, LF frames, odd sizes
    # across groups, gray, alpha, 16-bit, noise, animation, upsampling, EPF.
    ph = write_png('photo', 96, 80, 3, 8, photo)
    ph_small = write_png('photo_small', 64, 48, 3, 8, photo)
    ph_gray = write_png('photo_gray', 96, 80, 1, 8, lambda x, y, c: photo(x, y, 1))
    ph_rgba = write_png('photo_rgba', 80, 60, 4, 8, lambda x, y, c: photo(x, y, c) if c < 3 else (x * 4 + y) & 255)
    ph_16 = write_png('photo_16', 64, 48, 3, 16, lambda x, y, c: photo(x, y, c) * 257 + (x * 7 + y) % 200)
    big_edges = write_png('edges', 272, 56, 3, 8, edges)
    smooth = write_png('smooth', 256, 128, 3, 8, lambda x, y, c: int(127 + 100 * math.sin(x / 90.0 + c) * math.cos(y / 70.0)))
    add_png('vd_d1', ph, ['-d', '1'], 0, jxlref=True)
    add_png('vd_d05', ph_small, ['-d', '0.5'], 0, jxlref=True)
    add_png('vd_d2', ph, ['-d', '2'], 0, jxlref=True)
    add_png('vd_d4', ph, ['-d', '4'], 0, jxlref=True)
    add_png('vd_d8', ph, ['-d', '8'], 0, jxlref=True)
    add_png('vd_d15_large', smooth, ['-d', '15', '-e', '9'], 0, jxlref=True)
    for e in [3, 5, 9]:
        add_png('vd_e%d' % e, ph, ['-d', '1', '-e', str(e)], 0, jxlref=True)
    add_png('vd_progressive', ph, ['-d', '1', '--progressive'], 0, jxlref=True)
    add_png('vd_progressive_dc1', big_edges, ['-d', '2', '--progressive_dc=1'], 0, jxlref=True)
    add_png('vd_progressive_dc2', big_edges, ['-d', '3', '--progressive_dc=2', '-p'], 0, jxlref=True)
    add_png('vd_faster', ph, ['-d', '1', '--faster_decoding=4'], 0, jxlref=True)
    add_png('vd_edges_d2', big_edges, ['-d', '2'], 0, jxlref=True)
    add_png('vd_gray', ph_gray, ['-d', '1'], 0, jxlref=True)
    add_png('vd_rgba', ph_rgba, ['-d', '1'], 0, jxlref=True)
    add_png('vd_16', ph_16, ['-d', '1'], 1, jxlref=True)
    add_png('vd_noise', ph, ['-d', '1', '--photon_noise_iso=3200'], 0, jxlref=True)
    add_png('vd_upsample2', ph, ['-d', '1', '--resampling=2'], 0, jxlref=True)
    add_png('vd_epf3', ph, ['-d', '2', '--epf=3'], 0, jxlref=True)
    add_png('vd_group_order', big_edges, ['-d', '2', '--group_order=1'], 0, jxlref=True)
    vd_frames = [(64, 48, lambda x, y, c, k=k: photo(x + 9 * k, y, c) if c < 3 else 255) for k in range(3)]
    vd_anim = write_apng('vd_anim', 64, 48, vd_frames, [10, 10, 10])
    add_png('vd_anim', vd_anim, ['-d', '1'], 0, frames=3, jxlref=True)
    # JPEG recompression (4:4:4, 4:2:0, 4:2:2, gray).
    for name, fmt, source in [('jpeg444', 'yuvj444p', ph), ('jpeg420', 'yuvj420p', ph), ('jpeg422', 'yuvj422p', ph), ('jpeg_gray', 'gray', ph_gray)]:
        jpg = os.path.join(TMP, name + '.jpg')
        run(['ffmpeg', '-loglevel', 'error', '-y', '-i', source, '-q:v', '4', '-pix_fmt', fmt, jpg])
        add_png(name, jpg, ['--lossless_jpeg=1'], 0, jxlref=True)
    # Splines (written here) and splines with noise-free XYB output.
    spline_set = [((10, 12), [(15, 5), (-3, 8), (2, -10)], [[3, 1], [8, 4, -2], [5, 0, 1], [6, 2]]),
                  ((50, 8), [(-20, 20), (10, 0)], [[-2], [6, 0, 3], [-4, 2], [4, -1, 1]]),
                  ((5, 55), [], [[0], [5], [0], [8]])]
    splines_jxl(os.path.join(OUT, 'splines.jxl'), spline_set, 0)
    add_png('splines', None, [], 0, jxlref=True)
    splines_jxl(os.path.join(OUT, 'splines_q.jxl'), spline_set[:2], -3)
    add_png('splines_q', None, [], 0, jxlref=True)

    # Modular YCbCr with chroma subsampling (4:2:0, 4:2:2, 4:4:0).
    for name, modes in [('ycbcr420', [0, 1, 0]), ('ycbcr422', [0, 2, 0]), ('ycbcr440', [0, 3, 0])]:
        ycbcr_jxl(os.path.join(OUT, name + '.jxl'), 37, 23, modes)
        add_png(name, None, [], 0)
    # More than 16 extra channels (20 alpha channels; the first is shown).
    ycbcr_jxl(os.path.join(OUT, 'many_extra.jxl'), 21, 13, [0, 0, 0], 20)
    add_png('many_extra', None, [], 0)

    # Frame windows: frames larger than the image or starting outside it,
    # and reference-only frames read by patches (written here).
    frame_cases()
    add_file('frame_big')
    add_alias('frame_huge', 'frame_big')
    add_file('frame_negative')
    add_file('ref_patches')
    add_alias('ref_huge', 'ref_patches')
    vardct_window_jxl(os.path.join(OUT, 'vd_window.jxl'), (160, 120), (1400, 1100), (40, 30), 2)
    add_png('vd_window', None, [], 0, jxlref=True)
    hand_written()

    # Metadata and color.
    w, h = 23, 17
    exif = b'MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00\x01\x00\x00\x00\x00\x00\x00'
    xmp = b'XML:com.adobe.xmp\x00\x00\x00\x00\x00<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF/></x:xmpmeta>'
    meta = write_png('meta', w, h, 3, 8, gradient, chunk(b'eXIf', exif) + chunk(b'iTXt', xmp))
    add('meta_brob', meta, ['-d', '0'], 0)
    add('meta_raw', meta, ['-d', '0', '--compress_boxes=0'], 0)
    icc = make_icc()
    open(os.path.join(OUT, 'profile.icc'), 'wb').write(icc)
    iccp = chunk(b'iCCP', b'test\x00\x00' + zlib.compress(icc))
    with_icc = write_png('icc', 31, 19, 3, 8, smooth_noise, iccp)
    add('icc_lossless', with_icc, ['-d', '0'], 0)
    # A 45 KB profile: an ICC stream with many prefix-coded clusters and
    # long LZ77 matches.
    big_icc = make_icc(True)
    open(os.path.join(OUT, 'profile_big.icc'), 'wb').write(big_icc)
    with_big_icc = write_png('icc_big', 31, 19, 3, 8, smooth_noise, chunk(b'iCCP', b'test\x00\x00' + zlib.compress(big_icc)))
    add('icc_big', with_big_icc, ['-d', '0'], 0)
    # XYB with an ICC profile decodes to sRGB.
    add_png('icc_lossy', with_icc, ['-m', '1', '-d', '1'], 0, srgb=True)

    with open(os.path.join(OUT, 'cases.dodo'), 'w') as f:
        f.write('package jxl_test\n\n')
        f.write('// Generated by tests/image/jxl/generate.py. Per case: the .jxl name, the\n')
        f.write('// reference PNG name (compare cases), then width, height, channels, native\n')
        f.write('// sample code, kind (0: FNV-1a hash of the native samples, 1: compare with\n')
        f.write('// the PNG, 2: compare with the lossless reference NAME.jxl), displayed\n')
        f.write('// frame index, extra channel (kind 3: compare extra channel E with the\n')
        f.write('// 16-bit reference NAME.jxl); and the hash for kind 0.\n\n')
        f.write('const CASE_COUNT: usize = %d\n\n' % len(CASES))
        f.write('static CASE_FILES: [CASE_COUNT]&str = [\n')
        for c in CASES:
            f.write('    "%s",\n' % c[0])
        f.write(']\n\n')
        f.write('static CASE_REFS: [CASE_COUNT]&str = [\n')
        for c in CASES:
            f.write('    "%s",\n' % c[1])
        f.write(']\n\n')
        f.write('static CASE_SHAPES: [%d]u32 = [\n' % (len(CASES) * 7))
        for c in CASES:
            ec = c[9] if len(c) > 9 else 0
            f.write('    %d, %d, %d, %d, %d, %d, %d,\n' % (c[2], c[3], c[4], c[5], c[6], c[7], ec))
        f.write(']\n\n')
        f.write('static CASE_HASHES: [CASE_COUNT]u64 = [\n')
        for c in CASES:
            f.write('    0x%016x,\n' % c[8])
        f.write(']\n')
    print('%d cases' % len(CASES))

main()
