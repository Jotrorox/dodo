#!/usr/bin/env python3
"""Generate stdlib/std/compress/brotli/tables.dodo from RFC 7932.

The file holds the 122,784-byte static dictionary (Appendix A), the 121
word transforms (Appendix B) and the literal context lookup tables Lut0,
Lut1 and Lut2 (section 7.1). Sources:

  * the dictionary from google/brotli c/common/dictionary.bin, checked
    against its known SHA-256 and the length and CRC-32 that RFC 7932
    Appendix A states;
  * the transforms and context tables parsed from the RFC text, checked
    against the lengths and CRC-32 values the RFC states for them.

Both files are downloaded unless --dictionary / --rfc name local copies.
Run with --write to update the file in place or --check to verify it; with
neither, print the generated file to stdout.
"""

import argparse
import hashlib
import re
import sys
import urllib.request
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "stdlib/std/compress/brotli/tables.dodo"
DICTIONARY_URL = "https://raw.githubusercontent.com/google/brotli/master/c/common/dictionary.bin"
RFC_URL = "https://www.rfc-editor.org/rfc/rfc7932.txt"
DICTIONARY_SHA256 = "20e42eb1b511c21806d4d227d07e5dd06877d8ce7b3a817f378f313653f35c70"
DICTIONARY_CRC = 0x5136CB04
DICTIONARY_LEN = 122784
TRANSFORMS_CRC = 0x3D965F81
TRANSFORMS_LEN = 648
LUT_CRCS = {"Lut0": 0x8E91EFB7, "Lut1": 0xD01A32F4, "Lut2": 0x0DD7A0D6}

# RFC elementary transform names -> the codes used in tables.dodo (and in
# the RFC's check-value serialization): 0 Identity, 1 FermentFirst,
# 2 FermentAll, 3..11 OmitFirst1..9, 12..20 OmitLast1..9.
def transform_code(name: str) -> int:
    if name == "Identity":
        return 0
    if name == "FermentFirst":
        return 1
    if name == "FermentAll":
        return 2
    m = re.fullmatch(r"OmitFirst([1-9])", name)
    if m:
        return 2 + int(m.group(1))
    m = re.fullmatch(r"OmitLast([1-9])", name)
    if m:
        return 11 + int(m.group(1))
    raise ValueError(f"unknown transform {name!r}")


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def rfc_pages(text: str) -> list[str]:
    """RFC lines without page headers, footers and form feeds."""
    lines = []
    for line in text.replace("\f", "").splitlines():
        if line.startswith("RFC 7932 ") or line.startswith("Alakuijala & Szabadka"):
            continue
        lines.append(line)
    return lines


def c_string(literal: str) -> bytes:
    """Decode a C string literal body (without quotes)."""
    out = bytearray()
    i = 0
    while i < len(literal):
        c = literal[i]
        if c != "\\":
            out += c.encode("ascii")
            i += 1
            continue
        e = literal[i + 1]
        if e == "x":
            out.append(int(literal[i + 2:i + 4], 16))
            i += 4
        elif e in "nt":
            out.append(10 if e == "n" else 9)
            i += 2
        elif e in "\"'\\":
            out += e.encode("ascii")
            i += 2
        else:
            raise ValueError(f"unknown escape in {literal!r}")
    return bytes(out)


def parse_transforms(lines: list[str]) -> list[tuple[bytes, int, bytes]]:
    start = next(i for i, l in enumerate(lines) if l.startswith("Appendix B."))
    end = next(i for i, l in enumerate(lines) if l.startswith("Appendix C."))
    row = re.compile(r'^\s+(\d+)\s+"((?:[^"\\]|\\.)*)"\s+(\w+)\s+"((?:[^"\\]|\\.)*)"\s*$')
    transforms = []
    for line in lines[start:end]:
        m = row.match(line)
        if not m:
            continue
        index = int(m.group(1))
        if index != len(transforms):
            raise ValueError(f"transform {index} out of order")
        transforms.append((c_string(m.group(2)), transform_code(m.group(3)), c_string(m.group(4))))
    if len(transforms) != 121:
        raise ValueError(f"found {len(transforms)} transforms")
    serial = b"".join(p + b"\0" + bytes([t]) + s + b"\0" for p, t, s in transforms)
    if len(serial) != TRANSFORMS_LEN or zlib.crc32(serial) != TRANSFORMS_CRC:
        raise ValueError("transform list does not match the RFC check value")
    return transforms


def parse_lut(lines: list[str], name: str) -> list[int]:
    start = next(i for i, l in enumerate(lines) if l.strip() == f"{name} :=")
    values: list[int] = []
    for line in lines[start + 1:]:
        if len(values) == 256:
            break
        if not line.strip():
            continue
        values += [int(v) for v in line.replace(",", " ").split()]
    if len(values) != 256 or zlib.crc32(bytes(values)) != LUT_CRCS[name]:
        raise ValueError(f"{name} does not match the RFC check value")
    return values


def byte_string(data: bytes) -> str:
    out = []
    for b in data:
        if b == 0x22:
            out.append('\\"')
        elif b == 0x5C:
            out.append("\\\\")
        elif 0x20 <= b < 0x7F:
            out.append(chr(b))
        else:
            out.append(f"\\x{b:02x}")
    return 'b"' + "".join(out) + '"'


def u8_table(name: str, values: list[int]) -> str:
    lines = [f"static {name}: [{len(values)}]u8 = ["]
    for start in range(0, len(values), 16):
        chunk = ", ".join(str(v) for v in values[start:start + 16])
        comma = "," if start + 16 < len(values) else ""
        lines.append(f"    {chunk}{comma}")
    lines.append("]")
    return "\n".join(lines)


def generate(dictionary: bytes, rfc: str) -> str:
    if len(dictionary) != DICTIONARY_LEN:
        raise ValueError(f"dictionary has {len(dictionary)} bytes")
    if hashlib.sha256(dictionary).hexdigest() != DICTIONARY_SHA256:
        raise ValueError("dictionary SHA-256 mismatch")
    if zlib.crc32(dictionary) != DICTIONARY_CRC:
        raise ValueError("dictionary CRC-32 does not match RFC 7932 Appendix A")
    lines = rfc_pages(rfc)
    transforms = parse_transforms(lines)
    # Affixes are stored once each in AFFIXES, a byte string of
    # length-prefixed entries; TRANSFORMS holds (prefix offset, elementary
    # transform, suffix offset) per transform.
    affixes = bytearray()
    where: dict[bytes, int] = {}
    for prefix, _, suffix in transforms:
        for affix in (prefix, suffix):
            if affix not in where:
                where[affix] = len(affixes)
                affixes.append(len(affix))
                affixes += affix
    if len(affixes) > 255:
        raise ValueError("affix table exceeds 8-bit offsets")
    triples = []
    for prefix, kind, suffix in transforms:
        triples += [where[prefix], kind, where[suffix]]
    parts = [
        "package brotli",
        "",
        "// Generated by scripts/generate_brotli_tables.py from RFC 7932; do not edit.",
        "",
        "// The static dictionary (RFC 7932 Appendix A, google/brotli",
        "// c/common/dictionary.bin): 122,784 bytes, CRC-32 0x5136cb04.",
        "const DICTIONARY: &[u8] = " + byte_string(dictionary),
        "",
        "// Prefixes and suffixes of the word transforms, each a length byte",
        "// followed by the bytes.",
        "const AFFIXES: &[u8] = " + byte_string(bytes(affixes)),
        "",
        "// Per transform (Appendix B): prefix offset in AFFIXES, elementary",
        "// transform (0 Identity, 1 FermentFirst, 2 FermentAll, 3..11 OmitFirst1..9,",
        "// 12..20 OmitLast1..9), suffix offset in AFFIXES.",
        u8_table("TRANSFORMS", triples),
        "",
        "// Literal context lookup tables (section 7.1).",
        u8_table("LUT0", parse_lut(lines, "Lut0")),
        u8_table("LUT1", parse_lut(lines, "Lut1")),
        u8_table("LUT2", parse_lut(lines, "Lut2")),
        "",
    ]
    return "\n".join(parts)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dictionary", type=Path, help="local copy of dictionary.bin")
    parser.add_argument("--rfc", type=Path, help="local copy of rfc7932.txt")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    dictionary = args.dictionary.read_bytes() if args.dictionary else fetch(DICTIONARY_URL)
    rfc = (args.rfc.read_bytes() if args.rfc else fetch(RFC_URL)).decode("utf-8")
    text = generate(dictionary, rfc)
    if args.write:
        TARGET.write_text(text)
        return 0
    if args.check:
        if TARGET.read_text() != text:
            print(f"{TARGET} is out of date", file=sys.stderr)
            return 1
        return 0
    sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
