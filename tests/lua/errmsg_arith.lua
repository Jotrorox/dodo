-- Error messages from arithmetic and bitwise operators: nil/table/boolean
-- operands with variable info, strings that are not numerals, integer
-- division and modulo by zero, and values without integer representation
-- in bitwise operations.

local function try(f)
  print(select(2, pcall(f)))
end

try(function() local x; return x + 1 end)
try(function() local x; return 1 + x end)
try(function() return undefined_g * 2 end)
try(function() local t = {}; return t.x - 1 end)
try(function() local t = {}; return -t.neg end)
local up
try(function() return up / 2 end)
try(function() local t = {}; return t + 1 end)
try(function() local b = true; return b % 2 end)
try(function() return {} ^ 2 end)
try(function() local f = print; return f // 1 end)
try(function() local a, b = 1, nil; return a * b end)

-- strings: numerals are coerced, non-numerals fail
print(pcall(function() return "10" + 5, "0x10" * 2, "1e1" - 1, " 3 " / 1 end))
try(function() return "abc" + 1 end)
try(function() local s = "abc"; return s * 2 end)
try(function() local s = "1x"; return -s end)
try(function() local t = {s = "z"}; return t.s + 0 end)
try(function() return "" + 0 end)

-- integer division and modulo by zero
try(function() local a, b = 1, 0; return a // b end)
try(function() local a, b = 1, 0; return a % b end)
try(function() local a = 7; return a // 0 end)
try(function() local a = 7; return a % 0 end)
print(1 // 0.0, -1 // 0.0, 1 % 0.0 ~= 1 % 0.0, 1.0 // 0, 1 / 0)
print(pcall(function() local z = 0; return 5 / z end))
print(math.mininteger // -1, math.mininteger % -1)

-- bitwise operators
try(function() local x = 1.5; return x | 1 end)
try(function() local x = 2^63; return x & 1 end)
try(function() local s = "1.5"; return s << 1 end)
try(function() local s = "abc"; return s ~ 1 end)
try(function() local t = {}; return ~t end)
try(function() local x; return x >> 1 end)
try(function() local b = false; return 1 | b end)
print(pcall(function() return "3" | 0, "0x10" & 0xff, 2.0 << 1, "8" >> 1 end))
try(function() return math.huge | 0 end)
try(function() local n = 0/0; return n & 1 end)

-- comparisons are a separate message family, but mixed number/string fails
try(function() return 1 < "2" end)

-- metamethods take precedence and missing ones produce the message
local mt = {__add = function() return "added" end}
local a = setmetatable({}, mt)
print(a + 1, 1 + a, a + a)
try(function() return a - 1 end)
try(function() return a .. "x" end)

-- arithmetic on results of calls (no variable name)
try(function() local function f() end; return f() + 1 end)
try(function() return #nil end)
try(function() local n = 5; return #n end)
try(function() local x; return #x end)
try(function() local t = {}; return #t.field end)

-- string arithmetic metamethods: errors mention the original operands
try(function() return "10" + {} end)
try(function() return {} .. "x" end)
