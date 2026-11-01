-- Integer-oriented math functions: floor/ceil (integer results when
-- representable, floats otherwise), fmod with integers and floats
-- (negative operands, zero divisor errors, minint % -1), abs(minint)
-- wrapping, max/min preserving subtypes, ult, tointeger, math.type,
-- maxinteger/mininteger and integer overflow wraparound.

local function show(...)
  local t = table.pack(...)
  for i = 1, t.n do
    local v = t[i]
    t[i] = (math.type(v) or type(v)) .. ":" .. tostring(v)
  end
  return table.concat(t, " ", 1, t.n)
end

-- floor / ceil
print(show(math.floor(3.7), math.floor(-3.7), math.floor(5), math.floor(-0.0)))
print(show(math.ceil(3.2), math.ceil(-3.2), math.ceil(5), math.ceil(-0.5)))
print(show(math.floor(2^62), math.floor(2^63), math.floor(-2^63), math.ceil(-2^63 - 1025)))
print(show(math.floor(1e100), math.ceil(-1e100), math.floor(math.huge), math.ceil(-math.huge)))
print(show(math.floor(0/0) ~= math.floor(0/0)))
print(show(math.floor("3.5"), math.ceil("2")))
print(pcall(math.floor, "x"))

-- fmod
print(show(math.fmod(7, 3), math.fmod(-7, 3), math.fmod(7, -3), math.fmod(-7, -3)))
print(show(math.fmod(7.5, 2), math.fmod(-7.5, 2), math.fmod(7, 2.5)))
print(show(math.fmod(math.mininteger, -1), math.fmod(math.maxinteger, -1)))
print(show(math.fmod(5, math.huge), math.fmod(-0.0, 1)))
local nan = math.fmod(1, 0.0)
print("fmod(1, 0.0) is nan:", nan ~= nan, math.type(nan))
print(pcall(math.fmod, 1, 0))
print(pcall(math.fmod, 1))
print(pcall(math.fmod, 1, "a"))
-- compare with the % operator (floor division semantics)
print(show(-7 % 3, 7 % -3, -7.5 % 2, math.mininteger % -1, 5 % math.huge, -5 % math.huge))

-- abs
print(show(math.abs(-5), math.abs(5), math.abs(-5.5), math.abs(-0.0)))
print(show(math.abs(math.mininteger), math.abs(math.mininteger + 1)))

-- max / min keep the original value and subtype
print(show(math.max(1, 2.0), math.max(2.0, 1), math.max(2, 2.0), math.max(2.0, 2)))
print(show(math.min(1, 1.0), math.min(1.0, 1), math.min(-0.0, 0), math.max(-0.0, 0)))
print(show(math.max(3), math.min(-1, -5, 10, -5.0), math.max(math.mininteger, -math.huge)))
print(pcall(math.max))
print(pcall(math.min))
print(pcall(math.max, 1, "x"))
-- max/min compare with "<" (no numeric conversion): strings compare as strings
print(pcall(math.max, "10", 9))
print(show(math.max("10", "9"), math.min("b", "a", "c")))

-- ult
print(math.ult(1, 2), math.ult(-1, 2), math.ult(2, -1), math.ult(math.maxinteger, math.mininteger))
print(pcall(math.ult, 1.5, 2))
print(show(math.ult(1.0, 2)))

-- tointeger
print(show(math.tointeger(3.0), math.tointeger(3.5), math.tointeger(2^63), math.tointeger(-2^63)))
print(show(math.tointeger("8"), math.tointeger("x"), math.tointeger({}), math.tointeger(7)))
print(show(math.tointeger(-0.0), math.tointeger(0/0), math.tointeger(math.huge)))
print(pcall(math.tointeger))

-- math.type
print(math.type(1), math.type(1.0), math.type("1"), math.type(nil), math.type({}))
print(pcall(math.type))

-- limits and wraparound
print(math.maxinteger, math.mininteger, math.maxinteger + 1 == math.mininteger)
print(math.mininteger - 1 == math.maxinteger, -math.mininteger == math.mininteger)
print(math.maxinteger * 2, math.mininteger * -1, math.maxinteger // -1)
print(show(math.maxinteger + 0.0, math.mininteger + 0.0, math.maxinteger + 0.0 == 2^63))
print(show(1 << 63, (1 << 63) - 1, 1 << 64, -1 >> 1, -1 >> 63))
print(show(3 // 2, -3 // 2, 3.0 // 2, -3 // 2.0, 7 // -2))
print(show(2^2, 10 / 2, 3 * 1.0, 6 // 2))
