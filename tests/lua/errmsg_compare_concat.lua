-- Error messages for comparisons (order operators between incompatible
-- types, two tables, two booleans...) and for concatenation of nil,
-- boolean, table and function values with variable info. Equality never
-- raises errors.

local function try(f)
  print(select(2, pcall(f)))
end

-- order comparisons
try(function() return 1 < nil end)
try(function() return nil < 1 end)
try(function() local x; return x <= 2 end)
try(function() return 1 > nil end)
try(function() return {} < {} end)
try(function() return {} <= {} end)
try(function() return true < false end)
try(function() return "a" < 1 end)
try(function() return 1 >= "a" end)
try(function() return print < print end)
try(function() return {} < 1 end)
try(function() return 2.5 > {} end)
try(function() local co = coroutine.create(print); return co < co end)
try(function() return nil < nil end)
try(function() local t = {}; return t.a < t.b end)

-- valid mixed comparisons
print(1 < 1.5, 2^53 == 2^53 + 1, "a" < "b", "Z" < "a", "" < "a", "abc" < "abd")
print(1 == "1", {} == {}, nil == false, 0 == -0, 0.0 == -0.0)

-- __lt / __le on one operand only
local mt = {__lt = function() return true end, __le = function() return false end}
local o = setmetatable({}, mt)
print(o < 1, 1 < o, o <= {}, o > 1, o >= 1)
-- __lt result converted to boolean
local mt2 = {__lt = function() return "yes" end, __le = function() return nil end}
local p = setmetatable({}, mt2)
print(p < p, p <= p, p > p, p >= p)
-- __le missing: the default build (LUA_COMPAT_5_3) falls back to not (b < a)
local onlylt = setmetatable({}, {__lt = function() return true end})
try(function() return onlylt <= onlylt end)

-- concatenation
try(function() local x; return "a" .. x end)
try(function() local x; return x .. "a" end)
try(function() return "a" .. undefined_c end)
try(function() local t = {}; return "a" .. t.f end)
try(function() local b = true; return b .. "" end)
try(function() return "x" .. {} end)
try(function() return {} .. "x" end)
try(function() return print .. "x" end)
local upv
try(function() return "a" .. upv end)
try(function() local a, b, c = "x", "y", nil; return a .. b .. c end)
try(function() local a, b, c = nil, "y", "z"; return a .. b .. c end)
try(function() local a, b, c = "x", nil, "z"; return a .. b .. c end)

-- numbers concatenate with their string conversion
print(1 .. 2, 1.0 .. "", -0.0 .. "", 2^63 .. "", math.mininteger .. "", 1e100 .. "")
print(0x10 .. "|" .. 1e15 .. "|" .. 1e16 .. "|" .. 0.1 .. "|" .. 1/3)

-- __concat on either side
local cmt = {__concat = function(a, b)
  local function s(v) return type(v) == "table" and "T" or tostring(v) end
  return s(a) .. "+" .. s(b)
end}
local c = setmetatable({}, cmt)
print(c .. "x", "x" .. c, 1 .. c, c .. c, "a" .. "b" .. c)
try(function() return c .. nil end)
