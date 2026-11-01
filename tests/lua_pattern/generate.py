#!/usr/bin/env python3
"""Generate differential test cases for the Lua pattern engine.

    python3 tests/lua_pattern/generate.py --lua /path/to/lua5.4 [--random N] [--seed S] [--out DIR]

Writes DIR/cases.txt (default tests/lua_pattern) and, when --lua names a reference
PUC Lua 5.4 interpreter, DIR/expected.txt via reference.lua. Each case line is
"x<hex subject> x<hex pattern> <init>"; the engine is exercised through
string.find, string.match, string.gmatch, and string.gsub(s, p, "<%0>").
"""
import argparse
import os
import random
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

# Hand-written edge cases: (subject, pattern, init).
EDGE = [
    ("abc", "", 1), ("abc", "", 4), ("abc", "", 5), ("abc", "", -1), ("abc", "", -10), ("", "", 1),
    ("", "a*", 1), ("", "^$", 1), ("", "$", 1), ("abc", "^", 1), ("abc", "$", 1), ("abc", "^$", 1),
    ("hello world", "o*", 1), ("abc", "%w*", 1), ("hello world", "o", 1), ("hello world", "o", 6),
    ("hello world", "l+", 1), ("hello world", "(l)(l)", 1), ("hello world", "()ll()", 1),
    ("hello", "(h)(e)(l)(l)(o)", 1), ("hello", "((h)(e))", 1), ("hello", "^(h?)", 1),
    ("x = 1, y = 2", "(%w+)%s*=%s*(%w+)", 1), ("  trim  ", "^%s*(.-)%s*$", 1),
    ("THE (quick) fox", "%((%a+)%)", 1), ("f(a(b)c)d", "%b()", 1), ("f(a(b)c", "%b()", 1),
    ("[[x]]", "%b[]", 1), ('"a""b"', '%b""', 1), ("aXbXc", "%bXX", 1),
    ("THE (quick) fox", "%f[%a]%a+", 1), ("THE (quick) fox", "%f[%a]%a+%f[%A]", 5),
    ("hello", "%f[%l]", 1), ("hello", "%f[%z]", 1), ("hello", "%f[%W]", 1), ("", "%f[%W]", 1),
    ("aaa", "%f[a]", 1), ("aaa", "%f[^a]", 1), ("a\0b", "%f[%z]", 1), ("a\0b", "%f[\0]", 1),
    ("abcabc", "(abc)%1", 1), ("abab", "(a)(b)%2", 1), ("abab", "(a)(b)%1%2", 1), ("aa", "(a)%1", 1),
    ("abc", "%0", 1), ("abc", "(a)%2", 1), ("abc", "(a%1)", 1), ("abc", "()%1", 1), ("aa", "()a%1", 1),
    ("abc", "%", 1), ("abc", "a%", 1), ("xbc", "x%", 1), ("abc", "[a", 1), ("abc", "[a-", 1),
    ("abc", "[]", 1), ("abc", "[^]", 1), ("a]", "[]]", 1), ("a]", "[^]]", 1), ("a]", "[]a]", 1),
    ("]", "[a-%%]", 1), ("%", "[a-%%]", 1), ("-", "[a-]", 1), ("-", "[-a]", 1), ("b", "[a-c]", 1),
    ("-", "[%a-z]", 1), ("q", "[%a-z]", 1), ("^", "[%^]", 1), ("^", "[^^]", 1), ("a^", "[a^]+", 1),
    ("abc", "%b", 1), ("abc", "%ba", 1), ("abc", "%f", 1), ("abc", "%fa", 1), ("abc", "%f[a", 1),
    ("abc", "(a", 1), ("abc", "a)", 1), ("a)", "a)", 1), ("abc", "(()", 1), ("abc", ")", 1),
    ("abc", "(", 1), ("abc", "()", 1), ("abc", "()", 4), ("abc", "(%w*)", 1),
    ("abc", "(" * 32 + ")" * 32, 1), ("abc", "(" * 33 + ")" * 33, 1), ("abc", "()" * 32, 1),
    ("abc", "()" * 33, 1), ("abc", "(" * 33, 1), ("xyz", "(" * 33 + "a", 1),
    ("a.b", "%.", 1), ("a.b", ".", 2), ("a+b", "a+b", 1), ("a+b", "a%+b", 1), ("aab", "a-b", 1),
    ("aab", "a-", 1), ("aab", "a?a?b", 1), ("b", "a?b", 1), ("$a", "$a", 1), ("a$", "a$", 1),
    ("a$b", "$b", 1), ("a$", "$$", 1), ("a", "a$$", 1), ("a^b", "a^b", 1), ("^a", "^^a", 1),
    ("*a", "*a", 1), ("+a", "+a", 1), ("?", "?", 1), ("-", "-", 1), ("ab", "a**", 1),
    ("\xff\x80", "%a", 1), ("\xff\x80", "%A+", 1), ("\xff", "[\x80-\xff]", 1), ("\xff", ".", 1),
    ("\0\0", "%z", 1), ("Zz", "%Z", 1), ("\0a", "\0", 1), ("\0a", "[\0]a", 1), ("a\0", "a%z", 1),
    ("\t\n\v\f\r x", "%s+", 1), ("\x7f\x1f", "%c+", 1), ("!/:@[`{~", "%p+", 1), ("aZ09_", "%w+", 1),
    ("09afAFgG", "%x+", 1), ("aB", "%u", 1), ("aB", "%l", 1), ("aB", "%L", 1), ("a b", "%G", 1),
    ("a", "%y", 1), ("y", "%y", 1), ("Y", "%Y", 1), ("q", "%q", 1),
    ("abc", "b", -1), ("abc", "b", -2), ("abc", "c", -100), ("abc", "a", 0), ("abc", "a", 2),
    ("abc", "^b", 2), ("abc", "^a", 2), ("xa^a", "^a", 1), ("x^a", "^a", 1),
    ("aaa", "a*" * 199, 1), ("aaa", "a*" * 200, 1), ("", "a*" * 200, 1), ("", "a?" * 199, 1),
    ("", "a?" * 200, 1), ("a" * 300, "a?" * 300 + "a" * 300, 1), ("aaa", "a-" * 200, 1),
    ("aaa", "(a*" + "a*" * 197 + ")", 1), ("aaa", "(a*" + "a*" * 198 + ")", 1),
    ("a" * 50, ".-b", 1), ("a" * 50, "(.-)$", 1), ("a" * 60 + "b", "a*b", 1),
    ("x" * 200 + "needle" + "x" * 10, "needle", 1), ("x" * 200 + "needle", "needle", 1),
    ("x" * 200 + "needlf", "needle", 1), ("abcd" * 40 + "abce", "abce", 1), ("ab" * 100, "bab", 50),
    ("x" * 100 + "ab", "ab", 1), ("x" * 100 + "ab", "xab", 100), ("x" * 100, "xxxxx", 97),
    ("x" * 100, "xxxxx", 96), ("ab" * 50, "abababab", 3), ("ab" * 50, "ba", 101), ("abc", "abcd", 1),
    ("key=val; k2=v2", "(%w+)=(%w+)", 1), ("12.5e3", "^[+-]?%d+%.?%d*[eE]?[+-]?%d*$", 1),
    ("one two  three", "%S+", 1), ("a,b,,c", "([^,]*)", 1), ("<a><b>", "<(.-)>", 1),
    ("<a><b>", "<(.*)>", 1), ("abc", "[%w_]+", 1), ("a-b", "[a%-b]+", 1), ("a]b", "[%]]", 1),
    ("a[b", "[%[]", 1), ("ab", "[%a%d]+", 1), ("ab12", "[^%d]+", 1), ("ab", "[b-a]", 1),
]

SUBJECT_ALPHABET = ["a", "a", "a", "b", "b", "c", "(", ")", "[", "]", "%", "-", ".", " ", "1", "A",
                    "\0", "\xff", "^", "$", "x"]
ATOMS = ["a", "b", "c", "x", ".", "%a", "%d", "%w", "%s", "%p", "%A", "%l", "%u", "%x", "%c", "%g",
         "%%", "%.", "%(", "%)", "%]", "%-", "%z", "%W", "%S", "[ab]", "[^a]", "[a-c]", "[%a_]", "[]]",
         "[a-]", "[-a]", "[^]a]", "[%d%]]", "[b-a]", "[a%-c]", "[%a-c]", "[(%)]", "(", ")", "()",
         "%1", "%2", "%3", "%0", "%b()", "%bab", "%bxx", "%b", "%f[a]", "%f[%w]", "%f[^a]", "%f[%z]",
         "%f[%W]", "%fa", "^", "$", "%", "[", "]", "-", "*", "+", "?", " ", "1", "\0", "\xff", "(a)",
         "(.)", "(%w+)", "(a*)", "(.-)", "[%a-%%]", "[a-%%]"]
QUANTS = ["", "", "", "*", "+", "-", "?"]


def random_pattern(rng):
    parts = []
    if rng.random() < 0.15:
        parts.append("^")
    for _ in range(rng.randint(0, 6)):
        parts.append(rng.choice(ATOMS) + rng.choice(QUANTS))
    if rng.random() < 0.1:
        parts.append("$")
    return "".join(parts)


def random_bytes_pattern(rng):
    raw = "ab^$*+?.([%-])fb1"
    return "".join(rng.choice(raw) for _ in range(rng.randint(0, 7)))


def random_nested(rng, depth):
    parts = []
    for _ in range(rng.randint(1, 4)):
        roll = rng.random()
        if roll < 0.2 and depth < 4:
            parts.append("(" + random_nested(rng, depth + 1) + ")")
        elif roll < 0.25:
            parts.append("()")
        elif roll < 0.3:
            parts.append("%" + str(rng.randint(1, 4)))
        else:
            parts.append(rng.choice(ATOMS[:40]) + rng.choice(QUANTS))
    return "".join(parts)


DEPTH_ITEMS = ["a?", "a?", "a-", "%a?", ".?", "[a]?", "a*"]


def depth_case(rng):
    # Patterns that nest about 200 matcher calls deep. A quantified item only
    # nests when it consumes a byte, so the subject has enough 'a's for every
    # item. All items accept the empty string, so the first path at any start
    # offset succeeds unless the nesting limit is hit, which avoids exponential
    # backtracking in either engine.
    count = rng.randint(150, 230)
    items = [rng.choice(DEPTH_ITEMS[:6]) for _ in range(count)]
    for _ in range(rng.randint(0, 3)):
        items.insert(rng.randint(0, len(items)), rng.choice(["()", "(a?)"]))
    if rng.random() < 0.2:
        items.append("a*")
    subject = "a" * (count + 4 + rng.randint(0, 5))
    return subject, "".join(items), 1


def class_of(ch):
    c = ord(ch)
    if ch.isascii() and ch.isalpha():
        return "%a"
    if ch.isascii() and ch.isdigit():
        return "%d"
    if c in (9, 10, 11, 12, 13, 32):
        return "%s"
    if 33 <= c <= 126:
        return "%p"
    if c < 32 or c == 127:
        return "%c"
    return "."


def derived_case(rng):
    # A pattern built from a substring of the subject, so it usually matches.
    subject = "".join(rng.choice(SUBJECT_ALPHABET) for _ in range(rng.randint(1, 16)))
    i = rng.randint(0, len(subject) - 1)
    j = rng.randint(i, min(len(subject), i + 8))
    pieces = []
    for ch in subject[i:j]:
        roll = rng.random()
        if roll < 0.35:
            piece = "%" + ch if ch in "^$*+?.([%-])" else ch
        elif roll < 0.5:
            piece = "."
        elif roll < 0.75:
            piece = class_of(ch)
        elif ch not in "]%^-":
            piece = "[" + rng.choice(["", "x", "%d"]) + ch + rng.choice(["", "b", "%s"]) + "]"
        else:
            piece = "[%" + ch + "]"
        pieces.append(piece + rng.choice(["", "", "", "*", "+", "-", "?"]))
    closed = 0
    for _ in range(rng.randint(0, 3)):
        if not pieces:
            break
        a = rng.randint(0, len(pieces) - 1)
        b = rng.randint(a, len(pieces) - 1)
        pieces[b] = pieces[b] + ")"
        pieces[a] = "(" + pieces[a]
        closed += 1
    if closed and rng.random() < 0.3:
        pieces.append("%" + str(rng.randint(1, closed)))
    if rng.random() < 0.15:
        pieces.insert(rng.randint(0, len(pieces)), "()")
    pattern = "".join(pieces)
    if rng.random() < 0.1:
        pattern = "^" + pattern
    if rng.random() < 0.1:
        pattern += "$"
    return subject, pattern, rng.choice([1, 1, 1, rng.randint(1, len(subject) + 1)])


def random_case(rng):
    roll = rng.random()
    if roll < 0.01:
        return depth_case(rng)
    if roll < 0.4:
        return derived_case(rng)
    if roll < 0.25:
        subject = "".join(rng.choice(SUBJECT_ALPHABET[:8]) for _ in range(rng.randint(0, 24)))
        return subject, random_nested(rng, 0), 1
    subject = "".join(rng.choice(SUBJECT_ALPHABET) for _ in range(rng.randint(0, 10)))
    pattern = random_bytes_pattern(rng) if rng.random() < 0.2 else random_pattern(rng)
    roll = rng.random()
    if roll < 0.6:
        init = 1
    elif roll < 0.9:
        init = rng.randint(1, len(subject) + 2)
    else:
        init = rng.randint(-len(subject) - 2, 0)
    return subject, pattern, init


def encode(text):
    return "x" + text.encode("latin-1").hex()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--lua", help="reference Lua 5.4 interpreter for expected.txt")
    parser.add_argument("--random", type=int, default=6000, help="number of random cases")
    parser.add_argument("--seed", type=int, default=20261101)
    parser.add_argument("--out", default=HERE)
    args = parser.parse_args()
    rng = random.Random(args.seed)
    cases = list(EDGE) + [random_case(rng) for _ in range(args.random)]
    os.makedirs(args.out, exist_ok=True)
    cases_path = os.path.join(args.out, "cases.txt")
    with open(cases_path, "w", encoding="ascii", newline="\n") as f:
        for subject, pattern, init in cases:
            f.write(f"{encode(subject)} {encode(pattern)} {init}\n")
    print(f"wrote {len(cases)} cases to {cases_path}", file=sys.stderr)
    if args.lua:
        expected = subprocess.run([args.lua, os.path.join(HERE, "reference.lua"), cases_path],
                                  check=True, capture_output=True).stdout
        with open(os.path.join(args.out, "expected.txt"), "wb") as f:
            f.write(expected)


if __name__ == "__main__":
    main()
