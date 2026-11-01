#!/usr/bin/env python3
"""Regenerate the std/compress/brotli test fixtures and cases.dodo.

Inputs are synthesized deterministically (English-like text that exercises
the static dictionary and its transforms, structured binary, random bytes,
repetitive data, a 17 MiB input that spans several meta-blocks) and
compressed with the brotli command-line tool at qualities 0..11 and window
sizes 10..24. A few streams are written bit by bit here: metadata and
uncompressed meta-blocks, which the reference encoder only emits in special
cases, meta-blocks with the most prefix codes (brotli_wide.py), and invalid
streams with their expected errors. cases.dodo records
each fixture's decompressed length and CRC-32 (the originals are not
committed) or the error it must produce.

Run from anywhere: python3 tests/compress/brotli/generate.py
"""

import os
import random
import subprocess
import zlib

import brotli_wide

HERE = os.path.dirname(os.path.abspath(__file__))

# Error positions in brotli.Error, plus 0 for success.
OK, TRUNCATED, INVALID, UNSUPPORTED = 0, 1, 2, 6

WORDS = (
    "the of and to in is that for it as was with be by on not he this are or his from at which but have an they "
    "you were her she there been one all we their has would when if so what can will no more other time people "
    "about into than only its some could new them these first two may then do any like my now over such our man "
    "me even most made after also did many before must through back years where much your way well down should "
    "because each just those how too little state good very make world still own see men work long get here "
    "between both life being under never day same another know while last might us great old year off come since "
    "against go came right used take three himself few house use during without again place american around "
    "however home small found mrs thought went say part once general high upon school every don does got united "
    "left number course war until always away something fact though water less public put think almost hand "
    "enough far took head yet government system better set told nothing night end why called didn eyes find going "
    "look asked later knew point next program city business give group toward young days let room president side "
    "social given present several order national possible rather second face per among form important often "
    "things looked early white case john become large big need four within felt along children saw best church "
    "ever least power development light thing seemed family interest want members mind country area others done "
    "turned although open god service certain kind problem began different door thus help means sense whole "
    "matter perhaps itself york times law human line above name example action company hands local show whether "
    "five history gave today either act feet across taken past quite anything seen having death week experience"
).split()


def english(rng, size):
    out = []
    length = 0
    sentence_start = True
    while length < size:
        word = rng.choice(WORDS)
        roll = rng.random()
        if sentence_start or roll < 0.03:
            word = word.capitalize()
        elif roll < 0.04:
            word = word.upper()
        sentence_start = False
        piece = word
        roll = rng.random()
        if roll < 0.08:
            piece += ","
        elif roll < 0.14:
            piece += "."
            sentence_start = True
        elif roll < 0.15:
            piece = '<a href="' + word + '">' + word + "</a>"
        elif roll < 0.16:
            piece = "(" + word + ")"
        elif roll < 0.17:
            piece += "\n\n"
            sentence_start = True
        out.append(piece)
        length += len(piece) + 1
    return " ".join(out).replace("\n\n ", "\n\n").encode()[:size]


def structured(rng, size):
    out = bytearray()
    counter = 0
    while len(out) < size:
        kind = rng.randrange(4)
        if kind == 0:
            for _ in range(16):
                out += (counter * 2654435761 & 0xFFFFFFFF).to_bytes(4, "little")
                counter += 1
        elif kind == 1:
            out += bytes((i * 7 + counter) & 0xFF for i in range(64))
        elif kind == 2:
            out += (rng.randrange(-300, 300) & 0xFFFF).to_bytes(2, "little") * rng.randrange(1, 9)
        else:
            out += bytes(rng.randrange(256) for _ in range(rng.randrange(1, 24)))
    return bytes(out[:size])


def compress(data, quality, window=22):
    return subprocess.run(["brotli", "-c", "-q", str(quality), "-w", str(window)],
                          input=data, capture_output=True, check=True).stdout


class BitWriter:
    def __init__(self):
        self.bits = []

    def put(self, value, count):
        for i in range(count):
            self.bits.append((value >> i) & 1)

    def align(self):
        while len(self.bits) % 8:
            self.bits.append(0)

    def raw(self, data):
        self.align()
        for b in data:
            self.put(b, 8)

    def bytes(self):
        self.align()
        return bytes(sum(self.bits[i + j] << j for j in range(8)) for i in range(0, len(self.bits), 8))


def metablock_length(w, length):
    # MNIBBLES = 4 (bits 00), then MLEN - 1 in four nibbles.
    w.put(0, 2)
    w.put(length - 1, 16)


def crafted():
    cases = []
    # Window bits 16 (one 0 bit); a metadata meta-block with 5 skipped bytes;
    # an uncompressed meta-block; an empty metadata meta-block; a second
    # uncompressed meta-block; ISLAST + ISLASTEMPTY.
    w = BitWriter()
    w.put(0, 1)
    w.put(0, 1)          # ISLAST = 0
    w.put(3, 2)          # MNIBBLES = 0: metadata
    w.put(0, 1)          # reserved
    w.put(1, 2)          # MSKIPBYTES = 1
    w.put(4, 8)          # MSKIPLEN - 1
    w.raw(b"skip!")
    w.put(0, 1)
    metablock_length(w, 11)
    w.put(1, 1)          # ISUNCOMPRESSED
    w.raw(b"hello world")
    w.put(0, 1)
    w.put(3, 2)
    w.put(0, 1)
    w.put(0, 2)          # MSKIPBYTES = 0
    w.align()
    w.put(0, 1)
    metablock_length(w, 6)
    w.put(1, 1)
    w.raw(b" again")
    w.put(1, 1)          # ISLAST
    w.put(1, 1)          # ISLASTEMPTY
    cases.append(("crafted_meta", w.bytes(), OK, b"hello world again"))
    # Large-window Brotli: 1, 000, 001 then the 6-bit window size.
    w = BitWriter()
    w.put(1, 1)
    w.put(0, 3)
    w.put(1, 3)
    w.put(0, 1)
    w.put(30, 6)
    w.put(1, 1)
    w.put(1, 1)
    cases.append(("crafted_large_window", w.bytes(), UNSUPPORTED, None))
    # Nonzero padding after the last meta-block.
    w = BitWriter()
    w.put(0, 1)
    w.put(1, 1)
    w.put(1, 1)
    w.put(1, 1)          # a set fill bit
    cases.append(("crafted_padding", w.bytes(), INVALID, None))
    # Nonzero padding before uncompressed data.
    w = BitWriter()
    w.put(0, 1)
    w.put(0, 1)
    metablock_length(w, 1)
    w.put(1, 1)
    w.put(1, 1)          # a set fill bit
    w.raw(b"x")
    w.put(3, 2)
    cases.append(("crafted_raw_padding", w.bytes(), INVALID, None))
    # An exuberant length nibble (MNIBBLES = 5 with a zero top nibble).
    w = BitWriter()
    w.put(0, 1)
    w.put(1, 1)
    w.put(0, 1)
    w.put(1, 2)
    w.put(0, 20)
    cases.append(("crafted_nibble", w.bytes(), INVALID, None))
    # A stream cut inside its uncompressed data.
    w = BitWriter()
    w.put(0, 1)
    w.put(0, 1)
    metablock_length(w, 100)
    w.put(1, 1)
    w.raw(b"short")
    cases.append(("crafted_truncated", w.bytes(), TRUNCATED, None))
    # One meta-block with 256 insert-and-copy prefix codes of 704 symbols:
    # about 246,000 workspace words (brotli_wide.py), as the last
    # meta-block and followed by an empty last one.
    wide = english(random.Random(4), 300)
    for name, split in (("crafted_wide", False), ("crafted_wide_split", True)):
        stream = brotli_wide.wide_stream(wide, split)
        decoded = subprocess.run(["brotli", "-d", "-c"], input=stream, capture_output=True, check=True).stdout
        assert decoded == wide, name
        cases.append((name, stream, OK, wide))
    return cases


def main():
    rng = random.Random(7932)
    text = english(rng, 24000)
    text_large = english(rng, 48000)
    binary = structured(rng, 32000)
    noise = bytes(rng.randrange(256) for _ in range(8000))
    pattern = b"abc" * 40000
    zeros = bytes(1 << 20)
    # 17 MiB of text blocks with a running counter: more than one 16 MiB
    # meta-block at any quality, yet small once compressed.
    block = english(random.Random(1), 65536)
    huge = bytearray()
    k = 0
    while len(huge) < (17 << 20):
        huge += b"%08d " % k + block[:4000]
        k += 1
    huge = bytes(huge[:17 << 20])

    cases = []
    for q in (0, 5, 11):
        cases.append((f"empty_q{q}", compress(b"", q), OK, b""))
    cases.append(("byte_q11", compress(b"a", 11), OK, b"a"))
    # Small streams for the truncation and corruption tests.
    short = english(random.Random(2), 2000)
    short_binary = structured(random.Random(3), 2000)
    cases.append(("short_q5", compress(short, 5), OK, short))
    cases.append(("short_q11", compress(short, 11), OK, short))
    cases.append(("short_binary_q9", compress(short_binary, 9, 10), OK, short_binary))
    for q in range(12):
        cases.append((f"text_q{q}", compress(text, q), OK, text))
    for w in (10, 12, 14, 16, 24):
        cases.append((f"text_w{w}_q9", compress(text_large, 9, w), OK, text_large))
    for w in (10, 16, 24):
        cases.append((f"text_w{w}_q11", compress(text_large, 11, w), OK, text_large))
    for q in (0, 1, 2, 4, 6, 9, 11):
        cases.append((f"binary_q{q}", compress(binary, q), OK, binary))
    cases.append(("binary_w11_q10", compress(binary, 10, 11), OK, binary))
    for q in (0, 5, 11):
        cases.append((f"random_q{q}", compress(noise, q), OK, noise))
    for q in (0, 3, 11):
        cases.append((f"pattern_q{q}", compress(pattern, q), OK, pattern))
    cases.append(("zeros_q11", compress(zeros, 11), OK, zeros))
    # Quality 1 cuts meta-blocks every 128 KiB; quality 5 at the 16 MiB limit.
    cases.append(("multi_q1", compress(huge[:2 << 20], 1, 24), OK, huge[:2 << 20]))
    cases.append(("huge_q5", compress(huge, 5, 24), OK, huge))
    cases += crafted()

    for f in os.listdir(HERE):
        if f.endswith(".br"):
            os.remove(os.path.join(HERE, f))
    total = 0
    for name, stream, _, _ in cases:
        with open(os.path.join(HERE, name + ".br"), "wb") as out:
            out.write(stream)
        total += len(stream)
    print(f"{len(cases)} fixtures, {total} bytes")

    lines = [
        "package brotli_test",
        "",
        "// Generated by tests/compress/brotli/generate.py. Per case: the .br",
        "// name, the expected outcome (0: decodes, else 1 + the position of the",
        "// brotli.Error it fails with), the decompressed length and its CRC-32.",
        "",
        f"const CASE_COUNT: usize = {len(cases)}",
        "",
        "static CASE_FILES: [CASE_COUNT]&str = [",
    ]
    lines += [f'    "{name}",' for name, _, _, _ in cases]
    lines[-1] = lines[-1].rstrip(",")
    lines.append("]")
    lines.append("static CASE_OUTCOME: [CASE_COUNT]u8 = [" + ", ".join(str(c[2]) for c in cases) + "]")
    lengths = [len(c[3]) if c[3] is not None else 0 for c in cases]
    crcs = [zlib.crc32(c[3]) if c[3] is not None else 0 for c in cases]
    lines.append("static CASE_LENGTH: [CASE_COUNT]usize = [" + ", ".join(str(v) for v in lengths) + "]")
    lines.append("static CASE_CRC: [CASE_COUNT]u32 = [" + ", ".join(f"0x{v:08x}" for v in crcs) + "]")
    lines.append("")
    with open(os.path.join(HERE, "cases.dodo"), "w") as out:
        out.write("\n".join(lines))


if __name__ == "__main__":
    main()
