-- Arithmetic metamethods __add __sub __mul __div __mod __pow __idiv __unm:
-- dispatch from either operand (left operand's metamethod first), numbers
-- and strings on the other side, the arguments passed (unm gets the operand
-- twice), result truncation to one value, and string arithmetic coercion.

local V = {}
V.__index = V
local function vec(x, y) return setmetatable({x = x, y = y}, V) end
local function str(v) if getmetatable(v) == V then return "(" .. v.x .. "," .. v.y .. ")" end return tostring(v) end
local function lift(a) if type(a) == "number" then return vec(a, a) end return a end
V.__add = function(a, b) a, b = lift(a), lift(b); return vec(a.x + b.x, a.y + b.y) end
V.__sub = function(a, b) a, b = lift(a), lift(b); return vec(a.x - b.x, a.y - b.y) end
V.__mul = function(a, b) a, b = lift(a), lift(b); return vec(a.x * b.x, a.y * b.y) end
V.__div = function(a, b) a, b = lift(a), lift(b); return vec(a.x / b.x, a.y / b.y) end
V.__mod = function(a, b) a, b = lift(a), lift(b); return vec(a.x % b.x, a.y % b.y) end
V.__pow = function(a, b) a, b = lift(a), lift(b); return vec(a.x ^ b.x, a.y ^ b.y) end
V.__idiv = function(a, b) a, b = lift(a), lift(b); return vec(a.x // b.x, a.y // b.y) end
V.__unm = function(a, b) return vec(-a.x, -a.y), "ignored" end

local p, q = vec(7, 9), vec(2, 4)
print("add", str(p + q), str(p + 1), str(1 + p))
print("sub", str(p - q), str(p - 1), str(10 - p))
print("mul", str(p * q), str(p * 2), str(2 * p))
print("div", str(p / q), str(p / 2), str(18 / p))
print("mod", str(p % q), str(p % 4), str(20 % p))
print("pow", str(q ^ q), str(q ^ 2), str(2 ^ q))
print("idiv", str(p // q), str(p // 2), str(20 // p))
print("unm", str(-p), str(- -p), select('#', -p))

-- operand order and dispatch: left metamethod first, then right
local trace = {}
local function mk(name)
  local mt = {}
  for _, ev in ipairs({"__add", "__sub", "__mul", "__div", "__mod", "__pow", "__idiv", "__unm"}) do
    mt[ev] = function(a, b)
      trace[#trace + 1] = name .. ev .. "(" .. (type(a) == "table" and a.n or tostring(a)) .. "," .. (type(b) == "table" and b.n or tostring(b)) .. ")"
      return name
    end
  end
  return mt
end
local A = setmetatable({n = "A"}, mk("mA"))
local B = setmetatable({n = "B"}, mk("mB"))
local _ = A + B; _ = B + A; _ = 5 - A; _ = A * "s"; _ = "s" / B; _ = -A
print("dispatch", table.concat(trace, " "))
-- only the right operand has the metamethod
local plain = {n = "plain"}
trace = {}
_ = plain + B; _ = B % plain; _ = plain ^ A
print("right only", table.concat(trace, " "))
-- __unm receives the operand twice
local seen = {}
local U = setmetatable({}, {__unm = function(a, b) seen[1] = a; seen[2] = b; return 0 end})
_ = -U
print("unm args", seen[1] == U, seen[2] == U)

-- result is truncated to one value and may be any type
local multi = setmetatable({}, {__add = function() return 1, 2, 3 end, __sub = function() return nil end,
  __mul = function() return "str" end, __div = function() return false end})
print("results", select('#', multi + 1), multi + 1, multi - 1, multi * 1, multi / 1)

-- a metamethod can be a callable table
local callable = setmetatable({}, {__call = function(_, a, b) return "called" end})
local CT = setmetatable({}, {__add = callable})
print("callable mm", CT + 1)

-- strings are coerced to numbers by the string metatable's arithmetic
print("string arith", "10" + 1, "3" * "4", "2" ^ 3, -"2", "7" // 2, "7" % "4", "1.5" + 1, "0x10" + 0)
print("string arith types", math.type("10" + 1), math.type("10" + 1.0), math.type("1e1" + 0))
print("string int div", "9" / "3", math.type("9" // "3"))
print("string spaces", " 5 " + 0, "\t6\n" * 1)
print("bad string arith", pcall(function() return "abc" + 1 end))
print("bad string arith2", pcall(function() return 1 - "1x" end))
print("bad string unm", pcall(function() return -"z" end))
-- string metamethods defer to the other operand's metamethods
local SM = setmetatable({}, {__add = function(a, b) return "mm:" .. tostring(type(a)) end})
print("string + table", "abc" + SM, SM + "abc")

-- integer/float rules apply inside metamethod-free arithmetic too
print("idiv by zero int", pcall(function() return 1 // 0 end))
print("mod by zero int", pcall(function() return 1 % 0 end))
print("float div zero", 1 / 0, -1 / 0, 1 // 0.0, 1 % math.huge, -1 % math.huge)

-- missing metamethods
print("no mm", pcall(function() local t = {}; return t + 1 end))
print("no mm right", pcall(function() local t = {}; return 1 * t end))
print("no mm field", pcall(function() local t = {f = {}}; return t.f - 1 end))
print("no mm unm", pcall(function() local t = {}; return -t end))
print("no mm bool", pcall(function() return true + 1 end))
print("no mm nil global", pcall(function() return 2 ^ undefined_g end))
print("no mm upvalue", pcall(function() return plain + 1 end))
print("no mm two tables", pcall(function() return {} + {} end))
print("no mm constant", pcall(function() return {} // 1 end))
