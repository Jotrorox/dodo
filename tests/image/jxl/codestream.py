"""A small hand-written JPEG XL codestream writer for decoder fixtures that
cjxl never emits (used by generate.py).

Every entropy-coded stream uses one cluster with a 32-symbol prefix code of
5-bit codes and hybrid uint split 0, so token t > 0 stands for 2^(t-1) plus
t - 1 raw bits. Modular images use one global MA tree with a single leaf and the
zero predictor, so the residuals are the samples. VarDCT frames use the
default quantizer tables, block contexts and coefficient orders; their
coefficients are arbitrary small values (djxl's decode is the reference).
"""
import struct

DIM = [(1, 9), (1, 13), (1, 18), (1, 30)]
UPS = [(1, 0), (2, 0), (4, 0), (8, 0)]
CROP = [(0, 8), (256, 11), (2304, 14), (18688, 30)]
TOC = [(0, 10), (1024, 14), (17408, 22), (4211712, 30)]


class Writer:
    def __init__(self):
        self.bits = []

    def u(self, n, v):
        assert 0 <= v < (1 << n) or n == 0 and v == 0, (n, v)
        for i in range(n):
            self.bits.append((v >> i) & 1)

    def flag(self, b):
        self.u(1, 1 if b else 0)

    def u32(self, v, dists):
        for sel, (off, n) in enumerate(dists):
            if off <= v < off + (1 << n):
                self.u(2, sel)
                self.u(n, v - off)
                return
        raise ValueError('U32 %d does not fit %s' % (v, dists))

    def u64(self, v):
        if v == 0:
            self.u(2, 0)
        elif v <= 16:
            self.u(2, 1); self.u(4, v - 1)
        elif v <= 272:
            self.u(2, 2); self.u(8, v - 17)
        else:
            self.u(2, 3); self.u(12, v & 4095)
            v >>= 12
            shift = 12
            while v:
                self.u(1, 1)
                if shift == 60:
                    self.u(4, v & 15)
                    v = 0
                    break
                self.u(8, v & 255)
                v >>= 8
                shift += 8
            self.u(1, 0)

    def enum(self, v):
        self.u32(v, [(0, 0), (1, 0), (2, 4), (18, 6)])

    def f16(self, x):
        self.u(16, struct.unpack('<H', struct.pack('<e', x))[0])

    def pad(self):
        while len(self.bits) % 8:
            self.bits.append(0)

    def extend(self, other):
        self.bits.extend(other.bits)

    def data(self):
        self.pad()
        return bytes(sum(b << k for k, b in enumerate(self.bits[i:i + 8])) for i in range(0, len(self.bits), 8))

    # Entropy coding: one cluster, 32 symbols of 5 bits, split 0.
    def code(self, ndist):
        self.u(1, 0)                          # no LZ77
        if ndist > 1:
            self.u(1, 1); self.u(2, 0)        # simple context map: cluster 0
        self.u(1, 1)                          # prefix codes
        self.u(4, 0)                          # hybrid config split 0
        self.u(1, 1); self.u(4, 4); self.u(4, 15)  # 32 symbols
        self.u(2, 0)                          # HSKIP 0
        lengths = {0: [0, 0], 1: [1, 1, 1, 0]}
        for v in [0, 0, 0, 0, 0, 1] + [0] * 12:  # only code length 5 used
            self.bits.extend(lengths[v])

    def token(self, v):
        def sym(t):
            for i in range(4, -1, -1):
                self.bits.append((t >> i) & 1)
        assert v >= 0
        if v == 0:
            sym(0)
            return
        n = v.bit_length() - 1
        assert n + 1 < 32
        sym(n + 1)
        self.u(n, v - (1 << n))

    def signed(self, v):
        self.token(2 * v if v >= 0 else -2 * v - 1)


class EC:
    """An extra channel description."""
    def __init__(self, kind=0, bits=8, exp_bits=0, dim_shift=0, name=b'', associated=False, spot=(0.0, 0.0, 0.0, 0.0), cfa=1, default=False):
        self.kind, self.bits, self.exp_bits, self.dim_shift = kind, bits, exp_bits, dim_shift
        self.name, self.associated, self.spot, self.cfa, self.default = name, associated, spot, cfa, default


def bit_depth(w, bits, exp_bits=0):
    if exp_bits:
        w.u(1, 1)
        w.u32(bits, [(32, 0), (16, 0), (24, 0), (1, 6)])
        w.u(4, exp_bits - 1)
    else:
        w.u(1, 0)
        w.u32(bits, [(8, 0), (10, 0), (12, 0), (1, 6)])


def image_header(w, width, height, bits=8, xyb=False, ecs=(), gray=False, animated=False):
    w.u(8, 0xFF); w.u(8, 0x0A)
    w.u(1, 0); w.u32(height, DIM); w.u(3, 0); w.u32(width, DIM)
    w.u(1, 0)                                 # metadata not all default
    w.flag(animated)                          # extra fields
    if animated:
        w.u(3, 0)                             # orientation 1
        w.u(1, 0); w.u(1, 0)                  # no intrinsic size, no preview
        w.u(1, 1)                             # animation
        w.u32(100, [(100, 0), (1000, 0), (1, 10), (1, 30)])
        w.u32(1, [(1, 0), (1001, 0), (1, 8), (1, 10)])
        w.u32(0, [(0, 0), (0, 3), (0, 16), (0, 32)])
        w.u(1, 0)                             # no timecodes
    bit_depth(w, bits)
    w.u(1, 1)                                 # 16-bit modular buffers
    w.u32(len(ecs), [(0, 0), (1, 0), (2, 4), (1, 12)])
    for e in ecs:
        if e.default:
            w.u(1, 1)
            continue
        w.u(1, 0)
        w.enum(e.kind)
        bit_depth(w, e.bits, e.exp_bits)
        w.u32(e.dim_shift, [(0, 0), (3, 0), (4, 0), (1, 3)])
        w.u32(len(e.name), [(0, 0), (0, 4), (16, 5), (48, 10)])
        for b in e.name:
            w.u(8, b)
        if e.kind == 0:
            w.flag(e.associated)
        elif e.kind == 2:
            for v in e.spot:
                w.f16(v)
        elif e.kind == 5:
            w.u32(e.cfa, [(1, 0), (0, 2), (3, 4), (19, 8)])
    w.flag(xyb)
    if gray:
        w.u(1, 0); w.u(1, 0); w.enum(1)       # colour encoding: grey
        w.enum(1)                             # D65
        w.u(1, 0); w.enum(13); w.enum(1)      # sRGB transfer, relative
    else:
        w.u(1, 1)                             # sRGB
    if animated:
        w.u(1, 1)                             # default tone mapping
    w.u(2, 0)                                 # no extensions
    w.u(1, 1)                                 # default transform data
    w.pad()


def blending(w, nec, partial, mode=0, alpha=0, clamp=False, source=0):
    w.u32(mode, [(0, 0), (1, 0), (2, 0), (3, 2)])
    uses_alpha = nec > 0 and mode in (2, 3)
    if uses_alpha:
        w.u32(alpha, [(0, 0), (1, 0), (2, 0), (3, 3)])
    if uses_alpha or mode == 4:
        w.flag(clamp)
    if mode != 0 or partial:
        w.u(2, source)


def frame_header(w, ecs, xyb=False, ftype=0, modular=True, flags=0, ycbcr=False, jup=(0, 0, 0), up=1, ec_up=None,
                 group_shift=1, qm=(3, 2), lf_level=1, crop=None, image_size=None, blend=None, ec_blend=None,
                 duration=None, is_last=True, save_as=0, save_before_ct=False, gab=False, gab_weights=None,
                 epf=0, epf_sigma=1.0):
    nec = len(ecs)
    w.u(1, 0)
    w.u(2, ftype); w.flag(modular); w.u64(flags)
    if not xyb:
        w.flag(ycbcr)
    use_lf = (flags & 32) != 0
    if ycbcr and not use_lf:
        for m in jup:
            w.u(2, m)
    if not use_lf:
        w.u32(up, UPS)
        for i, e in enumerate(ecs):
            total = ec_up[i] if ec_up else (1 << e.dim_shift)
            w.u32(total >> e.dim_shift, UPS)
    if modular:
        w.u(2, group_shift)
    if not modular and xyb:
        w.u(3, qm[0]); w.u(3, qm[1])
    if ftype != 2:
        w.u32(1, [(1, 0), (2, 0), (3, 0), (4, 3)])  # one pass
    if ftype == 1:
        w.u32(lf_level, [(1, 0), (2, 0), (3, 0), (4, 0)])
    partial = False
    if ftype != 1:
        w.flag(crop is not None)
        if crop is not None:
            x0, y0, cw, chh = crop
            if ftype != 2:
                w.u32(2 * x0 if x0 >= 0 else -2 * x0 - 1, CROP)
                w.u32(2 * y0 if y0 >= 0 else -2 * y0 - 1, CROP)
            w.u32(cw, CROP); w.u32(chh, CROP)
            iw, ih = image_size
            partial = ftype in (0, 3) and (x0 > 0 or y0 > 0 or x0 + cw < iw or y0 + chh < ih)
    normal = ftype in (0, 3)
    if normal:
        b = blend or (0, 0, False, 0)
        blending(w, nec, partial, *b)
        for i in range(nec):
            eb = ec_blend[i] if ec_blend else (0, 0, False, 0)
            blending(w, nec, partial, *eb)
        if duration is not None:
            w.u32(duration, [(0, 0), (1, 0), (0, 8), (0, 32)])
        w.flag(is_last)
    else:
        is_last = False
    if ftype != 1 and not is_last:
        w.u(2, save_as)
    dur = duration or 0
    can_ref = not is_last and ftype != 1 and (dur == 0 or save_as != 0)
    if can_ref and (blend is None or blend[0] == 0) and not partial and normal:
        w.flag(save_before_ct)
    elif ftype == 2:
        w.flag(save_before_ct)
    w.u(2, 0)                                 # no name
    w.u(1, 0)                                 # restoration filter
    w.flag(gab)
    if gab:
        w.flag(gab_weights is not None)
        if gab_weights is not None:
            for v in gab_weights:
                w.f16(v)
    w.u(2, epf)
    if epf:
        if not modular:
            w.u(1, 0)                         # default sharpness LUT
        w.u(1, 0)                             # default channel scale
        w.u(1, 0)                             # default sigma parameters
        if modular:
            w.f16(epf_sigma)
    w.u(2, 0)                                 # filter extensions
    w.u(2, 0)                                 # frame extensions


def toc(w, sizes):
    w.u(1, 0)                                 # not permuted
    w.pad()
    for size in sizes:
        w.u32(size, TOC)
    w.pad()


def global_tree(s, predictor=0):
    """GlobalModular's tree: one leaf with the zero (0) or gradient (5)
    predictor."""
    s.u(1, 1)
    s.code(6)
    s.token(0)                                # leaf
    s.token(predictor)
    s.token(0); s.token(0); s.token(0)        # offset 0, multiplier 1
    s.code(1)


def predict(rows, x, y, predictor):
    if predictor == 0:
        return 0
    left = rows[y][x - 1] if x > 0 else (rows[y - 1][x] if y > 0 else 0)
    top = rows[y - 1][x] if y > 0 else left
    topleft = rows[y - 1][x - 1] if x > 0 and y > 0 else left
    grad = left + top - topleft
    return max(min(left, top), min(max(left, top), grad))


def modular_stream(s, channels, predictor=0):
    """A stream using the global tree: header, then every channel's samples
    (lists of rows) as residuals of the tree's predictor."""
    s.u(1, 1); s.u(1, 1); s.u(2, 0)           # global tree, default WP, no transforms
    for rows in channels:
        for y, row in enumerate(rows):
            for x, v in enumerate(row):
                s.signed(v - predict(rows, x, y, predictor))


def patch_dictionary(s, nec, refs):
    """refs: list of (slot, rx, ry, pw, ph, [(x, y, [(mode, alpha, clamp)] * (1 + nec))])."""
    s.code(10)
    s.token(len(refs))
    for slot, rx, ry, pw, ph, places in refs:
        s.token(slot); s.token(rx); s.token(ry); s.token(pw - 1); s.token(ph - 1)
        s.token(len(places) - 1)
        last = None
        for x, y, modes in places:
            if last is None:
                s.token(x); s.token(y)
            else:
                s.signed(x - last[0]); s.signed(y - last[1])
            last = (x, y)
            assert len(modes) == 1 + nec
            for mode, alpha, clamp in modes:
                s.token(mode)
                if mode in (4, 5, 6, 7) and nec > 1:
                    s.token(alpha)
                if mode in (3, 4, 5, 6, 7):
                    s.token(1 if clamp else 0)


def splines(s, spline_list, qadj):
    s.code(6)
    s.token(len(spline_list) - 1)
    last = (0, 0)
    for i, sp in enumerate(spline_list):
        x, y = sp[0]
        if i == 0:
            s.token(x); s.token(y)
        else:
            s.signed(x - last[0]); s.signed(y - last[1])
        last = (x, y)
    s.signed(qadj)
    for start, deltas, dct in spline_list:
        s.token(len(deltas))
        for dx, dy in deltas:
            s.signed(dx); s.signed(dy)
        for c in range(4):
            for v in dct[c] + [0] * (32 - len(dct[c])):
                s.signed(v)


def ceil_div(a, b):
    return (a + b - 1) // b


def write_modular(path, width, height, frames, bits=8, xyb=False, ecs=(), gray=False, animated=False, predictor=5):
    """frames: list of dicts with frame_header() arguments plus 'channels'
    (lists of rows, color then extra channels at their coded sizes),
    optional 'patches' (refs) and 'splines' ((list, qadj))."""
    w = Writer()
    image_header(w, width, height, bits, xyb, ecs, gray, animated)
    for fr in frames:
        args = dict(fr)
        channels = args.pop('channels')
        refs = args.pop('patches', None)
        spl = args.pop('splines', None)
        flags = args.pop('flags', 0)
        if refs is not None:
            flags |= 2
        if spl is not None:
            flags |= 16
        frame_header(w, ecs, xyb=xyb, flags=flags, **args)
        s = Writer()
        if refs is not None:
            patch_dictionary(s, len(ecs), refs)
        if spl is not None:
            splines(s, spl[0], spl[1])
        s.u(1, 1)                             # default LF dequantization
        global_tree(s, predictor)
        modular_stream(s, channels, predictor)
        section = s.data()
        toc(w, [len(section)])
        w.bits.extend(bits_of(section))
    open(path, 'wb').write(w.data())


def bits_of(data):
    out = []
    for b in data:
        for i in range(8):
            out.append((b >> i) & 1)
    return out


# VarDCT -------------------------------------------------------------------

# DctSelect: (blocks wide, blocks high, coefficient order index).
DCT8, HORNUSS, DCT2, DCT4 = 0, 1, 2, 3
DS_SHAPE = {0: (1, 1), 1: (1, 1), 2: (1, 1), 3: (1, 1), 4: (2, 2), 5: (4, 4), 6: (2, 1), 7: (1, 2),
            12: (1, 1), 13: (1, 1), 14: (1, 1), 15: (1, 1), 16: (1, 1), 17: (1, 1)}


def vardct_frame(w, ecs, xyb, width, height, layout, rng, lf_values=None, use_lf=False, cfl=None,
                 global_scale=2048, quant_lf=16, coeff_range=6, density=0.3, ec_channels=(), **header):
    """One VarDCT frame (one group, one pass, so one TOC section).
    layout: dict (bx, by) -> DctSelect of the first block of each varblock,
    covering the block grid. lf_values(c, x, y): quantized LF sample of
    channel c (0 X/Cb, 1 Y, 2 B/Cr) at its (subsampled) block position."""
    flags = header.pop('flags', 0)
    if use_lf:
        flags |= 32
    frame_header(w, ecs, xyb=xyb, modular=False, flags=flags, **header)
    up = header.get('up', 1)
    jup = header.get('jup', (0, 0, 0)) if header.get('ycbcr') else (0, 0, 0)
    hs_of, vs_of = [0, 1, 1, 0], [0, 1, 0, 1]
    maxh = max(hs_of[m] for m in jup)
    maxv = max(vs_of[m] for m in jup)
    hs = [maxh - hs_of[m] for m in jup]
    vs = [maxv - vs_of[m] for m in jup]
    sw, sh = ceil_div(width, up), ceil_div(height, up)
    bw = (ceil_div(sw, 8 << maxh)) << maxh
    bh = (ceil_div(sh, 8 << maxv)) << maxv
    assert bw <= 32 and bh <= 32
    s = Writer()
    s.u(1, 1)                                 # default LF dequantization
    s.u32(global_scale, [(1, 11), (2049, 11), (4097, 12), (8193, 16)])
    s.u32(quant_lf, [(16, 0), (1, 5), (1, 8), (1, 16)])
    s.u(1, 1)                                 # default block context map
    if cfl is None:
        s.u(1, 1)
    else:
        factor, base_x, base_b, xlf, blf = cfl[:5]
        s.u(1, 0)
        s.u32(factor, [(84, 0), (256, 0), (2, 8), (258, 16)])
        s.f16(base_x); s.f16(base_b)
        s.u(8, xlf + 128); s.u(8, blf + 128)
    global_tree(s)
    if ec_channels:
        modular_stream(s, ec_channels)
    # LF group.
    if not use_lf:
        s.u(2, 0)                             # extra precision
        lf = []
        for c in (1, 0, 2):
            lf.append([[lf_values(c, x, y) for x in range(bw >> hs[c])] for y in range(bh >> vs[c])])
        modular_stream(s, lf)
    blocks = sorted(layout.items(), key=lambda kv: (kv[0][1], kv[0][0]))
    covered = set()
    for (bx, by), ds in blocks:
        cw, chh = DS_SHAPE[ds]
        for dy in range(chh):
            for dx in range(cw):
                assert (bx + dx, by + dy) not in covered and bx + dx < bw and by + dy < bh
                covered.add((bx + dx, by + dy))
    assert len(covered) == bw * bh
    nb = len(blocks)
    s.u(max(0, (bw * bh - 1).bit_length()), nb - 1)
    tiles_x, tiles_y = ceil_div(bw, 8), ceil_div(bh, 8)
    cfl_x = cfl[5] if cfl and len(cfl) > 5 else [[0] * tiles_x for _ in range(tiles_y)]
    cfl_b = cfl[6] if cfl and len(cfl) > 6 else [[0] * tiles_x for _ in range(tiles_y)]
    strategies = [ds for _, ds in blocks]
    mults = [rng.randrange(0, 6) for _ in blocks]
    sharp = [[rng.randrange(0, 8) for _ in range(bw)] for _ in range(bh)]
    modular_stream(s, [cfl_x, cfl_b, [strategies, mults], sharp])
    # HfGlobal, HfPass.
    s.u(1, 1)                                 # default dequantization matrices
    s.u(2, 2)                                 # no custom coefficient orders
    s.code(495 * 15)
    # The group's coefficients.
    for (bx, by), ds in blocks:
        cw, chh = DS_SHAPE[ds]
        size = cw * chh * 64
        llf = cw * chh
        for c in (1, 0, 2):
            if ((bx >> hs[c]) << hs[c]) != bx or ((by >> vs[c]) << vs[c]) != by:
                continue
            values = []
            for k in range(llf, size):
                if rng.random() < density * (1.0 - (k - llf) / size):
                    v = rng.randrange(1, coeff_range + 1) * rng.choice((-1, 1))
                    values.append(v)
                else:
                    values.append(0)
            while values and values[-1] == 0:
                values.pop()
            nz = sum(1 for v in values if v)
            s.token(nz)
            for v in values:
                s.signed(v)
    section = s.data()
    toc(w, [len(section)])
    w.bits.extend(bits_of(section))
