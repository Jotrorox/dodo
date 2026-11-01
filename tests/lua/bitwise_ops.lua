-- Bitwise: and/or/xor/not/shifts on integers, floats and strings; precedence
local maxi, mini = math.maxinteger, math.mininteger
local function hex(v) return string.format("0x%016X", v) end
local function show(label, v) print(label, math.type(v), v, hex(v)) end

show("5 & 3", 5 & 3)
show("5 | 3", 5 | 3)
show("5 ~ 3", 5 ~ 3)
show("~5", ~5)
show("~0", ~0)
show("~-1", ~-1)
show("-1 & 0xff", -1 & 0xff)
show("mini | 1", mini | 1)
show("maxi ~ mini", maxi ~ mini)
show("0xF0F0 & 0x0FF0", 0xF0F0 & 0x0FF0)

-- shifts
show("1 << 0", 1 << 0)
show("1 << 1", 1 << 1)
show("1 << 62", 1 << 62)
show("1 << 63", 1 << 63)
show("1 << 64", 1 << 64)
show("1 << 100", 1 << 100)
show("1 << -1", 1 << -1)
show("2 << -1", 2 << -1)
show("-1 >> 1", -1 >> 1)
show("-1 >> 63", -1 >> 63)
show("-1 >> 64", -1 >> 64)
show("-1 << 64", -1 << 64)
show("mini >> 63", mini >> 63)
show("4 >> -1", 4 >> -1)
show("4 >> -62", 4 >> -62)
show("1 >> -63", 1 >> -63)
show("1 >> -64", 1 >> -64)
show("1 << mini", 1 << mini)
show("1 >> mini", 1 >> mini)
show("-1 << maxi", -1 << maxi)
show("0xff >> 4", 0xff >> 4)
show("-16 >> 2 (logical)", -16 >> 2)

-- floats with exact integer values and numeric strings
show("3.0 & 1", 3.0 & 1)
show("2^62 | 0", 2^62 | 0)
show("-2^63 | 0", -2^63 | 0)
show("1.0 << 3.0", 1.0 << 3.0)
show("~2.0", ~2.0)
-- strings are NOT coerced by bitwise operators (no string metamethods for them)
for _, src in ipairs{'return "3" & 1', 'return "3.0" | 0', 'return ~"0"', 'return 1 << "4"'} do
  print(src, pcall(load(src, "=s")))
end
-- but they can be converted explicitly
show('math.tointeger("0x10") | 0', math.tointeger("0x10") | 0)
show('tonumber("0xffffffffffffffff") ~ 0', tonumber("0xffffffffffffffff") ~ 0)

-- precedence: comparison < | < ~ < & < shift < .. < +-
print("1 | 2 & 3", 1 | 2 & 3)
print("1 | 2 ~ 3", 1 | 2 ~ 3)
print("6 ~ 3 & 1", 6 ~ 3 & 1)
print("1 << 2 + 1", 1 << 2 + 1)
print("1 << 2 | 1", 1 << 2 | 1)
print("5 & 3 == 1", 5 & 3 == 1)
print("~0 + 1", ~0 + 1)
print("- ~0", - ~0)
print("~ -1", ~ -1)
print("~~5", ~~5)
print("1 .. 2 << 1", pcall(load("return 1 .. 2 << 1", "=p")))
print("2 ^ 2 << 1", 2 ^ 2 << 1)
print("3 | 4 < 8", 3 | 4 < 8)
print("0xF0 >> 4 << 4", 0xF0 >> 4 << 4)

-- metamethods
local mt = {}
for _, ev in ipairs{"band", "bor", "bxor", "shl", "shr"} do
  mt["__" .. ev] = function(a, b)
    local function n(x) return type(x) == "table" and "T" or tostring(x) end
    return ev .. "(" .. n(a) .. "," .. n(b) .. ")"
  end
end
mt.__bnot = function(a, b) return "bnot(" .. (rawequal(a, b) and "same" or "diff") .. ")" end
local T = setmetatable({}, mt)
print(T & 1, 1 | T, T ~ T, 2 << T, T >> 3, ~T)
print("1.5 & T", 1.5 & T, '"x" | T', "x" | T)
