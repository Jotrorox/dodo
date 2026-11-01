#!/usr/bin/env python3
"""Differential fuzzer for Dodo's Lua runtime against the reference PUC Lua.

    python3 scripts/lua_fuzz.py gen OUTDIR COUNT [--seed N]
    python3 scripts/lua_fuzz.py run [--lua build/lua] [--ref REF] [--count N]
                                    [--seed N] [--jobs J] [--keep DIR]

`gen` writes COUNT random programs. `run` generates programs in a scratch
directory, runs each under both interpreters (same working directory and
relative script name, so error positions agree), compares exit status, stdout
and stderr after normalising addresses, and copies diverging programs to
--keep (default build/lua-fuzz-fail).

The programs exercise runtime semantics rather than the compiler: arithmetic
on mixed integer/float/string operands with edge values, comparisons,
concatenation, string library calls with random arguments and patterns,
tables (length, holes, the table library), metatables with random
metamethods, closures, varargs, pcall/error with all kinds of values,
coroutines (yields inside metamethods, iterators, pcall, to-be-closed
variables), goto, to-be-closed variables and numeric for loops with edge
bounds. Output avoids unspecified behaviour (hash order, addresses, sort
stability, borders of tables with holes) so both interpreters must agree
exactly.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

HEADER = r'''
local function fmt(v)
  local t = type(v)
  if t == "number" then
    if v ~= v then return "nan" end
    if math.type(v) == "integer" then return tostring(v) end
    return tostring(v) .. "|" .. string.format("%.17g", v)
  elseif t == "string" then
    return string.format("%q", v)
  elseif t == "nil" or t == "boolean" then
    return tostring(v)
  end
  return t
end
local function show(...)
  local n = select("#", ...)
  local out = {}
  for i = 1, n do out[i] = fmt((select(i, ...))) end
  print(table.concat(out, " "))
end
local function try(f, ...) show(pcall(f, ...)) end
local function keys(t)
  local ks = {}
  for k in pairs(t) do ks[#ks + 1] = k end
  table.sort(ks, function(a, b)
    local ta, tb = type(a), type(b)
    if ta ~= tb then return ta < tb end
    if ta == "number" or ta == "string" then return a < b end
    return false
  end)
  return ks
end
local function dump(t)
  if type(t) ~= "table" then return fmt(t) end
  local out = {}
  for _, k in ipairs(keys(t)) do out[#out + 1] = fmt(k) .. "=" .. fmt(rawget(t, k)) end
  return "{" .. table.concat(out, ",") .. "}"
end
local function seq(t, n)
  local out = {}
  for i = 1, n do out[i] = fmt(rawget(t, i)) end
  return table.concat(out, ",")
end
local LOG = {}
local function log(...) LOG[#LOG + 1] = table.concat({...}, " ") end
local function flush()
  print("log: " .. table.concat(LOG, "; "))
  LOG = {}
end
'''

INTS = ["0", "1", "-1", "2", "3", "-3", "7", "-7", "10", "63", "64", "-64",
        "65", "255", "256", "1000", "0x7fffffff", "0x80000000",
        "math.maxinteger", "math.mininteger", "(math.maxinteger - 1)",
        "(math.mininteger + 1)", "0xffffffffffffffff", "9007199254740993"]
FLOATS = ["0.0", "-0.0", "0.5", "-0.5", "1.5", "2.5", "-2.5", "3.0", "-7.0",
          "0.1", "1e15", "1e16", "1e100", "2^53", "2^63", "-2^63", "2^64",
          "1e308", "-1e308", "5e-324", "1e-310", "(1/0)", "(-1/0)", "(0/0)",
          "123456789012.0", "1e14", "-123.456", "3.14159", "2^-1074",
          "9.2233720368547758e18", "4611686018427387904.0"]
STRS = ['"10"', '"0x10"', '" 5 "', '"1e2"', '"-0"', '"abc"', '""', '"3.0"',
        '"0x7fffffffffffffff"', '"9223372036854775808"', '"1e"', '" 0x1p4 "',
        '"inf"', '"nan"', '"10\\0"', '"-0x8000000000000000"', '"1.5e300"',
        '"  -7  "', '"0x.8"', '".5"', '"5."', '"0b1"', '"1 2"', '"\\t12\\n"']
OTHERS = ["nil", "true", "false", "{}", "print", "MT1", "MT2"]
ALLVALS = INTS + FLOATS + STRS + OTHERS

ARITH = ["+", "-", "*", "/", "//", "%", "^"]
BITW = ["&", "|", "~", "<<", ">>"]
CMP = ["==", "~=", "<", "<=", ">", ">="]


def lua_str(s: str) -> str:
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"' or ch == "\\":
            out.append("\\" + ch)
        elif 32 <= o < 127:
            out.append(ch)
        else:
            out.append("\\%03d" % o)
    out.append('"')
    return "".join(out)


class Gen:
    def __init__(self, r: random.Random):
        self.r = r
        self.uid = 0

    def fresh(self, base="v"):
        self.uid += 1
        return "%s%d" % (base, self.uid)

    def c(self, xs):
        return self.r.choice(xs)

    def num(self):
        return self.c(INTS + FLOATS) if self.r.random() < 0.85 else self.c(STRS)

    def val(self):
        r = self.r.random()
        if r < 0.4:
            return self.c(INTS)
        if r < 0.7:
            return self.c(FLOATS)
        if r < 0.9:
            return self.c(STRS)
        return self.c(OTHERS)

    def smallint(self, lo=-5, hi=12):
        return str(self.r.randint(lo, hi))

    # ------------------------------------------------------------ blocks

    def b_arith(self):
        lines = []
        for _ in range(self.r.randint(3, 10)):
            a, b = self.val(), self.val()
            op = self.c(ARITH + BITW + CMP + [".."])
            # constant-folded and runtime forms must agree
            if self.r.random() < 0.5:
                lines.append("try(function() local a, b = %s, %s return a %s b end)" % (a, b, op))
            else:
                lines.append("try(function() return %s %s %s end)" % (a, op, b))
            if self.r.random() < 0.3:
                u = self.c(["-", "~", "not ", "#"])
                lines.append("try(function() local a = %s return %sa end)" % (a, u))
        return lines

    def b_mathlib(self):
        lines = []
        fns1 = ["math.floor", "math.ceil", "math.abs", "math.tointeger",
                "math.type", "math.sqrt", "math.exp", "math.log", "tostring",
                "tonumber", "math.modf", "math.frexp", "math.ult"]
        fns2 = ["math.fmod", "math.max", "math.min", "math.log", "math.ult",
                "tonumber", "math.ldexp", "string.format"]
        for _ in range(self.r.randint(3, 8)):
            if self.r.random() < 0.5:
                f = self.c(fns1)
                lines.append("try(%s, %s)" % (f, self.val()))
            else:
                f = self.c(fns2)
                if f == "tonumber":
                    lines.append("try(tonumber, %s, %s)" % (self.c(STRS + ['"ff"', '"zz"', '"-11"', '"7fffffffffffffff"', '"  1010  "', '"1e1"']),
                                                           self.c(["2", "8", "10", "16", "36", "1", "37", "16.0", "2.5"])))
                elif f == "string.format":
                    lines.append("try(string.format, %s, %s)" % (self.c(['"%d"', '"%5.2f"', '"%g"', '"%.3g"', '"%x"', '"%i"', '"%a"', '"%.14g"', '"%s"', '"%q"', '"%c"', '"%o"', '"%e"', '"%10.4s"', '"%-5d|"', '"%+.0f"', '"%#x"', '"% d"', '"%05.1f"', '"%.99f"', '"%5"', '"%"', '"%z"']),
                                                                 self.val()))
                else:
                    lines.append("try(%s, %s, %s)" % (f, self.num(), self.num()))
        if self.r.random() < 0.3:
            s = self.r.randint(0, 1000)
            lines.append("math.randomseed(%d) show(math.random(1, 100), math.random(), math.random(0), math.random(-5, 5))" % s)
            lines.append("try(math.random, %s, %s)" % (self.c(INTS), self.c(INTS)))
        return lines

    def b_compare(self):
        lines = []
        for _ in range(self.r.randint(3, 8)):
            a, b = self.c(INTS + FLOATS + STRS), self.c(INTS + FLOATS + STRS)
            op = self.c(CMP)
            lines.append("try(function() local a, b = %s, %s return a %s b end)" % (a, b, op))
        return lines

    def b_concat(self):
        lines = []
        for _ in range(self.r.randint(2, 6)):
            n = self.r.randint(2, 5)
            vals = [self.c(INTS + FLOATS + STRS + ["nil", "true", "{}", "MT1"]) for _ in range(n)]
            locs = ", ".join("x%d" % i for i in range(n))
            lines.append("try(function() local %s = %s return %s end)" % (locs, ", ".join(vals), " .. ".join("x%d" % i for i in range(n))))
        return lines

    def rand_pattern(self):
        atoms = ["a", "b", ".", "%a", "%d", "%s", "%w", "%p", "%x", "%u", "%l",
                 "[ab]", "[^a]", "[%d_]", "[a-c]", "%%", "%.", "%b()", "%f[%w]",
                 "(", ")", "()", "%1", "[", "]", "^", "$", "-", "+", "*", "?",
                 "%", "[%a-]", "%A", "%D", "[]]", "[^]]", "%z", "\\0", "%g", "%c"]
        quants = ["", "", "", "*", "+", "-", "?"]
        n = self.r.randint(1, 5)
        p = []
        for _ in range(n):
            a = self.c(atoms)
            p.append(a + (self.c(quants) if a not in ("(", ")", "^", "$") else ""))
        if self.r.random() < 0.4:
            p.insert(0, "(")
            p.append(")")
        return "".join(p)

    def rand_subject(self):
        chars = "aabbc ()d_1-2.%\0xyzAB\n["
        n = self.r.randint(0, 14)
        return "".join(self.r.choice(chars) for _ in range(n))

    def b_strlib(self):
        lines = []
        for _ in range(self.r.randint(3, 9)):
            s = lua_str(self.rand_subject())
            k = self.r.random()
            if k < 0.12:
                lines.append("try(string.sub, %s, %s, %s)" % (s, self.c(INTS + ["-2", "-100", "100", "2.0", '"3"', "1.5"]), self.c(INTS + ["-2", "-100", "100", "nil"])))
            elif k < 0.2:
                lines.append("try(string.byte, %s, %s, %s)" % (s, self.smallint(-20, 20), self.smallint(-20, 20)))
            elif k < 0.27:
                lines.append("try(string.rep, %s, %s, %s)" % (s, self.smallint(-2, 5), self.c(['","', "nil", '""'])))
            elif k < 0.32:
                lines.append("try(string.char, %s)" % ", ".join(self.smallint(-1, 260) for _ in range(self.r.randint(0, 4))))
            elif k < 0.5:
                f = self.c(["string.find", "string.match"])
                extra = ""
                if self.r.random() < 0.5:
                    extra = ", %s" % self.c(["1", "2", "-1", "-3", "0", "100", "-100"])
                    if f == "string.find" and self.r.random() < 0.5:
                        extra += ", " + self.c(["true", "false", "nil"])
                lines.append("try(%s, %s, %s%s)" % (f, s, lua_str(self.rand_pattern()), extra))
            elif k < 0.65:
                repl = self.c(['"x"', '"%0%0"', '"%1"', '"%2"', '"%%"', '"%"', '{a="A", b=false}',
                               'function(a) return a and #a end', 'function() return nil end',
                               'function(a, b) return b end', '{}', '"<%0>"', '1', 'true'])
                n = self.c(["", ", 1", ", 0", ", -1", ", 2.0", ", 3"])
                lines.append("try(string.gsub, %s, %s, %s%s)" % (s, lua_str(self.rand_pattern()), repl, n))
            elif k < 0.72:
                lines.append("try(function() local t = {} for a, b in string.gmatch(%s, %s%s) do t[#t+1] = tostring(a) .. ':' .. tostring(b) end return table.concat(t, ',') end)"
                             % (s, lua_str(self.rand_pattern()), self.c(["", ", 2", ", -3", ", 100"])))
            elif k < 0.8:
                f = self.c(["string.upper", "string.lower", "string.len", "string.reverse"])
                lines.append("try(%s, %s)" % (f, self.c([s] + STRS + INTS[:4] + FLOATS[:4] + ["nil", "{}"])))
            elif k < 0.9:
                fmtspec = self.c(['"%5s|%-5s|"', '"%q"', '"%s %s"', '"%d %d"', '"%x"', '"%.3s"', '"%c%c"', '"%q %q"', '"%%"', '"%i"', '"%-+8.3f"', '"%.0s"'])
                lines.append("try(string.format, %s, %s, %s)" % (fmtspec, self.val(), self.val()))
            else:
                lines.append("try(function() local s = %s return s:len(), (s:rep(2)):upper(), #s, s == %s end)" % (s, lua_str(self.rand_subject())))
        return lines

    def b_pack(self):
        lines = []
        fmts = ["i4", "<i2", ">I3", "b", "B", "h", "H", "l", "j", "J", "T", "f", "d", "n",
                "s1", "s2", "z", "x", "Xi4", "!4", "i16", "i0", "c3", "<", ">", "=", "i7", "I9"]
        for _ in range(self.r.randint(2, 5)):
            n = self.r.randint(1, 3)
            f = "".join(self.c(fmts) for _ in range(n))
            args = ", ".join(self.c(INTS[:12] + ['"ab"', "1.5", "-2.0", '"x"']) for _ in range(n))
            lines.append("try(function() local s = string.pack(%s, %s) return #s, string.unpack(%s, s) end)" % (lua_str(f), args, lua_str(f)))
            lines.append("try(string.packsize, %s)" % lua_str(f))
        return lines

    def b_utf8(self):
        lines = []
        subjects = ['"héllo"', '"\\xff"', '"a\\u{10FFFF}b"', '"\\u{7FFFFFFF}"', '"abc"', '""', '"\\xe4\\xb8"', '"\\xc0\\x80"', '"\\xed\\xa0\\x80"']
        for _ in range(self.r.randint(2, 5)):
            s = self.c(subjects)
            k = self.r.random()
            if k < 0.25:
                lines.append("try(utf8.len, %s, %s, %s, %s)" % (s, self.smallint(-4, 6), self.smallint(-4, 6), self.c(["true", "false"])))
            elif k < 0.45:
                lines.append("try(utf8.codepoint, %s, %s, %s)" % (s, self.smallint(-4, 6), self.smallint(-4, 6)))
            elif k < 0.6:
                lines.append("try(utf8.offset, %s, %s, %s)" % (s, self.smallint(-4, 6), self.smallint(-4, 8)))
            elif k < 0.8:
                lines.append("try(utf8.char, %s)" % ", ".join(self.c(["65", "0", "0x7FF", "0x800", "0x10FFFF", "0x7FFFFFFF", "-1", "0x80000000", "228"]) for _ in range(self.r.randint(0, 3))))
            else:
                lines.append("try(function() local t = {} for p, c in utf8.codes(%s%s) do t[#t+1] = p .. '=' .. c end return table.concat(t, ' ') end)" % (s, self.c(["", ", true"])))
        return lines

    def b_table(self):
        t = self.fresh("t")
        lines = ["do local %s = {%s}" % (t, ", ".join(self.c(INTS[:8] + ['"s"', "true", "1.5"]) for _ in range(self.r.randint(0, 6))))]
        keys_ = INTS[:10] + ["1.0", "2.0", "-0.0", "0.5", '"a"', '"b"', '"1"', "true", "2^53"]
        for _ in range(self.r.randint(3, 12)):
            k = self.r.random()
            if k < 0.3:
                lines.append("  try(function() %s[%s] = %s end)" % (t, self.c(keys_ + ["nil", "(0/0)"]), self.c(["nil", "1", '"v"', "false", "2.5"])))
            elif k < 0.45:
                lines.append("  try(table.insert, %s, %s)" % (t, self.c(["1", '"x"', "nil", "false"])))
            elif k < 0.55:
                lines.append("  try(table.insert, %s, %s, %s)" % (t, self.smallint(-1, 10), self.c(["1", '"x"', "false"])))
            elif k < 0.65:
                lines.append("  try(table.remove, %s%s)" % (t, self.c(["", ", 1", ", #%s" % t, ", #%s + 1" % t, ", 0", ", -1", ", 100"])))
            elif k < 0.72:
                lines.append("  try(table.concat, %s, %s, %s, %s)" % (t, self.c(['","', '""', "nil", "1"]), self.smallint(-1, 4), self.smallint(-1, 8)))
            elif k < 0.8:
                lines.append("  try(table.unpack, %s, %s, %s)" % (t, self.smallint(-3, 4), self.smallint(-3, 8)))
            elif k < 0.86:
                lines.append("  try(table.move, %s, %s, %s, %s%s)" % (t, self.smallint(-2, 5), self.smallint(-2, 6), self.smallint(-2, 8), self.c(["", ", {}"])))
            elif k < 0.92:
                lines.append("  try(function() local c = 0 for i, v in ipairs(%s) do c = c + 1 end return c end)" % t)
            else:
                lines.append("  try(next, {}, %s)" % self.c(["nil", "1", '"zz"']))
            lines.append("  show(dump(%s))" % t)
        lines.append("  try(function() return #%s, select('#', table.unpack(%s, 1, 3)) end)" % (t, t))
        lines.append("end")
        return lines

    def b_sort(self):
        n = self.r.randint(0, 25)
        kind = self.r.random()
        if kind < 0.4:
            elems = [str(self.r.randint(-20, 20)) for _ in range(n)]
        elif kind < 0.6:
            elems = [str(self.r.randint(-20, 20) / 4.0) for _ in range(n)]
        elif kind < 0.8:
            elems = [lua_str("".join(self.r.choice("abcxyz") for _ in range(self.r.randint(0, 4)))) for _ in range(n)]
        else:
            elems = [self.c(["1", "2.5", '"a"', "true", "{}", "-3"]) for _ in range(n)]
        cmp = self.c(["", ", function(a, b) return a > b end",
                      ", function(a, b) return a < b end",
                      ", function(a, b) return tostring(a) < tostring(b) end"])
        t = self.fresh("t")
        return ["do local %s = {%s}" % (t, ", ".join(elems)),
                "  try(table.sort, %s%s)" % (t, cmp),
                "  show(seq(%s, %d)) end" % (t, n)]

    def b_meta(self):
        events = ["__add", "__sub", "__mul", "__div", "__mod", "__pow", "__unm",
                  "__idiv", "__band", "__bor", "__bxor", "__shl", "__shr", "__bnot",
                  "__concat", "__len", "__eq", "__lt", "__le", "__index",
                  "__newindex", "__call", "__tostring", "__name", "__close"]
        mt = self.fresh("mt")
        lines = ["do local %s = {}" % mt]
        for ev in self.r.sample(events, self.r.randint(1, 10)):
            if ev == "__index":
                v = self.c(["function(t, k) log('idx', tostring(k)) return k end", "{x = 1, [1] = 'one'}", "nil", "'str'"])
            elif ev == "__newindex":
                v = self.c(["function(t, k, v) log('nidx', tostring(k), tostring(v)) rawset(t, k, v) end", "{}", "function() error('ro') end"])
            elif ev == "__name":
                v = self.c(['"MyType"', "42"])
            elif ev == "__tostring":
                v = self.c(['function() return "TS" end', "function() return 1 end", "function() return {} end"])
            elif ev == "__call":
                v = "function(self, ...) log('call', select('#', ...)) return ... end"
            elif ev in ("__eq", "__lt", "__le"):
                v = self.c(["function(a, b) log('%s') return true end" % ev, "function(a, b) log('%s') return nil end" % ev, "function(a, b) return 0 end"])
            elif ev == "__len":
                v = self.c(["function(a) log('len') return 42 end", "function() return 'x' end", "function() end"])
            elif ev == "__close":
                v = "function(o, e) log('close', tostring(e)) end"
            else:
                v = self.c(["function(a, b) log('%s') return 7 end" % ev,
                            "function(a, b) log('%s', type(a), type(b)) return a == b end" % ev,
                            "function() error('in %s') end" % ev,
                            "function(a, b) return end"])
            lines.append("  %s.%s = %s" % (mt, ev, v))
        lines.append("  local a, b = setmetatable({}, %s), setmetatable({}, %s)" % (mt, self.c([mt, "{}", "nil"])))
        ops = []
        for _ in range(self.r.randint(3, 10)):
            x, y = self.c(["a", "b", "1", "2.5", '"s"', "nil"]), self.c(["a", "b", "1", '"3"'])
            k = self.r.random()
            if k < 0.45:
                ops.append("return %s %s %s" % (x, self.c(ARITH + BITW + CMP + [".."]), y))
            elif k < 0.55:
                ops.append("return %sa" % self.c(["-", "~", "#", "not "]))
            elif k < 0.65:
                ops.append("return a.x, a[1], a.zz, rawget(a, 'x')")
            elif k < 0.72:
                ops.append("a.y = 5 a[3] = 'q' return rawget(a, 'y'), rawget(a, 3)")
            elif k < 0.8:
                ops.append("return a(1, nil, 3)")
            elif k < 0.88:
                ops.append("return tostring(a):gsub('0x%x+', 'ADDR')")
            elif k < 0.94:
                ops.append("local c <close> = a return 1")
            else:
                ops.append("return rawequal(a, b), rawlen(a), a == b, a ~= b")
        for o in ops:
            lines.append("  try(function() %s end) flush()" % o)
        lines.append("end")
        return lines

    def b_inherit(self):
        # chains of __index / __newindex through tables and functions
        depth = self.r.randint(1, 6)
        lines = ["do local base = {v = 'base'}", "  local cur = base"]
        for i in range(depth):
            kind = self.c(["table", "func", "rawtable"])
            if kind == "func":
                lines.append("  do local up = cur cur = setmetatable({}, {__index = function(t, k) log('f%d', tostring(k)) return up[k] end}) end" % i)
            else:
                lines.append("  cur = setmetatable({lvl%d = %d}, {__index = cur, __newindex = %s})" % (i, i, "cur" if kind == "table" else "nil"))
        lines.append("  try(function() return cur.v, cur.lvl0, cur.missing end) flush()")
        lines.append("  try(function() cur.newk = 1 return rawget(cur, 'newk'), base.newk end) flush()")
        lines.append("end")
        return lines

    def b_closure(self):
        n = self.r.randint(1, 5)
        lines = ["do local fs = {}",
                 "  for i = 1, %d do local j = i * 2 fs[i] = function(d) j = j + (d or 0) i = i + 1 return i, j end end" % n,
                 "  local acc = {}",
                 "  for i = 1, %d do local a, b = fs[i](i) local c, d = fs[i]() acc[#acc+1] = a .. ':' .. b .. ':' .. c .. ':' .. d end" % n,
                 "  show(table.concat(acc, ' '))",
                 "  local x = 0 local function inc() x = x + 1 return x end local function get() return x end",
                 "  inc() inc() show(get(), x)",
                 "  local co = coroutine.wrap(function() local y = 10 local f = function() y = y + 1 return y end coroutine.yield(f) return f() end)",
                 "  local f = co() show(f(), f(), co())",
                 "end"]
        return lines

    def b_vararg(self):
        args = ", ".join(self.c(["nil", "1", '"a"', "false", "2.5"]) for _ in range(self.r.randint(0, 6)))
        lines = ["do local function va(...) return select('#', ...), ... end",
                 "  show(va(%s))" % args,
                 "  try(select, %s%s)" % (self.c(["1", "2", "-1", "-2", "0", "'#'", "-10", "10", "2.0", "'2'", "1.5"]), (", " + args) if args else ""),
                 "  show(table.pack(%s).n)" % args,
                 "  try(table.unpack, {%s}, %s, %s)" % (args, self.smallint(-2, 3), self.smallint(-2, 8)),
                 "  local function tail(n, ...) if n == 0 then return ... end return tail(n - 1, n, ...) end",
                 "  show(tail(%d))" % self.r.randint(0, 8),
                 "  show((va(%s)))" % args,
                 "  show({va(%s)}, #{va(%s)})" % (args, args),
                 "end"]
        return lines

    def b_error(self):
        lines = []
        errvals = ["nil", "1", "2.5", '"msg"', "{}", "setmetatable({}, {__tostring = function() return 'TS' end})",
                   "true", "math.huge", '"x\\0y"', "print"]
        for _ in range(self.r.randint(2, 6)):
            k = self.r.random()
            e = self.c(errvals)
            lvl = self.c(["", ", 0", ", 1", ", 2", ", 3", ", -1"])
            if k < 0.3:
                lines.append("try(function() error(%s%s) end)" % (e, lvl))
            elif k < 0.5:
                h = self.c(["function(m) return 'H:' .. tostring(m) end", "function(m) error('again') end",
                            "function(m) return m, 2 end", "print", "nil", "debug.traceback and function(m) return type(m) end"])
                lines.append("show(xpcall(function() error(%s%s) end, %s))" % (e, lvl, h))
            elif k < 0.6:
                lines.append("show(pcall(pcall, error, %s))" % e)
            elif k < 0.7:
                lines.append("show(pcall(error))")
            elif k < 0.8:
                lines.append("try(function() local x = nil; return x.y.z end)")
                lines.append("try(function() local t = {} return t.a.b end)")
                lines.append("try(function() return undefinedglobal.x end)")
                lines.append("try(function() local s = 'x' return s:nomethod() end)")
                lines.append("try(function() return #nil end)")
                lines.append("try(function() return math.floor({}) end)")
                lines.append("try(function() return ('x'):rep(1, 2, 3) < 1 end)")
            elif k < 0.9:
                lines.append("try(function() assert(%s, %s) end)" % (self.c(["false", "nil", "1"]), e))
                lines.append("try(assert, false)")
            else:
                lines.append("try(function() local ok, m = pcall(error, %s) error(m, 2) end)" % e)
        return lines

    def b_coroutine(self):
        k = self.r.randint(0, 8)
        if k == 0:
            return ["do local co = coroutine.create(function(a, b) local c = coroutine.yield(a + b) local d, e = coroutine.yield(c * 2) return d + e end)",
                    "  show(coroutine.resume(co, 1, 2)) show(coroutine.resume(co, 10)) show(coroutine.resume(co, 3, 4)) show(coroutine.resume(co)) show(coroutine.status(co)) end"]
        if k == 1:
            # yield inside metamethods
            ev = self.c(["__add", "__index", "__lt", "__concat", "__eq", "__len", "__call", "__newindex", "__le", "__unm"])
            body = {"__add": "return a + 1", "__index": "return a.foo", "__lt": "return a < a",
                    "__concat": "return a .. 'x'", "__eq": "return a == setmetatable({}, getmetatable(a))",
                    "__len": "return #a", "__call": "return a(5)", "__newindex": "a.z = 3 return 'set'",
                    "__le": "return a <= a", "__unm": "return -a"}[ev]
            return ["do local mt = {%s = function(...) local r = coroutine.yield('in %s') return r end}" % (ev, ev),
                    "  local co = coroutine.wrap(function() local a = setmetatable({}, mt) %s end)" % body,
                    "  try(co) try(co, 99) try(co) end"]
        if k == 2:
            # yield inside generic for iterator
            return ["do local function gen(n) return coroutine.wrap(function() for i = 1, n do coroutine.yield(i, i * i) end end) end",
                    "  local acc = {} for i, sq in gen(%d) do acc[#acc+1] = i .. '/' .. sq end show(table.concat(acc, ',')) end" % self.r.randint(0, 6)]
        if k == 3:
            # yield across pcall, error after yield
            return ["do local co = coroutine.create(function() local ok, e = pcall(function() coroutine.yield(1) error(%s) end) coroutine.yield(ok, e) return 'done' end)" % self.c(['"boom"', "{}", "nil", "42"]),
                    "  show(coroutine.resume(co)) show(coroutine.resume(co)) show(coroutine.resume(co)) show(coroutine.resume(co)) end"]
        if k == 4:
            # to-be-closed + coroutine.close
            return ["do local co = coroutine.create(function() local x <close> = setmetatable({}, {__close = function(_, e) log('closed', tostring(e)) %s end}) coroutine.yield(1) end)" % self.c(["", "error('cerr')", "coroutine.yield(2)"]),
                    "  show(coroutine.resume(co)) show(coroutine.close(co)) show(coroutine.status(co)) flush() end"]
        if k == 5:
            return ["do show(coroutine.isyieldable(), coroutine.running()) local co = coroutine.wrap(function() return coroutine.isyieldable(), select(2, coroutine.running()) end) show(co())",
                    "  try(coroutine.resume, coroutine.running()) try(coroutine.yield, 1) try(coroutine.wrap(function() error('w') end))",
                    "  local co2 = coroutine.create(function() return 1 end) coroutine.resume(co2) show(coroutine.resume(co2)) try(coroutine.close, co2)",
                    "  try(coroutine.wrap(function() error({}) end)) try(coroutine.wrap(function() error(nil) end)) end"]
        if k == 6:
            # nested coroutines and yield from inner to outer via wrap
            d = self.r.randint(1, 6)
            return ["do local function nest(n) if n == 0 then coroutine.yield('leaf') return 'bottom' end local co = coroutine.wrap(nest) local a = co(n - 1) local b = co() coroutine.yield(n) return tostring(a) .. tostring(b) end",
                    "  local co = coroutine.wrap(nest) show(co(%d)) show(co()) try(co) end" % d]
        if k == 7:
            # yield inside table.sort comparator is not allowed; inside gsub callback neither
            return ["do local co = coroutine.wrap(function() table.sort({3, 2, 1}, function(a, b) coroutine.yield() return a < b end) end) try(co)",
                    "  local co2 = coroutine.wrap(function() return string.gsub('ab', '.', function(c) coroutine.yield(c) end) end) try(co2)",
                    "  local co3 = coroutine.wrap(function() return pcall(coroutine.yield, 'py') end) show(co3()) show(co3('back')) end"]
        return ["do local co = coroutine.create(function(...) return select('#', ...), ... end)",
                "  show(coroutine.resume(co, %s)) show(coroutine.status(co)) try(coroutine.status, 1) end" % ", ".join(self.c(["nil", "1", "'a'"]) for _ in range(self.r.randint(0, 4)))]

    def b_goto(self):
        n = self.r.randint(0, 10)
        m = self.r.randint(1, 4)
        return ["do local out = {}",
                "  for i = 1, %d do" % n,
                "    if i %% %d == 0 then goto continue end" % m,
                "    out[#out+1] = i",
                "    ::continue::",
                "  end",
                "  local i = 0",
                "  ::top:: i = i + 1 if i < %d then goto top end" % self.r.randint(1, 5),
                "  do local fs = {} local k = 1 ::again:: local x = k fs[k] = function() return x end k = k + 1 if k <= 3 then goto again end show(fs[1](), fs[2](), fs[3]()) end",
                "  show(table.concat(out, ','), i) end"]

    def b_tbc(self):
        n = self.r.randint(1, 4)
        lines = ["do local function mk(name, fail) return setmetatable({}, {__close = function(o, e) log('close ' .. name, tostring(e)) if fail then error('cl ' .. name, 0) end end}) end",
                 "  try(function()"]
        for i in range(n):
            lines.append("    local v%d <close> = %s" % (i, self.c(["mk('a%d')" % i, "mk('b%d', true)" % i, "nil", "false"])))
        lines.append("    %s" % self.c(["return 1, 2", "error('body')", "do end", "error({})", "return"]))
        lines.append("  end) flush()")
        lines.append("  try(function() local x <close> = %s end)" % self.c(["{}", "1", "'s'", "setmetatable({}, {})"]))
        lines.append("  try(function() for i in function(s, c) if c < 2 then return c + 1 end end, nil, 0, mk('forclose') do log('it', i) end end) flush()")
        lines.append("  try(function() for i in function(s, c) if c < 2 then return c + 1 end end, nil, 0, mk('forbreak') do break end end) flush()")
        lines.append("end")
        return lines

    def b_for(self):
        lines = []
        bounds = ["0", "1", "-1", "3", "10", "math.maxinteger", "math.mininteger",
                  "(math.maxinteger - 2)", "(math.mininteger + 2)", "1.5", "0.1",
                  "2^53", "-2^63", "2^63", "1e308", "(1/0)", "(-1/0)", "(0/0)", '"1"', "nil", "3.0", "-0.0"]
        steps = ["1", "-1", "2", "-2", "0", "0.5", "-0.25", "math.maxinteger",
                 "math.mininteger", "0.0", "(1/0)", "1e308", '"1"', "3", "-3"]
        for _ in range(self.r.randint(2, 6)):
            a, b, s = self.c(bounds), self.c(bounds), self.c(steps)
            lines.append("try(function() local c, first, last = 0 for i = %s, %s, %s do c = c + 1 if c == 1 then first = i end last = i if c >= 20 then break end end return c, first, last end)" % (a, b, s))
        return lines

    def b_stringconv(self):
        lines = []
        for _ in range(self.r.randint(2, 6)):
            v = self.c(INTS + FLOATS)
            lines.append("try(function() local v = %s return tostring(v), v .. '', string.format('%%q', v), math.tointeger(v), v // 1, v %% 1 end)" % v)
            s = self.c(STRS)
            lines.append("try(function() local s = %s return tonumber(s), s + 0, s * 1, math.tointeger(s), -s end)" % s)
        return lines

    def b_misc(self):
        lines = []
        k = self.r.randint(0, 6)
        if k == 0:
            lines.append("try(load, %s)" % lua_str(self.c(["return 1 +", "x = = 1", "return ...", "local x <foo> = 1", "goto nowhere", "break", "return 1, 2, 3", "for i=1 do end", "\"unfinished", "[[long", "return 0x", "return 1e", "return '\\q'", "return '\\300'", "return '\\u{110000000}'"])))
            lines.append("try(function() return load(%s)() end)" % lua_str(self.c(["return 1, 2", "return ... ", "return select('#', ...)", "error('in load')", "return _ENV == _G"])))
        elif k == 1:
            lines.append("try(function() local t = setmetatable({}, {__index = function(t, k) return k * 2 end}) return t[1], t[2.0], t[-1] end)")
            lines.append("try(function() local t = {} t[1.0] = 'a' t[2^53] = 'b' return t[1], math.type(next(t)), t[2^53 | 0] end)")
        elif k == 2:
            lines.append("try(function() return string.format('%s|%s|%s', 1, 1.0, -0.0), 2^63 == math.maxinteger + 1.0, math.maxinteger + 0.0 == 2^63 end)")
            lines.append("try(function() return 1 < 1.5, math.maxinteger < 2^63, math.mininteger <= -2^63, 2^53 + 1 == 2^53 + 1.0, (2^53 | 0) + 1 == 2^53 end)")
        elif k == 3:
            lines.append("try(function() return rawequal('a', 'a'), rawlen({1, 2}), rawlen('abc'), rawget({5}, 1), select(-1, 1, 2, 3) end)")
            lines.append("try(rawlen, 5) try(rawset, {}, nil, 1) try(rawset, {}, 0/0, 1) try(setmetatable, 1, {}) try(setmetatable, {}, 1)")
            lines.append("try(function() return getmetatable('abc').__index == string, getmetatable(setmetatable({}, {__metatable = 'locked'})) end)")
            lines.append("try(setmetatable, setmetatable({}, {__metatable = 1}), {})")
        elif k == 4:
            lines.append("try(function() local t = {} for i = 1, 10 do t[i] = i end for i = 1, 10, 2 do t[i] = nil end local c = 0 for k in pairs(t) do c = c + 1 end return c end)")
            lines.append("try(ipairs) try(pairs) try(next, {}, 'nokey') try(tostring) try(tonumber) try(tonumber, '10', 99) try(tonumber, 10, 16)")
        elif k == 5:
            lines.append("try(function() local s = 0 for i = 1, 100 do s = s + i * 0.1 end return s, string.format('%.20g', s) end)")
            lines.append("try(function() return 7 // 0.0, -7 // 0.0, 0/0 == 0/0, -7 % math.huge, 7 % -math.huge, 5.3 % -2, -5 % 3, 5 % -3, math.mininteger // -1, math.mininteger % -1 end)")
        else:
            lines.append("try(function() return #'abc' + 1, -'2', 10 .. 20, '10' + 1, '0x10' + 0, '1e1' * 1, ' 3 ' - 1 end)")
            lines.append("try(function() return math.floor(-0.0), math.ceil(-0.5), math.abs(math.mininteger), math.fmod(-6, 4), math.fmod(6, -4), math.fmod(math.mininteger, -1) end)")
            lines.append("try(math.fmod, 1, 0) try(math.fmod, 1, 0.0) try(function() return 1 // 0 end) try(function() return 1 % 0 end)")
        return lines


    def b_varnames(self):
        # runtime error messages naming the culprit variable
        lines = ["do local up = %s" % self.c(["nil", "{}", "'s'", "1", "true"])]
        exprs = ["up.x", "up + 1", "up()", "#up", "-up", "up .. 'a'", "up < 1", "up & 1",
                 "glob_%d.f" % self.r.randint(0, 3), "glob_t.a.b", "glob_t:m()", "string.nope()",
                 "math.huge.x", "({}).a.b", "(nil)()", "up[1][2]", "up.x.y", "local l = up return l.f",
                 "local l = {} return l.k.j", "local l = {} l.m:n()", "local s = 'x' return s.y.z",
                 "return (up or nil)[1]", "return up[up]", "for i = 1, up do end", "return 1 + {}",
                 "return 2^'x'", "return 'a' < 1", "return {} < {}", "return {} <= 1", "return math.maxinteger + 1.5 | 0",
                 "return 1.5 | 0", "return '1.5' | 0", "return 2^63 | 0", "return 1 // 0", "return 1 % 0",
                 "local a <const> = 'k' return a.x", "local t = setmetatable({}, {__index = function(t, k) return nil end}) return t.x.y",
                 "glob_t = {} return glob_t.a.b.c", "return ('x')()", "return print.x", "up = {} return up.a.b",
                 "local co = coroutine.create(print) return co.x", "return io.stdout.x", "return io.stdout + 1"]
        for _ in range(self.r.randint(3, 8)):
            e = self.c(exprs)
            if not e.startswith(("local", "return", "for", "glob_t =", "up =")):
                e = "return " + e
            lines.append("  try(function() %s end)" % e)
        lines.append("end")
        return lines

    def b_quote(self):
        lines = []
        for _ in range(self.r.randint(2, 6)):
            v = self.c(INTS + FLOATS + [lua_str(self.rand_subject()), '"\\r\\n\\t\\0001"', '"\\200\\255"', '"\\0"'])
            lines.append("try(function() local s = string.format('%%q', %s) return s, load('return ' .. s)() == %s end)" % (v, v))
        lines.append("try(string.format, '%%10q', 1) try(string.format, '%%q', {}) try(string.format, '%%q') try(string.format, '%%s', setmetatable({}, {__tostring = function() return 42 end}))")
        return lines

    def b_gc(self):
        n = self.r.randint(1, 6)
        lines = ["do local order = {} collectgarbage('stop')"]
        for i in range(n):
            lines.append("  setmetatable({}, {__gc = function() order[#order + 1] = %d end})" % i)
        if self.r.random() < 0.5:
            lines.append("  local keep = setmetatable({}, {__gc = function() order[#order + 1] = 'kept' end})")
        lines.append("  collectgarbage() collectgarbage() collectgarbage('restart')")
        lines.append("  show(table.concat(order, ','))")
        lines.append("  local wk = setmetatable({}, {__mode = '%s'})" % self.c(["k", "v", "kv"]))
        lines.append("  local a, b = {}, {} wk[a] = 1 wk[1] = b wk[{}] = 2 wk[3] = {} wk['s'] = 's' wk[b] = a")
        lines.append("  collectgarbage()")
        lines.append("  local c = 0 for k, v in pairs(wk) do c = c + 1 end show(c, wk[a], wk[1] == b, wk.s)")
        lines.append("  show(collectgarbage('isrunning'), type(collectgarbage('count')), collectgarbage('step', 0) ~= nil)")
        lines.append("  show(collectgarbage('%s'), collectgarbage('%s'))" % (self.c(["incremental", "generational"]), self.c(["incremental", "generational"])))
        lines.append("  try(collectgarbage, 'bogus')")
        lines.append("end")
        return lines

    def b_iter(self):
        lines = ["do local p = setmetatable({}, {__pairs = function(t) local i = 0 return function() i = i + 1 if i <= 3 then return i, i * 10 end end, t, nil end})",
                 "  local acc = {} for k, v in pairs(p) do acc[#acc + 1] = k .. '=' .. v end show(table.concat(acc, ' '))",
                 "  local q = setmetatable({}, {__index = function(t, i) if i <= %d then return i * 2 end end})" % self.r.randint(0, 5),
                 "  acc = {} for i, v in ipairs(q) do acc[#acc + 1] = v end show(table.concat(acc, ' '))",
                 "  try(function() for k, v in pairs(nil) do end end)",
                 "  try(function() for i in 1 do end end)",
                 "  try(function() for i, v in ipairs() do end end)",
                 "  local t = {%s}" % ", ".join(self.c(["1", "nil", "'a'", "2.5"]) for _ in range(self.r.randint(0, 6))),
                 "  acc = {} for i, v in ipairs(t) do acc[#acc + 1] = fmt(v) end show(table.concat(acc, ' '))",
                 "  show(next({}), next({10}), select('#', next({})))",
                 "end"]
        return lines

    def b_debug(self):
        lines = ["do local function f(a, b, ...) local c = a return debug.getinfo(1, 'nSlu') end",
                 "  local i = f(1, 2) show(i.nparams, i.isvararg, i.nups, i.what, i.short_src, i.linedefined, i.lastlinedefined, i.currentline, i.source:sub(1, 1))",
                 "  local function g(x) local y = x * 2 local n1, v1 = debug.getlocal(1, 1) local n2, v2 = debug.getlocal(1, 2) return n1, v1, n2, v2, debug.getlocal(1, 3) end",
                 "  show(g(21))",
                 "  show(debug.getlocal(f, 1), debug.getlocal(f, 2), debug.getlocal(f, 3), debug.getlocal(print, 1))",
                 "  local u1, u2 = 5, 6 local function h() return u1 + u2 end show(debug.getupvalue(h, 1), debug.getupvalue(h, 2), debug.getupvalue(h, 3))",
                 "  show(debug.setupvalue(h, 1, 50), h())",
                 "  show(debug.getinfo(print).what, debug.getinfo(print).short_src, debug.getinfo(1, 'l').currentline, debug.getinfo(100))",
                 "  try(debug.getinfo, 1, '>') try(debug.getinfo, {}, 'x') try(debug.getlocal, 100, 1)",
                 "  local tb = debug.traceback('msg', 1):gsub('0x%x+', 'ADDR') show(tb)",
                 "  show(debug.traceback(42), debug.traceback(nil) == nil, type(debug.traceback()))",
                 "  local co = coroutine.create(function() local z = 1 coroutine.yield() end) coroutine.resume(co)",
                 "  show((debug.traceback(co):gsub('0x%x+', 'ADDR'))) show(debug.getlocal(co, 1, 1))",
                 "  local mt = debug.getmetatable('x') show(mt.__index == string, debug.getregistry ~= nil)",
                 "  show(pcall(debug.sethook)) show(debug.gethook())",
                 "end"]
        return lines

    def b_dump(self):
        lines = ["do local k = %s local function src(a, ...) local t = {a, ...} return #t, k, select('#', ...) end" % self.c(["1", "'s'", "2.5", "nil"]),
                 "  for _, strip in ipairs({false, true}) do",
                 "    local d = string.dump(src, strip) local f = load(d, 'dumped', 'b')",
                 "    show(pcall(f, 1, 2, 3)) show(debug.setupvalue(f, 1, 'K'), pcall(f, 9))",
                 "  end",
                 "  try(load, string.dump(src), 'x', 't') try(load, 'return 1', 'x', 'b') try(string.dump, print)",
                 "  try(load(string.dump(load('return ...'))), 1, 2)",
                 "  local parts = {'return ', '1 ', '+ ', '2'} local i = 0 show(load(function() i = i + 1 return parts[i] end)())",
                 "  try(load, function() return {} end) try(load, function() error('rd') end)",
                 "  show(load('x = ...; return x', 'c', 't', {})(7))",
                 "  try(load('return a', '=name', 't', setmetatable({}, {__index = function(t, k) return k .. '!' end})))",
                 "end"]
        return lines

    def b_io(self):
        lines = ["do local f = io.tmpfile()",
                 "  f:write(%s, ' ', %s, '\\n', 'line2\\n', '0x1F 1e3 -.5 nan 12abc\\n')" % (self.c(INTS + FLOATS[:12]), self.c(INTS + FLOATS[:12])),
                 "  f:seek('set') show(f:read('n', 'n', 'l'))",
                 "  show(f:read('L')) show(f:read('n', 'n', 'n', 'n')) show(f:read('l'), f:read('a'), f:read('l'), f:read(0))",
                 "  f:seek('set', 2) show(f:read(3), f:seek('cur'), f:seek('end') > 0)",
                 "  try(f.read, f, 'x') try(f.seek, f, 'bad') show(io.type(f), io.type(1))",
                 "  f:close() show(io.type(f), pcall(f.read, f))",
                 "  try(io.lines, '/nonexistent/x')",
                 "end"]
        return lines

    def b_strcmp(self):
        lines = []
        pool = ['"a"', '"A"', '"a\\0"', '"a\\0b"', '""', '"\\0"', '"\\200"', '"ab"', '"b"', '"Z"', '"\\255"', '" "']
        for _ in range(self.r.randint(2, 6)):
            a, b = self.c(pool), self.c(pool)
            lines.append("show(%s < %s, %s <= %s, %s == %s, %s > %s)" % (a, b, a, b, a, b, a, b))
        return lines

    def b_uncaught(self):
        return [self.c(["error('top level')", "error({})", "error(nil)", "error(setmetatable({}, {__tostring = function() return 'TSERR' end}))",
                        "local t = nil; t.x = 1", "error('lvl2', 2)", "os.exit(3)", "os.exit(false)", "os.exit(true)",
                        "error(42)", "coroutine.wrap(function() error('in coro') end)()", "assert(false)", "assert(nil, 'amsg')",
                        "local function deep(n) if n == 0 then error('deep') end return (deep(n - 1)) end deep(30)"])]

    def program(self, nblocks: int) -> str:
        blocks = [self.b_arith, self.b_mathlib, self.b_compare, self.b_concat,
                  self.b_strlib, self.b_strlib, self.b_pack, self.b_utf8,
                  self.b_table, self.b_sort, self.b_meta, self.b_meta,
                  self.b_inherit, self.b_closure, self.b_vararg, self.b_error,
                  self.b_coroutine, self.b_coroutine, self.b_goto, self.b_tbc,
                  self.b_for, self.b_stringconv, self.b_misc, self.b_varnames,
                  self.b_quote, self.b_gc, self.b_iter, self.b_debug, self.b_dump,
                  self.b_io, self.b_strcmp, self.b_varnames]
        out = [HEADER,
               "MT1 = setmetatable({}, {__add = function(a, b) return 'MTadd' end, __concat = function(a, b) return 'MTcat' end, __lt = function() return true end, __le = function() return false end, __eq = function() return true end, __index = function(t, k) return k end, __len = function() return 3 end, __unm = function() return 'neg' end, __tostring = function() return 'MT1' end})",
               "MT2 = setmetatable({}, {__name = 'Obj'})"]
        for i in range(nblocks):
            out.append("-- block %d" % i)
            out.extend(self.c(blocks)())
        if self.r.random() < 0.15:
            out.extend(self.b_uncaught())
        out.append("print('end')")
        return "\n".join(out) + "\n"


ADDR = re.compile(rb"(?<=: )0x[0-9a-fA-F]+|(?<=\()0x[0-9a-fA-F]+(?=\))")


def normalise(b: bytes) -> bytes:
    return ADDR.sub(b"ADDR", b)


def run_one(interp: str, script: Path, timeout: float):
    try:
        p = subprocess.run([interp, script.name], cwd=script.parent, capture_output=True, timeout=timeout,
                           env={"PATH": "/usr/bin:/bin", "LC_ALL": "C", "TZ": "UTC"})
        err = p.stderr.replace(interp.encode(), b"lua")
        return p.returncode, normalise(p.stdout), normalise(err)
    except subprocess.TimeoutExpired:
        return "timeout", b"", b""


def cmd_gen(a):
    out = Path(a.outdir)
    out.mkdir(parents=True, exist_ok=True)
    for i in range(a.count):
        r = random.Random(a.seed * 1000003 + i)
        (out / ("f%05d.lua" % i)).write_text(Gen(r).program(r.randint(10, 40)))


def cmd_run(a):
    lua = str(Path(a.lua).resolve())
    ref = str(Path(a.ref).resolve())
    keep = Path(a.keep)
    keep.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="lua-fuzz-"))

    def job(i):
        r = random.Random(a.seed * 1000003 + i)
        src = Gen(r).program(r.randint(10, 40))
        d = work / ("w%d" % i)
        d.mkdir()
        f = d / "prog.lua"
        f.write_text(src)
        exp = run_one(ref, f, a.timeout)
        if exp[0] == "timeout":
            shutil.rmtree(d)
            return i, None
        got = run_one(lua, f, a.timeout * 4)
        bad = got != exp
        if bad:
            (keep / ("s%d_%d" % (a.seed, i))).mkdir(exist_ok=True)
            kd = keep / ("s%d_%d" % (a.seed, i))
            shutil.copy(f, kd / "prog.lua")
            (kd / "ref.out").write_bytes(b"exit %r\n" % (exp[0],) + exp[1] + b"\n--stderr--\n" + exp[2])
            (kd / "got.out").write_bytes(b"exit %r\n" % (got[0],) + got[1] + b"\n--stderr--\n" + got[2])
        shutil.rmtree(d)
        return i, bad

    nbad = nskip = 0
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as ex:
        for i, bad in ex.map(job, range(a.count)):
            if bad is None:
                nskip += 1
            elif bad:
                nbad += 1
                print("DIFF seed=%d prog=%d -> %s" % (a.seed, i, keep / ("s%d_%d" % (a.seed, i))), flush=True)
    shutil.rmtree(work, ignore_errors=True)
    print("ran %d programs (seed %d): %d diverged, %d skipped (reference timeout)" % (a.count, a.seed, nbad, nskip))
    return 1 if nbad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("outdir")
    g.add_argument("count", type=int)
    g.add_argument("--seed", type=int, default=1)
    r = sub.add_parser("run")
    r.add_argument("--lua", default=str(REPO / "build" / "lua"))
    r.add_argument("--ref", default=str(REPO / "target" / "lua-reference" / "bin" / "lua"))
    r.add_argument("--count", type=int, default=200)
    r.add_argument("--seed", type=int, default=1)
    r.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    r.add_argument("--timeout", type=float, default=10.0)
    r.add_argument("--keep", default=str(REPO / "build" / "lua-fuzz-fail"))
    a = ap.parse_args()
    sys.exit(cmd_gen(a) if a.cmd == "gen" else cmd_run(a))


if __name__ == "__main__":
    main()
