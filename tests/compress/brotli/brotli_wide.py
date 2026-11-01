#!/usr/bin/env python3
"""Hand-written Brotli streams whose one meta-block needs a large decoder
workspace, for tests/compress/brotli and the JPEG XL metadata tests.

wide_stream(payload) stores payload as the literals of one insert command
in a compressed meta-block with 256 insert-and-copy block types and so 256
insert-and-copy prefix codes over all 704 symbols (320 of length 9, 384 of
length 10: 960 table words each), a literal code with every byte at length
8 and one-symbol distance and block codes. Decoding it takes about 246,000
workspace words, more than the 128K words (256 KiB) decompress() once kept
on the stack. With split=True the meta-block is not the last and an empty
last meta-block follows.

Used as a module by tests/compress/brotli/generate.py and
tests/image/jxl/generate.py. As a script, it rewrites a JPEG XL container:
  python3 brotli_wide.py IN.jxl OUT.jxl
replaces the plain Exif box of IN.jxl with a brob box holding the same
payload in a wide stream.
"""

import struct
import sys

# The fixed code for code length code lengths, as the decoder indexes it
# by the next four bits.
CL_PREFIX_LEN = [2, 2, 2, 3, 2, 2, 2, 4, 2, 2, 2, 3, 2, 2, 2, 4]
CL_PREFIX_VALUE = [0, 4, 3, 2, 0, 4, 3, 1, 0, 4, 3, 2, 0, 4, 3, 5]
CODE_LENGTH_ORDER = [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15]
INSERT_OFFSET = [0, 1, 2, 3, 4, 5, 6, 8, 10, 14, 18, 26, 34, 50, 66, 98, 130, 194, 322, 578, 1090, 2114, 6210, 22594]
INSERT_BITS = [0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 12, 14, 24]


class BitWriter:
    def __init__(self):
        self.bits = []

    def put(self, value, count):
        for i in range(count):
            self.bits.append((value >> i) & 1)

    def code(self, value, length):
        # Prefix codes are stored most significant bit first.
        for i in reversed(range(length)):
            self.bits.append((value >> i) & 1)

    def bytes(self):
        while len(self.bits) % 8:
            self.bits.append(0)
        return bytes(sum(self.bits[i + j] << j for j in range(8)) for i in range(0, len(self.bits), 8))


def canonical(lengths):
    """Canonical codes (RFC 7932 section 3.2) for lengths, as (code, length)."""
    count = [0] * 16
    for l in lengths:
        if l:
            count[l] += 1
    nxt = [0] * 16
    code = 0
    for l in range(1, 16):
        code = (code + count[l - 1]) << 1 if l > 1 else 0
        nxt[l] = code
    out = []
    for l in lengths:
        if l:
            out.append((nxt[l], l))
            nxt[l] += 1
        else:
            out.append(None)
    return out


def var_u8(w, value):
    if value == 0:
        w.put(0, 1)
        return
    w.put(1, 1)
    n = value.bit_length() - 1
    w.put(n, 3)
    if n:
        w.put(value - (1 << n), n)


def simple_one(w, alphabet, symbol):
    """A simple prefix code with one symbol, which reads no bits."""
    w.put(1, 2)
    w.put(0, 2)
    w.put(symbol, (alphabet - 1).bit_length())


def code_length_value(w, v):
    p = min((p for p in range(16) if CL_PREFIX_VALUE[p] == v), key=lambda p: CL_PREFIX_LEN[p])
    w.put(p, CL_PREFIX_LEN[p])


def complex_code(w, lengths):
    """A complex prefix code for lengths using one or two distinct nonzero
    length values and no zeros or repeats."""
    values = sorted(set(lengths))
    assert 0 not in values and len(values) <= 2
    cl = [0] * 18
    for v in values:
        cl[v] = 1
    w.put(0, 2)  # HSKIP = 0
    space = 32
    for i in range(18):
        v = cl[CODE_LENGTH_ORDER[i]]
        code_length_value(w, v)
        if v:
            space -= 32 >> v
            if space <= 0:
                break
    if len(values) == 2:
        codes = canonical(cl)
        for l in lengths:
            w.code(*codes[l])
    # One value: the code length code has one symbol and reads no bits.


def wide_stream(payload, split=False, window_bits=16):
    assert 1 <= len(payload) <= 22594
    w = BitWriter()
    assert window_bits == 16
    w.put(0, 1)  # WBITS = 16
    w.put(0 if split else 1, 1)  # ISLAST
    if not split:
        w.put(0, 1)  # ISLASTEMPTY
    w.put(0, 2)  # MNIBBLES = 4
    w.put(len(payload) - 1, 16)
    if split:
        w.put(0, 1)  # ISUNCOMPRESSED
    var_u8(w, 0)  # one literal block type
    var_u8(w, 255)  # 256 insert-and-copy block types
    simple_one(w, 258, 1)  # block type code
    simple_one(w, 26, 0)  # block count code: 1 + 2 extra bits
    w.put(3, 2)  # first block count 4
    var_u8(w, 0)  # one distance block type
    w.put(0, 6)  # NPOSTFIX = NDIRECT = 0
    w.put(0, 2)  # literal context mode LSB6
    var_u8(w, 0)  # one literal tree
    var_u8(w, 0)  # one distance tree
    complex_code(w, [8] * 256)
    command_lengths = [9] * 320 + [10] * 384
    for _ in range(256):
        complex_code(w, command_lengths)
    simple_one(w, 64, 0)
    # One command inserting the payload; its copy is never reached.
    ic = max(i for i in range(24) if INSERT_OFFSET[i] <= len(payload))
    cell = 2 if ic < 8 else (4 if ic < 16 else 7)
    base = {2: 0, 4: 8, 7: 16}[cell]
    symbol = cell * 64 + ((ic - base) << 3)
    w.code(*canonical(command_lengths)[symbol])
    w.put(len(payload) - INSERT_OFFSET[ic], INSERT_BITS[ic])
    literal = canonical([8] * 256)
    for b in payload:
        w.code(*literal[b])
    if split:
        w.put(1, 1)  # ISLAST
        w.put(1, 1)  # ISLASTEMPTY
    return w.bytes()


def boxes(data):
    out = []
    at = 0
    while at < len(data):
        size, kind = struct.unpack('>I4s', data[at:at + 8])
        assert size >= 8
        out.append((kind, data[at + 8:at + size]))
        at += size
    return out


def wide_brob(data):
    """data, a JPEG XL container, with its Exif box as a wide brob box."""
    out = b''
    for kind, payload in boxes(data):
        if kind == b'Exif':
            kind, payload = b'brob', b'Exif' + wide_stream(payload)
        out += struct.pack('>I4s', 8 + len(payload), kind) + payload
    return out


if __name__ == '__main__':
    with open(sys.argv[1], 'rb') as f:
        data = f.read()
    with open(sys.argv[2], 'wb') as f:
        f.write(wide_brob(data))
