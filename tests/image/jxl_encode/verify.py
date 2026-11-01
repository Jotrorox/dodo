#!/usr/bin/env python3
"""Round-trip check of the Dodo JPEG XL encoder against libjxl's djxl.

Generates test images (gradients, noise, few colors, text-like edges; gray,
gray+alpha, RGB, RGBA; 8 and 16-bit, F16 and F32; odd sizes), encodes each
at every effort and container setting with a small Dodo program, decodes the
result with djxl and compares every sample exactly. Also reports sizes
against `cjxl -d 0 -e 1/2/3`.

Usage: python3 tests/image/jxl_encode/verify.py [--quick] [--sizes-only]
       [--image FILE.ppm|.pgm|.pam ...]
Needs djxl/cjxl (libjxl) on PATH; run `cargo build` first.
"""
import argparse
import os
import random
import shutil
import struct
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
DODO = os.path.join(ROOT, "target", "debug", "dodo")

PROGRAM = r'''
package main

import "core/mem"
import "std/console"
import "std/fs"
import "std/image/color"
import "std/image/jxl/encode"
import "std/image/pixels"
import "std/platform"

static mut JOBS: [1048576]u8 = [0u8; 1048576]
static mut PIX: [67108864]u8 = [0u8; 67108864]
static mut OUT: [201326592]u8 = [0u8; 201326592]
static mut SCRATCH: [16777216]u8 = [0u8; 16777216]

const EXIF: [26]u8 = [0x4D, 0x4D, 0, 0x2A, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]
const XMP: [44]u8 = [0x3C, 0x78, 0x3A, 0x78, 0x6D, 0x70, 0x6D, 0x65, 0x74, 0x61, 0x20, 0x78, 0x6D, 0x6C, 0x6E, 0x73, 0x3A, 0x78, 0x3D, 0x22, 0x61, 0x64, 0x6F, 0x62, 0x65, 0x3A, 0x6E, 0x73, 0x3A, 0x6D, 0x65, 0x74, 0x61, 0x2F, 0x22, 0x3E, 0x3C, 0x2F, 0x78, 0x3A, 0x78, 0x6D, 0x70, 0x3E]

fn say(text: &str) {
    match console.println(text) {
        ok(_) => {}, err(_) => {}
    }
}

fn say_num(prefix: &str, n: usize) {
    match console.printf("{} {}\n", prefix, n) {
        ok(_) => {}, err(_) => {}
    }
}

fn number(field: &[u8]) -> usize {
    n := 0usize
    for i in 0usize..field.len {
        n = n * 10 + (field[i] - 48) as usize
    }
    return n
}

fn run_job(line: &[u8]) -> i32 {
    // in out width height channels sample premultiplied effort container meta
    starts := [0usize; 12]
    ends := [0usize; 12]
    count := 0usize
    i := 0usize
    for i < line.len && count < 12 {
        for i < line.len && line[i] == 32 {
            i += 1
        }
        if i >= line.len {
            break
        }
        starts[count] = i
        for i < line.len && line[i] != 32 {
            i += 1
        }
        ends[count] = i
        count += 1
    }
    if count < 10 {
        say("err BadJob")
        return 1
    }
    input := unsafe { mem.str_from_utf8(&line[starts[0]..ends[0]]) }
    output := unsafe { mem.str_from_utf8(&line[starts[1]..ends[1]]) }
    width := number(&line[starts[2]..ends[2]]) as u32
    height := number(&line[starts[3]..ends[3]]) as u32
    channels := number(&line[starts[4]..ends[4]])
    sample := number(&line[starts[5]..ends[5]]) as u32
    premultiplied := number(&line[starts[6]..ends[6]]) == 1
    effort := number(&line[starts[7]..ends[7]]) as u32
    container := number(&line[starts[8]..ends[8]]) == 1
    meta := number(&line[starts[9]..ends[9]])
    layout := pixels.Layout.from_codes(channels, sample, premultiplied)
    workspace := platform.workspace()
    pix := unsafe { &mut PIX }
    got := match fs.read_file(input, &mut pix[..], &mut workspace) {
        ok(report) => {
            report.read
        }, err(_) => {
            say("err ReadFailed")
            return 1
        }
    }
    options := encode.EncodeOptions.new()
    options.effort = effort
    options.container = container
    if meta == 2 {
        options.color = color.ColorInfo.linear()
        options.orientation = color.Orientation.Rotate90
    }
    view := match pixels.View.new(&pix[..got], width, height, layout.clone()) {
        ok(v) => {
            v
        }, err(e) => {
            say(pixels.error_name(e))
            return 1
        }
    }
    md := encode.Metadata.new(&EXIF, &XMP)
    bound := 0usize
    if meta == 1 {
        bound = match encode.max_encoded_size_with_metadata(width, height, &layout, &options, &md) {
            ok(n) => {
                n
            }, err(_) => {
                0
            }
        }
    } else {
        bound = match encode.max_encoded_size(width, height, &layout, &options) {
            ok(n) => {
                n
            }, err(_) => {
                0
            }
        }
    }
    need := match encode.encode_scratch_size(width, height, &layout, &options) {
        ok(n) => {
            n
        }, err(_) => {
            0
        }
    }
    out := unsafe { &mut OUT }
    scratch := unsafe { &mut SCRATCH }
    if need > scratch.len || bound > out.len {
        say("err BuffersTooSmallInHarness")
        return 1
    }
    n := 0usize
    if meta == 1 {
        n = match encode.encode_with_metadata(&view, &options, &md, &mut out[..bound], &mut scratch[..need]) {
            ok(n) => {
                n
            }, err(e) => {
                say(pixels.error_name(e))
                return 1
            }
        }
    } else {
        n = match encode.encode(&view, &options, &mut out[..bound], &mut scratch[..need]) {
            ok(n) => {
                n
            }, err(e) => {
                say(pixels.error_name(e))
                return 1
            }
        }
    }
    match fs.write_file(output, &out[..n], &mut workspace) {
        ok() => {}, err(_) => {
            say("err WriteFailed")
            return 1
        }
    }
    match console.printf("ok {} {} {}\n", n, bound, need) {
        ok(_) => {}, err(_) => {}
    }
    return 0
}

fn main() -> i32 {
    workspace := platform.workspace()
    // "jobs.txt" (a literal would be static storage, conflicting with JOBS).
    name := [106u8, 111, 98, 115, 46, 116, 120, 116]
    path := unsafe { mem.str_from_utf8(&name) }
    jobs := unsafe { &mut JOBS }
    size := match fs.read_file(path, &mut jobs[..], &mut workspace) {
        ok(report) => {
            report.read
        }, err(_) => {
            say("cannot read jobs.txt")
            return 1
        }
    }
    start := 0usize
    failures := 0i32
    for i in 0usize..size {
        if jobs[i] == 10 {
            if i > start {
                failures += run_job(&jobs[start..i])
            }
            start = i + 1
        }
    }
    return 0
}
'''

# ---------------------------------------------------------------------------
# Test images: lists of rows of pixels, each pixel a tuple of channel values
# (ints 0..maxval, or floats for F16/F32).

def half_bits(value):
    return struct.unpack("<H", struct.pack("<e", value))[0]

def float_bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]

def gen(kind, w, h, channels, maxval, seed):
    rnd = random.Random(seed)
    pix = []
    if kind == "few":
        palette = [tuple(rnd.randrange(maxval + 1) for _ in range(channels)) for _ in range(5)]
    for y in range(h):
        row = []
        for x in range(w):
            p = []
            for c in range(channels):
                if kind == "gradient":
                    v = ((x * (c + 1) + y * (5 - c)) * maxval) // max(1, 4 * w + 5 * h)
                elif kind == "noise":
                    v = rnd.randrange(maxval + 1)
                elif kind == "few":
                    v = palette[((x // 7) + (y // 5) * 3) % 5][c]
                elif kind == "text":
                    on = ((x // 2) % 5 < 2 and (y // 3) % 4 == 1) or (x + y) % 23 == 0
                    v = maxval if not on else (maxval // 8 if c % 2 == 0 else maxval // 3)
                elif kind == "smooth":
                    import math
                    v = int((0.5 + 0.25 * math.sin(x / 9.0 + c) + 0.24 * math.cos(y / 13.0 - c)) * maxval)
                    v = max(0, min(maxval, v + rnd.randrange(-2, 3)))
                else:
                    raise ValueError(kind)
                if c == channels - 1 and channels in (2, 4) and kind != "noise":
                    # alpha: mostly opaque with a soft gradient
                    v = maxval if (x + y) % 11 else (x * maxval) // max(1, w - 1)
                p.append(max(0, min(maxval, v)))
            row.append(tuple(p))
        pix.append(row)
    return pix

def raw_bytes(pix, sample):
    out = bytearray()
    for row in pix:
        for p in row:
            for v in p:
                if sample == 0:
                    out.append(v)
                elif sample == 1:
                    out += struct.pack("<H", v)
                elif sample == 2:
                    out += struct.pack("<H", half_bits(v))
                else:
                    out += struct.pack("<I", float_bits(v))
    return bytes(out)

# ---------------------------------------------------------------------------
# Decoded-image readers.

def read_pam(path):
    data = open(path, "rb").read()
    if data[:2] in (b"P5", b"P6"):
        parts = []
        pos = 2
        while len(parts) < 3:
            while data[pos:pos + 1].isspace():
                pos += 1
            if data[pos:pos + 1] == b"#":
                while data[pos:pos + 1] != b"\n":
                    pos += 1
                continue
            start = pos
            while not data[pos:pos + 1].isspace():
                pos += 1
            parts.append(int(data[start:pos]))
        pos += 1
        w, h, maxval = parts
        depth = 1 if data[:2] == b"P5" else 3
        return w, h, depth, maxval, data[pos:]
    assert data[:2] == b"P7", data[:2]
    header_end = data.index(b"ENDHDR\n") + 7
    fields = {}
    for line in data[:header_end].split(b"\n")[1:]:
        p = line.split()
        if len(p) >= 2:
            fields[p[0]] = p[1]
    return int(fields[b"WIDTH"]), int(fields[b"HEIGHT"]), int(fields[b"DEPTH"]), int(fields[b"MAXVAL"]), data[header_end:]

def read_npy(path):
    data = open(path, "rb").read()
    assert data[:6] == b"\x93NUMPY"
    major = data[6]
    if major == 1:
        hlen = struct.unpack("<H", data[8:10])[0]
        start = 10
    else:
        hlen = struct.unpack("<I", data[8:12])[0]
        start = 12
    header = data[start:start + hlen].decode()
    body = data[start + hlen:]
    d = eval(header)
    shape = d["shape"]
    dtype = d["descr"]
    assert dtype in ("<f4", "<f2"), dtype
    size = 4 if dtype == "<f4" else 2
    count = 1
    for s in shape:
        count *= s
    fmt = "<%d%s" % (count, "f" if size == 4 else "e")
    return shape, struct.unpack(fmt, body[:count * size])

def compare(job, decoded_path):
    w, h, channels, sample = job["w"], job["h"], job["channels"], job["sample"]
    pix = job["pix"]
    if job["meta"] == 2:
        # Encoded with orientation 6 (rotate 90 degrees clockwise), which
        # djxl applies.
        pix = [[pix[h - 1 - dx][dy] for dx in range(h)] for dy in range(w)]
        w, h = h, w
    if sample >= 2:
        shape, values = read_npy(decoded_path)
        # (frames, height, width, channels)
        ch = shape[-1]
        if ch != channels:
            return "channel count %d != %d" % (ch, channels)
        i = 0
        for y in range(h):
            for x in range(w):
                for c in range(channels):
                    want = pix[y][x][c]
                    got = values[i]
                    i += 1
                    if sample == 2:
                        ok = half_bits(got) == half_bits(want) or (want != want and got != got)
                    else:
                        ok = float_bits(got) == float_bits(want) or (want != want and got != got)
                    if not ok:
                        return "pixel (%d,%d,%d): %r != %r" % (x, y, c, got, want)
        return None
    dw, dh, depth, maxval, body = read_pam(decoded_path)
    if (dw, dh) != (w, h):
        return "size %dx%d != %dx%d" % (dw, dh, w, h)
    if depth != channels:
        return "depth %d != %d" % (depth, channels)
    want_max = 255 if sample == 0 else 65535
    if maxval != want_max:
        return "maxval %d != %d" % (maxval, want_max)
    size = 1 if maxval < 256 else 2
    i = 0
    for y in range(h):
        for x in range(w):
            for c in range(channels):
                if size == 1:
                    got = body[i]
                else:
                    got = (body[i] << 8) | body[i + 1]
                i += size
                if got != pix[y][x][c]:
                    return "pixel (%d,%d,%d): %d != %d" % (x, y, c, got, pix[y][x][c])
    return None

# ---------------------------------------------------------------------------
# Golden files of encode_test.dodo: the Python twin of its fill().

GOLDEN_IMAGES = {
    "gradient-rgb8": (16, 8, 3, 1, 0),
    "few-rgba8": (24, 16, 4, 1, 1),
    "noise-gray16": (9, 5, 1, 2, 2),
    "edges-ga8": (7, 3, 2, 1, 3),
    "wide-rgb8": (300, 20, 3, 1, 0),
    "tiny-gray8": (1, 1, 1, 1, 0),
}

def golden_pixels(w, h, channels, size, pattern):
    seed = 12345
    pix = []
    for y in range(h):
        row = []
        for x in range(w):
            p = []
            for c in range(channels):
                if pattern == 0:
                    v = (x * (c + 2) + y * (5 - c)) * 7
                elif pattern == 1:
                    v = (((x // 5) + (y // 4) * 3) % 5) * 53 + c * 31
                elif pattern == 2:
                    seed = (seed * 1103515245 + 12345) & 0xFFFFFFFF
                    v = seed >> 8
                else:
                    v = 250 if (x + 2 * y) % 5 < 2 else c * 40
                p.append(v & (0xFF if size == 1 else 0xFFFF))
            row.append(tuple(p))
        pix.append(row)
    return pix

def check_goldens(work):
    """Decode every committed golden file with djxl and compare."""
    folder = os.path.join(os.path.dirname(os.path.abspath(__file__)), "golden")
    failures = 0
    count = 0
    for name in sorted(os.listdir(folder)):
        if not name.endswith(".jxl"):
            continue
        base = name.rsplit("-e", 1)[0]
        w, h, channels, size, pattern = GOLDEN_IMAGES[base]
        job = dict(w=w, h=h, channels=channels, sample=0 if size == 1 else 1, meta=0, pix=golden_pixels(w, h, channels, size, pattern))
        dec = os.path.join(work, name + ".pam")
        d = subprocess.run(["djxl", os.path.join(folder, name), dec, "--quiet"], capture_output=True, text=True)
        count += 1
        if d.returncode != 0:
            print("FAIL golden djxl %s: %s" % (name, d.stderr.strip()[-300:]))
            failures += 1
            continue
        problem = compare(job, dec)
        if problem:
            print("FAIL golden %s: %s" % (name, problem))
            failures += 1
    print("%d golden files, %d failures" % (count, failures))
    return failures

def build_program(work, opt="0"):
    src = os.path.join(work, "prog")
    os.makedirs(src, exist_ok=True)
    with open(os.path.join(src, "main.dodo"), "w") as f:
        f.write(PROGRAM)
    exe = os.path.join(work, "encoder")
    subprocess.run([DODO, "build", os.path.join(src, "main.dodo"), "-O", opt, "-o", exe], check=True, timeout=600)
    return exe

def cjxl_size(path_png_or_pnm, effort, work):
    out = os.path.join(work, "ref.jxl")
    r = subprocess.run(["cjxl", path_png_or_pnm, out, "-d", "0", "-e", str(effort), "--quiet"], capture_output=True)
    if r.returncode != 0:
        return None
    return os.path.getsize(out)

def write_pnm(path, pix, w, h, channels, sample):
    maxval = 255 if sample == 0 else 65535
    if channels in (1, 3):
        header = b"P5" if channels == 1 else b"P6"
        data = bytearray(header + b"\n%d %d\n%d\n" % (w, h, maxval))
    else:
        tupl = b"GRAYSCALE_ALPHA" if channels == 2 else b"RGB_ALPHA"
        data = bytearray(b"P7\nWIDTH %d\nHEIGHT %d\nDEPTH %d\nMAXVAL %d\nTUPLTYPE %s\nENDHDR\n" % (w, h, channels, maxval, tupl))
    for row in pix:
        for p in row:
            for v in p:
                if maxval == 255:
                    data.append(v)
                else:
                    data += struct.pack(">H", v)
    open(path, "wb").write(bytes(data))

def load_pnm(path):
    w, h, depth, maxval, body = read_pam(path)
    sample = 0 if maxval < 256 else 1
    size = 1 if sample == 0 else 2
    pix = []
    i = 0
    for y in range(h):
        row = []
        for x in range(w):
            p = []
            for c in range(depth):
                if size == 1:
                    p.append(body[i])
                else:
                    p.append((body[i] << 8) | body[i + 1])
                i += size
            row.append(tuple(p))
        pix.append(row)
    return pix, w, h, depth, sample

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="fewer images")
    ap.add_argument("--image", action="append", default=[], help="extra PNM/PAM image to test")
    ap.add_argument("--efforts", default="1,2,3")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--no-builtin", action="store_true")
    ap.add_argument("--opt", default="0", help="optimization level of the encoder program")
    args = ap.parse_args()
    for tool in ("djxl", "cjxl"):
        if not shutil.which(tool):
            print("missing %s" % tool)
            return 2
    efforts = [int(e) for e in args.efforts.split(",")]
    work = tempfile.mkdtemp(prefix="jxlenc-verify-")
    golden_failures = check_goldens(work)
    exe = build_program(work, args.opt)

    images = []  # (name, pix, w, h, channels, sample, premultiplied)
    if not args.no_builtin:
        sizes = [(1, 1), (3, 5), (257, 131), (513, 300)]
        if args.quick:
            sizes = [(1, 1), (3, 5), (257, 131)]
        kinds = ["gradient", "noise", "few", "text", "smooth"]
        seed = 1
        for (w, h) in sizes:
            for channels in (1, 2, 3, 4):
                for sample in (0, 1):
                    for kind in kinds:
                        if (w, h) == (513, 300) and (kind, channels, sample) not in (("smooth", 3, 0), ("few", 4, 0), ("text", 1, 1), ("noise", 2, 0)):
                            continue
                        if (w, h) == (257, 131) and sample == 1 and kind not in ("gradient", "noise"):
                            continue
                        maxval = 255 if sample == 0 else 65535
                        images.append(("%s-%dx%d-c%d-%s" % (kind, w, h, channels, "u8" if sample == 0 else "u16"), gen(kind, w, h, channels, maxval, seed), w, h, channels, sample, False))
                        seed += 1
        # 16-bit samples with a common factor (8-bit content * 257, 12-bit
        # content * 16): coded through the MA tree multiplier.
        for (factor, maxval, channels, w, h) in [(257, 255, 3, 257, 131), (16, 4095, 1, 140, 70), (16, 4095, 4, 31, 300)]:
            pix = gen("smooth", w, h, channels, maxval, seed)
            pix = [[tuple(v * factor for v in p) for p in row] for row in pix]
            images.append(("scaled%d-%dx%d-c%d-u16" % (factor, w, h, channels), pix, w, h, channels, 1, False))
            seed += 1
        # palettes beyond 256 colors, and 16-bit palettes
        rnd = random.Random(seed)
        colors = [tuple(rnd.randrange(256) for _ in range(3)) for _ in range(700)]
        many = [[colors[(x * 7 + y * 13 + (x * y) % 5) % 700] for x in range(64)] for y in range(64)]
        images.append(("colors700-64x64-c3-u8", many, 64, 64, 3, 0, False))
        images.append(("few-300x40-c4-u16", gen("few", 300, 40, 4, 65535, seed), 300, 40, 4, 1, False))
        seed += 1
        # wider than one LF group (2048)
        images.append(("smooth-2100x9-c3-u8", gen("smooth", 2100, 9, 3, 255, seed), 2100, 9, 3, 0, False))
        seed += 1
        # floats and premultiplied alpha
        for (w, h) in [(1, 1), (19, 7), (260, 9)]:
            for channels in (1, 3, 4):
                pix = gen("smooth", w, h, channels, 1000, seed)
                fpix = [[tuple(v / 1000.0 for v in p) for p in row] for row in pix]
                images.append(("float16-%dx%d-c%d" % (w, h, channels), fpix, w, h, channels, 2, False))
                hdr = [[tuple((v / 250.0 - 1.0) if c == 0 else v / 1000.0 for c, v in enumerate(p)) for p in row] for row in pix]
                images.append(("float32-%dx%d-c%d" % (w, h, channels), hdr, w, h, channels, 3, False))
                seed += 1
        # 32-bit floats alternating in sign and spanning every exponent: the
        # worst case for residual size.
        rnd = random.Random(seed)
        signs = [[tuple(((-1) ** (x + y + c)) * rnd.choice([3.0e38, 1.0e-38, 1.5, 7.0e-45, 0.0, 65504.0, 1.0e20]) for c in range(3)) for x in range(37)] for y in range(5)]
        images.append(("float32-signs-37x5-c3", signs, 37, 5, 3, 3, False))
        pm = gen("smooth", 33, 17, 4, 255, seed)
        pm = [[(p[0] * p[3] // 255, p[1] * p[3] // 255, p[2] * p[3] // 255, p[3]) for p in row] for row in pm]
        images.append(("premultiplied-33x17-c4", pm, 33, 17, 4, 0, True))
    for path in args.image:
        pix, w, h, depth, sample = load_pnm(path)
        images.append((os.path.basename(path), pix, w, h, depth, sample, False))

    jobs = []
    for (name, pix, w, h, channels, sample, premul) in images:
        raw = os.path.join(work, name + ".raw")
        open(raw, "wb").write(raw_bytes(pix, sample))
        for effort in efforts:
            for container in (0, 1):
                meta = 0
                if container == 1 and (w * h) % 2 == 1:
                    meta = 1
                if container == 0 and name.startswith("gradient") and w == 3:
                    meta = 2
                out = os.path.join(work, "%s-e%d-c%d.jxl" % (name, effort, container))
                jobs.append(dict(name=name, pix=pix, w=w, h=h, channels=channels, sample=sample, premul=premul, effort=effort, container=container, meta=meta, raw=raw, out=out))
    with open(os.path.join(work, "jobs.txt"), "w") as f:
        for j in jobs:
            f.write("%s %s %d %d %d %d %d %d %d %d\n" % (j["raw"], j["out"], j["w"], j["h"], j["channels"], j["sample"], 1 if j["premul"] else 0, j["effort"], j["container"], j["meta"]))
    t0 = time.time()
    r = subprocess.run([exe], cwd=work, capture_output=True, text=True)
    elapsed = time.time() - t0
    lines = r.stdout.strip().split("\n") if r.stdout.strip() else []
    if r.returncode != 0 or len(lines) != len(jobs):
        print(r.stdout[-3000:], r.stderr[-3000:])
        print("encoder program failed (%d results for %d jobs)" % (len(lines), len(jobs)))
        return 1
    failures = 0
    sizes = {}
    for j, line in zip(jobs, lines):
        if not line.startswith("ok "):
            print("FAIL encode %s e%d c%d: %s" % (j["name"], j["effort"], j["container"], line))
            failures += 1
            continue
        n, bound, need = (int(v) for v in line.split()[1:4])
        if n > bound:
            print("FAIL bound %s: %d > %d" % (j["name"], n, bound))
            failures += 1
        ext = ".npy" if j["sample"] >= 2 else ".pam"
        dec = j["out"] + ext
        d = subprocess.run(["djxl", j["out"], dec, "--quiet"], capture_output=True, text=True)
        if d.returncode != 0:
            print("FAIL djxl %s e%d c%d: %s" % (j["name"], j["effort"], j["container"], d.stderr.strip()[-500:]))
            failures += 1
            continue
        problem = compare(j, dec)
        if problem:
            print("FAIL pixels %s e%d c%d: %s" % (j["name"], j["effort"], j["container"], problem))
            failures += 1
            continue
        if j["container"] == 0:
            sizes[(j["name"], j["effort"])] = n
    print("%d jobs, %d failures, encoder program %.1f s at -O%s" % (len(jobs), failures, elapsed, args.opt))
    # Compression against cjxl for the larger 8/16-bit images.
    report = []
    for (name, pix, w, h, channels, sample, premul) in images:
        if sample >= 2 or premul or w * h < 1000:
            continue
        pnm = os.path.join(work, name + (".pam" if channels in (2, 4) else ".pnm"))
        write_pnm(pnm, pix, w, h, channels, sample)
        ref = [cjxl_size(pnm, e, work) for e in (1, 2, 3)]
        ours = [sizes.get((name, e)) for e in efforts]
        report.append((name, ours, ref))
    if report:
        print("%-34s %s | %s" % ("image", " ".join("ours-e%d" % e for e in efforts), "cjxl-e1 cjxl-e2 cjxl-e3"))
        tot_ours = [0] * len(efforts)
        tot_ref = [0, 0, 0]
        for name, ours, ref in report:
            print("%-34s %s | %s" % (name, " ".join("%8s" % v for v in ours), " ".join("%7s" % v for v in ref)))
            for i, v in enumerate(ours):
                tot_ours[i] += v or 0
            for i, v in enumerate(ref):
                tot_ref[i] += v or 0
        print("%-34s %s | %s" % ("total", " ".join("%8d" % v for v in tot_ours), " ".join("%7d" % v for v in tot_ref)))
    if not args.keep:
        shutil.rmtree(work)
    else:
        print("kept", work)
    return 1 if failures or golden_failures else 0

if __name__ == "__main__":
    sys.exit(main())
