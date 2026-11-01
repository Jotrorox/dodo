-- Multiple assignment: extra values dropped, missing ones become nil,
-- swaps and rotations, all right-hand expressions evaluated before any
-- assignment, function-call adjustment (last expression expands), locals
-- vs globals, and nested table targets. Only cases whose result is
-- defined by the manual are printed.

local a, b, c = 1, 2
print("missing", a, b, c)
a, b = 10, 20, 30
print("extra", a, b)
local x, y = (function() return 1, 2, 3 end)()
print("call expands", x, y)
local p, q, r = (function() return 1, 2 end)(), 5
print("call truncated", p, q, r)
local s, t, u = 0, (function() return 1, 2 end)()
print("call last", s, t, u)
local v, w = ((function() return 7, 8 end)())
print("paren", v, w)
local function none() end
local n1, n2 = none()
print("none", n1, n2)
local m1, m2, m3 = none(), none()
print("none twice", m1, m2, m3)

-- swaps and rotations
local i, j = 1, 2
i, j = j, i
print("swap", i, j)
local k1, k2, k3 = "a", "b", "c"
k1, k2, k3 = k2, k3, k1
print("rotate", k1, k2, k3)
local tab = {1, 2, 3}
tab[1], tab[3] = tab[3], tab[1]
print("swap fields", tab[1], tab[2], tab[3])

-- all right-hand sides are evaluated before assignment
local e1, e2 = 1, 2
e1, e2 = e2 + 10, e1 + 10
print("rhs first", e1, e2)
local arr = {"x", "y", "z"}
local idx = 1
idx, arr[idx] = idx + 1, "changed"
-- the manual leaves the order of evaluation in assignments undefined, so
-- arr[idx] may use the old or the new idx; check only that idx is 2 and
-- exactly one of arr[1]/arr[2] changed
print("index target", idx, (arr[1] == "changed") ~= (arr[2] == "changed"), arr[3])

-- the evaluation order of right-hand sides is not specified by the
-- manual; only check that every function was called exactly once
local calls = {}
local function rec(name, val) calls[#calls + 1] = name; return val end
local r1, r2, r3 = rec("a", 1), rec("b", 2), rec("c", 3)
table.sort(calls)
print("each called", table.concat(calls), r1, r2, r3)

-- globals and locals mixed
g1, g2 = "G1", "G2"
local l1
l1, g3 = "L1", "G3"
print("globals", g1, g2, g3, l1)
g1, g2 = nil
print("globals nil", g1, g2)

-- nested table targets
local obj = {inner = {}}
obj.inner.a, obj.inner.b, obj.c = 1, 2, 3
print("nested", obj.inner.a, obj.inner.b, obj.c)
local key = "dyn"
obj[key], obj[key .. "2"] = "d1", "d2"
print("computed keys", obj.dyn, obj.dyn2)

-- assignment to the same variable twice: result is one of the values
local dup
dup, dup = 1, 2
print("same target", dup == 1 or dup == 2)

-- local declarations: the new locals are not in scope in their own
-- initializers
local lx = "outer"
do
  local lx, ly = "inner", lx
  print("initializer scope", lx, ly)
end
local function scope_test()
  local z = 5
  local z = z + 1
  return z
end
print("redeclare", scope_test())

-- assignment through __newindex runs for each target
local log = {}
local proxy = setmetatable({}, {__newindex = function(_, k2, v2) log[#log + 1] = k2 .. "=" .. tostring(v2) end})
proxy.a, proxy.b, proxy.c = 1, 2
table.sort(log)
print("newindex", table.concat(log, " "))

-- vararg adjustment in assignment
local function va(...)
  local q1, q2, q3 = ...
  return q1, q2, q3
end
print("va", va(1), va(1, 2, 3, 4))

-- long right-hand side lists
local z1, z2, z3, z4, z5, z6, z7, z8, z9, z10 = 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12
print("long", z1, z5, z10)
-- assignment statements are not expressions
print("syntax", load("x = y = 1") == nil, load("local a = (b = 1)") == nil)
print("bad target", select(2, load("1 = 2")))
print("bad target call", select(2, load("f() = 1")))
