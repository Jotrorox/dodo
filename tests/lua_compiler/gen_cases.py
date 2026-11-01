#!/usr/bin/env python3
"""Generate Lua sources that exercise compiler limits and edge cases.

    python3 tests/lua_compiler/gen_cases.py OUTDIR

The files are compared against the reference luac with compare.sh. They are
generated instead of committed because some are large (constant tables past
the LOADK range, loop bodies past the FORPREP range).
"""
import os
import sys

out = sys.argv[1]
os.makedirs(out, exist_ok=True)


def write(name, text):
    with open(os.path.join(out, name + ".lua"), "w") as f:
        f.write(text)


# Local variable limit (200) in the main function and in a nested one.
write("locals_200", "local " + ", ".join("a%d" % i for i in range(200)) + " = 1\n")
write("locals_201", "local " + ", ".join("a%d" % i for i in range(201)) + " = 1\n")
write("locals_201_fn", "\n\nfunction f()\n  local " + ", ".join("a%d" % i for i in range(201)) + "\nend\n")
write("locals_scopes", "".join("do local " + ", ".join("b%d" % i for i in range(150)) + " end\n" for _ in range(3)))

# Upvalue limit (255).
def upvals(n):
    decl = "".join("local u%d = %d\n" % (i, i) for i in range(n))
    # Locals are limited to 200 per function: spread them over two levels.
    outer = "".join("local v%d = %d\n" % (i, i) for i in range(n - 150))
    inner = "".join("local u%d = %d\n" % (i, i) for i in range(150))
    use = " + ".join(["u%d" % i for i in range(150)] + ["v%d" % i for i in range(n - 150)])
    return outer + "local function mid()\n" + inner + "  return function() return " + use + " end\nend\n"
write("upvalues_255", upvals(255))
write("upvalues_256", upvals(256))

# Register limit.
write("registers_call", "a = f(x" + ",x" * 260 + ")\n")
write("registers_ok", "a = f(x" + ",x" * 240 + ")\n")
write("registers_concat", "a = x" + " .. x" * 300 + "\n")
write("registers_table", "a = {" + ", ".join("{%d}" % i for i in range(300)) + "}\n")

# More than 256 constants: RK operands fall back to registers.
lines = ["local t, y = {}, 0"]
for i in range(300):
    lines.append("t.k%d = %d.5" % (i, i))
lines += [
    "local a = y + 299.5", "local b = y * 298.5", "local c = y - 297.5",
    "local d = t.k299", "t.k299 = 'v299'", "t[y] = 'k298'", "local e = y == 'k297'",
    "local f = y ~= 296.5", "local g = t:k295()", "x299 = 1", "local h = x299",
    "local i = y & 1000", "local j = 1000 | y", "local k = y // 290.5",
    "local l = y < 1e10", "local m = y == 1e10", "local n = -y + 1e11",
    "t[1e12] = 'k280'", "t[y] = true", "t[y] = nil", "t.k290 = false",
    "return t.k3, t['k299'], 'k298'",
]
write("constants_300", "\n".join(lines) + "\n")

# Past MAXARG_Bx (131071) constants: LOADKX + EXTRAARG.
n = 131200
chunks = []
for start in range(0, n, 1000):
    chunks.append("t[%d] = {" % start + ", ".join("%d.25" % i for i in range(start, min(n, start + 1000))) + "}\n")
write("constants_loadkx", "local t = {}\n" + "".join(chunks) + "local s = 'x' .. 'y'\nreturn s, 131199.25, 3.25\n")

# FORPREP/FORLOOP jump range (Bx).
write("forloop_long", "local a, b = 1, 2\nfor i = 1, 2 do\n" + "a = b\n" * 131000 + "end\n")
write("forloop_toolong", "local a, b = 1, 2\nfor i = 1, 2 do\n" + "a = b\n" * 131100 + "end\n")
write("tforloop_long", "local a, b = 1, 2\nfor k, v in pairs({}) do\n" + "a = b\n" * 131000 + "end\n")
write("while_long", "local a, b = 1, 2\nwhile a do\n" + "a = b\n" * 140000 + "end\n")

# Table constructors: SETLIST batches past C = 255, NEWTABLE size hints.
write("table_big_array", "t = {" + ", ".join(str(i) for i in range(1000)) + "}\n")
write("table_big_array_multret", "t = {" + ", ".join(str(i) for i in range(600)) + ", f()}\n")
write("table_big_hash", "t = {" + ", ".join("k%d = %d" % (i, i) for i in range(1000)) + "}\n")
write("table_mixed", "t = {" + ", ".join(("k%d = %d" if i % 3 else "%d") % ((i, i) if i % 3 else (i,)) for i in range(700)) + ", ...}\n")
for hs in (1, 2, 3, 4, 5, 7, 8, 9, 16, 17, 31, 32, 33):
    write("table_hash_%d" % hs, "t = {" + ", ".join("k%d = 1" % i for i in range(hs)) + "}\n")

# Line information: long gaps, many instructions on one line, going back.
write("lines_gaps", "".join("x%d = %d\n" % (i, i) + "\n" * (i * 37 % 300) for i in range(200)))
write("lines_oneline", " ".join("x%d = y%d" % (i, i) for i in range(400)) + "\n")
write("lines_back", "function f(a,\n" + "\n" * 200 + "b) return a + b end\n" +
      "local t = {\n" + "".join("  %d,\n" % i for i in range(300)) + "}\n" +
      "x = f(\n" + "\n" * 300 + "1, 2)\n")
write("lines_huge", "\n" * 100000 + "x = 1\n" + "\n" * 100000 + "y = 2\n")

# Deep nesting just below the parser's limit.
write("nesting_parens_150", "x = " + "(" * 150 + "1" + ")" * 150 + "\n")
write("nesting_blocks_120", "do " * 120 + "x = 1" + " end" * 120 + "\n")
write("nesting_funcs_60", "function f() " * 60 + "return 1" + " end" * 60 + "\n")
write("nesting_tables_150", "x = " + "{" * 150 + "}" * 150 + "\n")

# Numerals and constant folding edge cases.
exprs = [
    "9223372036854775807", "9223372036854775808", "-9223372036854775808",
    "0xffffffffffffffff", "0x7fffffffffffffff", "0x8000000000000000", "0x1p-1074",
    "1e309", "-1e309", "2^53", "2^63", "1/0", "-1/0", "0/0", "-0.0", "0.0", "-0",
    "3 // 0", "3 % 0", "3.0 // 0", "3.5 % 0", "1 << 64", "1 << 63", "1 >> -1",
    "1 << -1", "-1 >> 1", "-1 >> 64", "~0", "~1.5", "~2.0", "1.0 | 2", "1.5 | 2",
    "2^0.5", "7 // 2.0", "-7 // 2", "-7 % 3", "7 % -3", "-7.5 % 2", "5.3 % -2",
    "-(-9223372036854775807-1)", "math.mininteger // -1", "-9223372036854775807 - 1 // -1",
    "(-9223372036854775807 - 1) // -1", "(-9223372036854775807 - 1) % -1",
    "'10' + 1", "'a' .. 'b'", "1 .. 2", "10 // 3", "10.0 / 4", "2^-1", "(-2)^2",
    "-2^2", "1e15 + 1", "0.1 + 0.2", "100000000000000", "1e100", "123456789012345678",
    "3.14159265358979", "-3.0", "-(3.0)", "-(-3)", "- - 3", "not nil", "not 1",
    "not not x", "1 == 1", "1 < 2", "x + 0", "x + 128", "x + 129", "x - 127", "x - 128",
    "x - 129", "x * -1", "x << 1", "x << -128", "x >> 127", "1 << x", "128 << x",
    "-127 << x", "x >> 1.0", "x & 0xff", "0xff & x", "x | 1.5", "x ~ 3", "x // 1",
    "x % 0", "x / 0", "x ^ 2", "2 ^ x", "x == 127", "x == 128", "x == -127",
    "x == -128", "x == 1.0", "x == 1.5", "x == 'a'", "x == nil", "x == true",
    "x < 127.0", "x < 1.5", "127 < x", "-128 < x", "x > 3", "x >= 3", "3 > x",
    "3 >= x", "x <= y", "x > y", "x ~= y", "x == y", "x .. y .. z", "x and y or z",
    "x or y and z", "(x and y) or z", "x and (y or z)", "not (x == y)",
    "not (x < y) and z", "#x + 1", "-x", "~x", "x == 2^53", "x + 2^53",
    "x + 9007199254740993", "x * 0.5", "0.5 * x", "x + 1e300 * 1e10",
    "1e308 * 10", "5 // 0.0", "-5 // 0.0", "0.0 / 0.0 == 0.0 / 0.0",
    "1 // -0.0", "math.pi * 2", "2 * 3.5 + x", "x + 2 * 3.5", "(x + 2) * 3",
    "-1e-320", "4.9e-324", "0x.8p1", "0xA.8p0", "0x1P+4", "1E+2", ".5", "5.", "0x.1",
]
src = ["local x, y, z = ...", "local r = {}"]
for i, e in enumerate(exprs):
    src.append("r[%d] = %s" % (i + 1, e))
src.append("return r")
write("folding", "\n".join(src) + "\n")
write("folding_ifs", "\n".join("if %s then f(%d) end" % (e, i) for i, e in enumerate(exprs)) + "\n")
write("folding_whiles", "\n".join("while %s do f(%d) end" % (e, i) for i, e in enumerate(exprs[60:110])) + "\n")
