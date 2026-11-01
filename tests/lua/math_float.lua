-- Floating-point math functions: modf (integral and fractional parts,
-- infinities, negative zero), huge, pi, sqrt/exp/log with bases 2, 10
-- and others, trig functions, and the compat functions of the default
-- build (math.pow & co. exist only with LUA_COMPAT_MATHLIB: only the
-- standard set is checked here). Values printed with %.10g / %.15g / %g.
-- Note modf's first result is an integer when representable.

local function g(...)
  local t = table.pack(...)
  for i = 1, t.n do
    local v = t[i]
    if math.type(v) == "float" then t[i] = string.format("%.10g", v)
    elseif math.type(v) == "integer" then t[i] = "int:" .. v
    else t[i] = tostring(v) end
  end
  return table.concat(t, " ", 1, t.n)
end

-- modf
print(g(math.modf(3.7)))
print(g(math.modf(-3.7)))
print(g(math.modf(5)))
print(g(math.modf(-5)))
print(g(math.modf(0.0)))
print(string.format("%g %g", math.modf(-0.5)))
print(string.format("%g %g", math.modf(math.huge)))
print(string.format("%g %g", math.modf(-math.huge)))
print(math.type((math.modf(3.7))), math.type(select(2, math.modf(3.7))))
print(math.type((math.modf(2^53))), math.type((math.modf(1e100))))
local nanint, nanfrac = math.modf(0/0)
print(nanint ~= nanint, nanfrac ~= nanfrac)

-- constants
print(string.format("%.15g", math.pi), math.huge, -math.huge, math.huge == 1/0)
print(math.type(math.pi), math.type(math.huge))

-- sqrt / exp / log
print(g(math.sqrt(16), math.sqrt(2), math.sqrt(0), math.sqrt(-0.0)))
print(math.sqrt(-1) ~= math.sqrt(-1), math.sqrt(math.huge))
print(g(math.exp(0), math.exp(1), math.exp(-1), math.exp(710) == math.huge))
print(g(math.log(1), math.log(math.exp(2)), math.log(0) == -math.huge))
print(g(math.log(8, 2), math.log(1024, 2), math.log(2^50, 2), math.log(3, 2)))
print(g(math.log(1000, 10), math.log(1e-3, 10), math.log(12345, 10)))
print(g(math.log(81, 3), math.log(100, 100), math.log(2, 4), math.log(10, 0.5)))
local l11 = math.log(1, 1)
print(l11 ~= l11, g(math.log(2, 1)))   -- NaN itself is never printed (sign varies by CPU)
print(g(math.log(8, 2.0), math.log("8", "2")))
print(pcall(math.log))
print(pcall(math.log, "x"))

-- trigonometry
print(g(math.sin(0), math.cos(0), math.tan(0)))
print(g(math.sin(math.pi / 2), math.cos(math.pi), math.sin(math.pi / 6)))
print(g(math.asin(1), math.acos(-1), math.atan(1)))
print(g(math.atan(1, 1), math.atan(-1, -1), math.atan(0, -1), math.atan(1, 0)))
print(g(math.atan(0.0, -0.0), math.atan(-0.0, -0.0), math.atan(1)))
print(g(math.rad(180), math.deg(math.pi), math.deg(1)))

-- results are always floats for these functions, even with integer input
print(math.type(math.sqrt(4)), math.type(math.exp(0)), math.type(math.log(1)), math.type(math.sin(0)))

-- float formatting of special values
print(1e15, 1e16, 123456789012345.0, 0.1 + 0.2, 100 / 3, -1e-300, 2^-1074)
print(string.format("%.17g %.17g", 0.1, 1/3))
print(string.format("%a", 1.0) == "0x1p+0", string.format("%5.1f|%-8.3e|%G", 3.14159, 12345.678, 1e-10))

-- standard functions present
local names = {}
for k, v in pairs(math) do names[#names + 1] = k end
table.sort(names)
local std = {}
local standard = {abs=1, ceil=1, cos=1, deg=1, exp=1, floor=1, fmod=1, huge=1, log=1, max=1,
  maxinteger=1, min=1, mininteger=1, modf=1, pi=1, rad=1, random=1, randomseed=1, sin=1,
  sqrt=1, tan=1, tointeger=1, type=1, ult=1, asin=1, acos=1, atan=1}
for _, k in ipairs(names) do if standard[k] then std[#std + 1] = k end end
print(#std, table.concat(std, " "))
