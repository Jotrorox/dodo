#!/usr/bin/env python3
"""Random Lua program generator for differential testing against luac.

    python3 tests/lua/fuzz.py OUTDIR COUNT [SEED]

Writes COUNT syntactically valid programs that mix every statement form,
operators with constants of all kinds (to exercise folding, immediate and
K operands), short-circuit logic in value and jump contexts, closures,
multiple assignment conflicts, varargs, methods, gotos and attributes.
"""
import os
import random
import sys

out = sys.argv[1]
count = int(sys.argv[2])
seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
os.makedirs(out, exist_ok=True)

BINOPS = ["+", "-", "*", "/", "//", "%", "^", "&", "|", "~", "<<", ">>", "..",
          "==", "~=", "<", "<=", ">", ">=", "and", "or"]
UNOPS = ["- ", "not ", "#", "~"]
NUMS = ["0", "1", "2", "-1", "127", "128", "-127", "-128", "129", "255", "256",
        "1.0", "0.5", "-0.0", "2.5", "1e10", "3.0", "-2.0", "65535", "65536",
        "-65535", "-65536", "65537", "0x7fffffffffffffff", "9007199254740993",
        "1e300", "0.1", "100000", "-100000", "math.huge", "1//0", "0/0"]
STRS = ['"a"', '"b"', "'xyz'", '"10"', '"0x10"', '""', '"long string constant number one!!!!!!!!!!"']


class Gen:
    def __init__(self, rnd):
        self.r = rnd
        self.depth = 0
        self.labels = 0
        self.consts = set()

    def choice(self, xs):
        return self.r.choice(xs)

    def name(self, scope):
        vs = [v for v in scope if v != "..."]
        if vs and self.r.random() < 0.8:
            return self.choice(vs)
        return self.choice(["g1", "g2", "gt", "print", "_ENV", "math"])

    def expr(self, scope, d=0):
        r = self.r.random()
        if d > 3 or r < 0.25:
            return self.atom(scope)
        if r < 0.55:
            op = self.choice(BINOPS)
            return "%s %s %s" % (self.expr(scope, d + 1), op, self.expr(scope, d + 1))
        if r < 0.65:
            return self.choice(UNOPS) + self.paren(scope, d)
        if r < 0.72:
            return "(%s)" % self.expr(scope, d + 1)
        if r < 0.80:
            return self.call(scope, d)
        if r < 0.86:
            return self.table(scope, d)
        if r < 0.92:
            return "%s[%s]" % (self.name(scope), self.expr(scope, d + 1))
        if r < 0.96:
            return "%s.%s" % (self.name(scope), self.choice(["x", "y", "z", "k"]))
        return self.func(scope, d)

    def paren(self, scope, d):
        e = self.expr(scope, d + 1)
        return "(%s)" % e if " " in e else e

    def atom(self, scope):
        r = self.r.random()
        if r < 0.35:
            return self.name(scope)
        if r < 0.65:
            return self.choice(NUMS)
        if r < 0.78:
            return self.choice(STRS)
        if r < 0.88:
            return self.choice(["nil", "true", "false"])
        if r < 0.94 and "..." in scope:
            return "..."
        return self.choice(NUMS)

    def args(self, scope, d):
        n = self.r.randint(0, 3)
        return ", ".join(self.expr(scope, d + 1) for _ in range(n))

    def call(self, scope, d):
        r = self.r.random()
        f = self.name(scope)
        if r < 0.5:
            return "%s(%s)" % (f, self.args(scope, d))
        if r < 0.7:
            return "%s:%s(%s)" % (f, self.choice(["m", "n"]), self.args(scope, d))
        if r < 0.85:
            return "%s %s" % (f, self.choice(STRS))
        return "%s.%s(%s)" % (f, self.choice(["x", "y"]), self.args(scope, d))

    def table(self, scope, d):
        items = []
        for _ in range(self.r.randint(0, 6)):
            r = self.r.random()
            if r < 0.5:
                items.append(self.expr(scope, d + 1))
            elif r < 0.8:
                items.append("%s = %s" % (self.choice(["x", "y", "k1"]), self.expr(scope, d + 1)))
            else:
                items.append("[%s] = %s" % (self.expr(scope, d + 1), self.expr(scope, d + 1)))
        sep = self.choice([", ", "; "])
        return "{" + sep.join(items) + "}"

    def func(self, scope, d):
        params = ["p%d" % i for i in range(self.r.randint(0, 3))]
        vararg = self.r.random() < 0.4
        inner = scope + params + (["..."] if vararg else [])
        inner = [v for v in inner if v != "..."] + (["..."] if vararg else [])
        plist = ", ".join(params + (["..."] if vararg else []))
        body = self.block(inner, 2, True)
        return "function(%s)\n%s\nend" % (plist, body)

    def target(self, scope):
        r = self.r.random()
        if r < 0.55:
            nm = self.name(scope)
            return nm if nm not in self.consts else "g1"
        if r < 0.8:
            return "%s[%s]" % (self.name(scope), self.expr(scope, 2))
        return "%s.%s" % (self.name(scope), self.choice(["x", "y"]))

    def block(self, scope, n, ret=False):
        scope = list(scope)
        lines = []
        for _ in range(self.r.randint(1, n)):
            lines.append(self.stat(scope))
        if ret and self.r.random() < 0.3:
            lines.append("return " + ", ".join(self.expr(scope) for _ in range(self.r.randint(0, 3))))
        return "\n".join(lines)

    def cond(self, scope):
        return self.expr(scope, 1)

    def stat(self, scope):
        self.depth += 1
        try:
            if self.depth > 4:
                return "%s = %s" % (self.target(scope), self.expr(scope, 2))
            r = self.r.random()
            vs = [v for v in scope if v != "..."]
            if r < 0.14:
                n = self.r.randint(1, 3)
                names = ["l%d" % self.r.randint(0, 9) for _ in range(n)]
                attrs = [self.choice(["", "", "", "", "", "", " <const>"]) for _ in names]
                for nm, at in zip(names, attrs):
                    if at:
                        self.consts.add(nm)
                    else:
                        self.consts.discard(nm)
                if self.r.random() < 0.1:
                    attrs[-1] = " <close>"
                decl = ", ".join(a + b for a, b in zip(names, attrs))
                if self.r.random() < 0.8:
                    s = "local %s = %s" % (decl, ", ".join(self.expr(scope, 1) for _ in range(self.r.randint(1, 3))))
                else:
                    s = "local " + decl
                scope.extend(names)
                return s
            if r < 0.30:
                n = self.r.randint(1, 3)
                return "%s = %s" % (", ".join(self.target(scope) for _ in range(n)),
                                    ", ".join(self.expr(scope, 1) for _ in range(self.r.randint(1, 3))))
            if r < 0.40:
                return self.call(scope, 1)
            if r < 0.50:
                s = "if %s then\n%s\n" % (self.cond(scope), self.block(scope, 3))
                for _ in range(self.r.randint(0, 2)):
                    s += "elseif %s then\n%s\n" % (self.cond(scope), self.block(scope, 2))
                if self.r.random() < 0.5:
                    s += "else\n%s\n" % self.block(scope, 2)
                return s + "end"
            if r < 0.56:
                return "while %s do\n%s\nend" % (self.cond(scope), self.loopbody(scope))
            if r < 0.61:
                return "repeat\n%s\nuntil %s" % (self.loopbody(scope), self.cond(scope + ["l1"]))
            if r < 0.67:
                v = "i%d" % self.r.randint(0, 3)
                step = (", " + self.expr(scope, 2)) if self.r.random() < 0.4 else ""
                return "for %s = %s, %s%s do\n%s\nend" % (v, self.expr(scope, 2), self.expr(scope, 2), step,
                                                         self.loopbody(scope + [v]))
            if r < 0.72:
                ks = ["k", "v", "w"][: self.r.randint(1, 3)]
                return "for %s in %s do\n%s\nend" % (", ".join(ks), ", ".join(self.expr(scope, 2) for _ in range(self.r.randint(1, 4))),
                                                    self.loopbody(scope + ks))
            if r < 0.77:
                return "do\n%s\nend" % self.block(scope, 3, True)
            if r < 0.82:
                nm = self.choice(["lf", "lg"])
                scope.append(nm)
                pl = self.choice(["", "a", "a, b", "...", "a, ..."])
                inner = [v for v in scope if v != "..."] + ["a", "b"] + (["..."] if "..." in pl else [])
                return "local function %s(%s)\n%s\nend" % (nm, pl, self.block(inner, 3, True))
            if r < 0.86:
                pl = self.choice(["", "x", "x, ..."])
                inner = [v for v in scope if v != "..."] + ["x"] + (["..."] if "..." in pl else [])
                return "function %s(%s)\n%s\nend" % (self.choice(["gf", "gt.f", "gt.a.b", "gt:m", "gt.a:m"]),
                                                    pl, self.block(inner, 3, True))
            if r < 0.90:
                self.labels += 1
                lb = "L%d" % self.labels
                return "do\ngoto %s\n%s\n::%s::\nend" % (lb, self.block(scope, 2), lb)
            if r < 0.93:
                self.labels += 1
                lb = "B%d" % self.labels
                return "do\n::%s::\n%s\nif %s then goto %s end\nend" % (lb, self.block(scope, 2), self.cond(scope), lb)
            return "%s = %s" % (self.target(scope), self.expr(scope, 1))
        finally:
            self.depth -= 1

    def loopbody(self, scope):
        body = self.block(scope, 3)
        if body.split("\n")[-1].startswith("return"):
            body = "do " + body + " end"
        if self.r.random() < 0.3:
            body += "\nif %s then break end" % self.cond(scope)
        if self.r.random() < 0.2:
            body += "\nlocal c = function() return %s end" % self.name(scope)
        return body


rnd = random.Random(seed)
for i in range(count):
    g = Gen(rnd)
    prog = g.block(["..."], 12, True)
    with open(os.path.join(out, "f%05d.lua" % i), "w") as f:
        f.write(prog + "\n")
