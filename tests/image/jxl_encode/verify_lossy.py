#!/usr/bin/env python3
"""Check lossy JPEG XL encodings of the Dodo encoder against libjxl's djxl.

Runs the lossy tests (tests/image/jxl_encode/lossy_test.dodo) with
DODO_LOSSY_DUMP pointing at a temporary directory, so every encoding is
written as NAME.jxl next to our decoder's pixels (NAME.pam for integer
samples, NAME.pfm for floats). Each file is then decoded with djxl and the
two decoders must agree: within one 8-bit level for integer samples (257 in
16-bit units), within 0.002 for floats, and exactly for alpha.

Usage: python3 tests/image/jxl_encode/verify_lossy.py [--keep DIR]
Needs djxl (libjxl) on PATH; run `cargo build` first.
"""
import argparse
import os
import struct
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
DODO = os.path.join(ROOT, "target", "debug", "dodo")


def read_pam(path):
    data = open(path, "rb").read()
    end = data.index(b"ENDHDR\n") + 7
    fields = {}
    for line in data[:end].decode().splitlines()[1:-1]:
        key, value = line.split(None, 1)
        if key != "TUPLTYPE":
            fields[key] = int(value)
    w, h, depth, maxval = fields["WIDTH"], fields["HEIGHT"], fields["DEPTH"], fields["MAXVAL"]
    body = data[end:]
    if maxval > 255:
        values = struct.unpack(">%dH" % (w * h * depth), body[: w * h * depth * 2])
    else:
        values = body[: w * h * depth]
    return w, h, depth, maxval, list(values)


def read_pfm(path):
    data = open(path, "rb").read()
    parts = data.split(b"\n", 3)
    channels = 3 if parts[0] == b"PF" else 1
    w, h = map(int, parts[1].split())
    scale = float(parts[2])
    order = "<" if scale < 0 else ">"
    values = struct.unpack(order + "%df" % (w * h * channels), parts[3][: w * h * channels * 4])
    return w, h, channels, list(values)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep", help="dump directory to use and keep")
    args = parser.parse_args()
    directory = args.keep or tempfile.mkdtemp(prefix="jxl-lossy-")
    os.makedirs(directory, exist_ok=True)
    env = dict(os.environ, DODO_LOSSY_DUMP=directory)
    test = subprocess.run([DODO, "test", "tests/image/jxl_encode", "-O", "0", "--filter", "lossy_test.dodo"], cwd=ROOT, env=env, capture_output=True, text=True)
    if test.returncode != 0:
        print(test.stdout[-4000:], test.stderr[-4000:])
        sys.exit("lossy tests failed")
    failures = 0
    names = sorted(f[:-4] for f in os.listdir(directory) if f.endswith(".jxl"))
    for name in names:
        jxl = os.path.join(directory, name + ".jxl")
        ours_pam = os.path.join(directory, name + ".pam")
        if os.path.exists(ours_pam):
            reference = os.path.join(directory, name + ".djxl.pam")
            w, h, depth, maxval, ours = read_pam(ours_pam)
            subprocess.run(["djxl", jxl, reference, "--quiet", "--bits_per_sample=%d" % (16 if maxval > 255 else 8)], check=True)
            rw, rh, rdepth, rmax, theirs = read_pam(reference)
            assert (w, h, depth) == (rw, rh, rdepth), (name, w, h, depth, rw, rh, rdepth)
            alpha = depth in (2, 4)
            worst = 0
            for i, (a, b) in enumerate(zip(ours, theirs)):
                if alpha and i % depth == depth - 1:
                    if a != b:
                        worst = max(worst, 10 ** 9)
                    continue
                worst = max(worst, abs(a - b))
            limit = 257 if maxval > 255 else 1
            ok = worst <= limit
            print("%-14s %4dx%-4d %d ch %2d-bit  max difference %d%s" % (name, w, h, depth, 16 if maxval > 255 else 8, worst, "" if ok else "  FAIL"))
        else:
            ours_pfm = os.path.join(directory, name + ".pfm")
            reference = os.path.join(directory, name + ".djxl.pfm")
            w, h, channels, ours = read_pfm(ours_pfm)
            subprocess.run(["djxl", jxl, reference, "--quiet"], check=True)
            rw, rh, rchannels, theirs = read_pfm(reference)
            assert (w, h, channels) == (rw, rh, rchannels), name
            worst = max(abs(a - b) for a, b in zip(ours, theirs))
            ok = worst <= 0.002
            print("%-14s %4dx%-4d %d ch float   max difference %.5f%s" % (name, w, h, channels, worst, "" if ok else "  FAIL"))
        if not ok:
            failures += 1
    print("%d files, %d failures" % (len(names), failures))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
