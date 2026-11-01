-- Comparison metamethods: __eq (only for two tables / two userdata, not
-- called for identical objects or primitives, result converted to boolean,
-- ~= negates), __lt and __le (either operand, results to boolean, > and >=
-- swap operands), the __le emulation through "not (b < a)" that the
-- reference build keeps (default makefile defines LUA_COMPAT_5_3, which
-- enables LUA_COMPAT_LT_LE), and the error messages for incomparable values.

local trace = {}
local function named(n, mt) return setmetatable({n = n}, mt) end
local EQ = {__eq = function(a, b)
  trace[#trace + 1] = "eq(" .. tostring(a.n) .. "," .. tostring(b.n) .. ")"
  return a.v == b.v
end}
local a = named("a", EQ); a.v = 1
local b = named("b", EQ); b.v = 1
local c = named("c", EQ); c.v = 2
print("a==b", a == b, "a==c", a == c, "a~=b", a ~= b, "a~=c", a ~= c)
print("trace", table.concat(trace, " "))
trace = {}
print("same object", a == a, a ~= a, #trace)
print("vs number", a == 1, 1 == a, a ~= "x", #trace)
print("vs nil", a == nil, nil == a, #trace)
-- only one side has __eq: still called (the first operand's, else second's)
local plain = {n = "plain", v = 1}
print("plain==a", plain == a, "a==plain", a == plain)
print("trace", table.concat(trace, " "))
trace = {}
-- result converted to boolean
local truthy = setmetatable({}, {__eq = function() return "yes" end})
local falsy = setmetatable({}, {__eq = function() return nil end})
print("eq converted", truthy == {}, falsy == {}, truthy ~= {}, falsy ~= {})
print("eq type", type(truthy == {}))
-- different metamethods on the two sides: the left one is used
local L = setmetatable({}, {__eq = function() return true end})
local R = setmetatable({}, {__eq = function() return false end})
print("left wins", L == R, R == L)
-- rawequal bypasses __eq
print("rawequal", rawequal(a, b), rawequal(a, a), rawequal(1, 1.0), rawequal("x", "x"))
-- table keys use raw equality
local keyed = {[a] = "A"}
print("raw keys", keyed[a], keyed[b])

-- __lt and __le
local O = {}
O.__lt = function(x, y)
  local xv = type(x) == "table" and x.v or x
  local yv = type(y) == "table" and y.v or y
  trace[#trace + 1] = "lt(" .. tostring(xv) .. "," .. tostring(yv) .. ")"
  return xv < yv
end
O.__le = function(x, y)
  local xv = type(x) == "table" and x.v or x
  local yv = type(y) == "table" and y.v or y
  trace[#trace + 1] = "le(" .. tostring(xv) .. "," .. tostring(yv) .. ")"
  return xv <= yv
end
local one, two = setmetatable({v = 1}, O), setmetatable({v = 2}, O)
print("lt", one < two, two < one, one > two, two > one)
print("le", one <= two, two <= one, one >= two, two >= one)
print("trace", table.concat(trace, " "))
trace = {}
print("with numbers", one < 5, 5 < one, one <= 1, 0 >= two)
print("trace", table.concat(trace, " "))
trace = {}
-- result conversion
local LT = setmetatable({}, {__lt = function() return 0 end, __le = function() return false end})
print("lt converted", LT < LT, LT <= LT, LT > LT, LT >= LT, type(LT < LT))

-- without __le, a <= b is computed as not (b < a) in the reference build
-- (LUA_COMPAT_LT_LE); a pure 5.4 build without compat would raise instead
local onlylt = setmetatable({v = 1}, {__lt = function(x, y)
  trace[#trace + 1] = "lt(" .. tostring(x.v) .. "," .. tostring(y.v) .. ")"
  return x.v < y.v
end})
local onlylt2 = setmetatable({v = 2}, getmetatable(onlylt))
print("only __lt <", pcall(function() return onlylt < onlylt2 end))
print("only __lt <=", pcall(function() return onlylt <= onlylt2 end))
print("only __lt >=", pcall(function() return onlylt >= onlylt2 end))
print("trace", table.concat(trace, " "))
trace = {}

-- comparisons across mixed types without metamethods
local function try(label, f) print(label, pcall(f)) end
try("two tables", function() return {} < {} end)
try("two tables le", function() return {} <= {} end)
try("number < table", function() return 1 < {} end)
try("table < number", function() return {} < 1 end)
try("number < string", function() return 1 < "2" end)
try("string >= number", function() return "2" >= 1 end)
try("nil < number", function() local x; return x < 1 end)
try("bool < bool", function() return true < false end)
try("function", function() return print < print end)
-- equality across types never errors
print("eq mixed", 1 == "1", "1" == 1, {} == {}, print == print, 1 == 1.0, 0.0 == -0.0)
print("ne mixed", 1 ~= "1", nil ~= false)
-- strings compare bytewise, numbers numerically incl. int/float
print("strings", "a" < "b", "abc" < "abd", "" < "a", "Z" < "a", "a\0b" < "a\0c", "10" < "9")
print("numbers", 1 < 1.5, 2^53 < 2^53 + 1, math.maxinteger < 2^63, math.maxinteger + 0.0 == 2^63,
  math.mininteger <= -2^63, 1 < 0/0, 0/0 < 1, 0/0 == 0/0, 0/0 ~= 0/0)
print("int vs float exact", math.maxinteger == math.maxinteger + 0.0, 2^53 == 2^53 + 1,
  (2^53 | 0) + 1 == 2^53 + 1.0)
