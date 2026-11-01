-- Table key semantics: nil and NaN keys are rejected on write ("index is
-- nil", "index is NaN") but reading them yields nil; integral float keys are
-- normalized to integers; strings and numbers are distinct keys; booleans,
-- tables and functions work as keys by identity.

local function try(label, f)
  local ok, err = pcall(f)
  print(label, ok, err)
end

local t = {}
try("write nil key", function() t[nil] = 1 end)
try("write NaN key", function() t[0/0] = 1 end)
try("read nil key", function() return t[nil] end)
print("read nil", t[nil], "read NaN", t[0/0])
try("rawset nil", function() rawset(t, nil, 1) end)
try("rawset NaN", function() rawset(t, 0/0, 1) end)
print("rawget nil", rawget(t, nil), rawget(t, 0/0))
try("ctor nil key", function() return {[nil] = 1} end)
try("ctor NaN key", function() local nan = 0/0; return {[nan] = 1} end)
-- assigning nil to a nil key still raises
try("write nil to nil key", function() t[nil] = nil end)

-- float keys normalized to integers
local f = {}
f[1.0] = "one"
print("1.0 -> 1", f[1], math.type(next(f)))
f[2] = "two"
print("2 via 2.0", f[2.0])
f[-0.0] = "negzero"
print("-0.0 -> 0", f[0], math.type((next({[-0.0] = 1}))))
f[2^53] = "big"
print("2^53", f[math.tointeger(2^53)], math.type((next({[2^53] = 1}))))
f[1.5] = "frac"
print("1.5 stays float", f[1.5], f[1], math.type((next({[1.5] = 1}))))
-- 2^63 is not representable as an integer: stays a float key
local g = {}
g[2^63] = "huge"
print("2^63", g[2^63], math.type((next(g))), g[math.maxinteger])
g = {}
g[-2^63] = "minint"
print("-2^63", g[math.mininteger], math.type((next(g))))
-- infinity is a valid (float) key
g = {[math.huge] = "inf", [-math.huge] = "-inf"}
print("inf keys", g[1/0], g[-1/0])

-- strings and numbers are distinct
local s = {}
s[1] = "number one"
s["1"] = "string one"
print("1 vs '1'", s[1], s["1"], s[tonumber("1")], s[tostring(1)])
s["10"] = "s10"
print("no coercion", s[10], s["10"], s[1e1])

-- booleans as keys
local b = {[true] = "yes", [false] = "no"}
print("bool keys", b[true], b[false], b[1 == 1], b[1 == 2])

-- table and function keys by identity
local k1, k2 = {}, {}
local fk = function() end
local m = {[k1] = "k1", [k2] = "k2", [fk] = "fn", [print] = "print"}
print("identity", m[k1], m[k2], m[fk], m[print], m[{}])

-- keys equal under == but with different subtypes
local n = {}
n[3] = "int3"
print("3.0 lookup", n[3.0], n[6/2], n[3.0000000000000004])
n[0.1 + 0.2] = "fp"
print("0.1+0.2", n[0.3], n[0.1 + 0.2])

-- integer key extremes
local e = {[math.maxinteger] = "max", [math.mininteger] = "min"}
print("extremes", e[math.maxinteger], e[math.mininteger], e[math.maxinteger + 0.0])

-- removing keys
local r = {a = 1, b = 2}
r.a = nil
print("removed", r.a, r.b)
local cnt = 0
for _ in pairs(r) do cnt = cnt + 1 end
print("count after remove", cnt)

-- reading fields of non-tables
try("index nil", function() local x; return x.y end)
try("index number field", function() local x = 5; return x.y end)
try("index boolean", function() return (true).y end)
try("newindex nil", function() local x; x.y = 1 end)
try("newindex string", function() local x = "s"; x.y = 1 end)
try("index global nil", function() return undefined_global.field end)
try("index field chain", function() local x = {a = {}}; return x.a.b.c end)
-- strings have a metatable: indexing with a missing key yields nil
print("string index", ("abc").nosuch, ("abc").len == string.len)
