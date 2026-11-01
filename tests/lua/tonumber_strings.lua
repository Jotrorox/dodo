-- tonumber without base: decimal/hex numerals, whitespace, invalid strings, non-strings
local function esc(s) return (s:gsub("[^\32-\126]", function(c) return string.format("\\%d", c:byte()) end)) end
local function show(v)
  local r = tonumber(v)
  local label = type(v) == "string" and '"' .. esc(v) .. '"' or
                (type(v) == "number" or type(v) == "boolean") and tostring(v) or type(v)
  if r == nil then print(label, "nil")
  elseif r ~= r then print(label, "nan")
  else print(label, math.type(r), r) end
end

for _, s in ipairs{
  "10", "-10", "+10", "  10  ", "\t\n\v\f\r10\r\n", "010", "0", "-0", "+0", "-0.0",
  "1.5", "-1.5", ".5", "5.", "-.5", "1e2", "1E2", "1e+2", "1e-2", "-1e-2", "1.5e3",
  "0x10", "0X10", "-0x10", "+0x10", "  0x10  ", "0xff", "0xFF", "0x", "0x.", "0xg",
  "0x1p4", "0x1P-2", "0x.8", "0xA.8p1", "0x1.8", "-0x1p1", "0x1p", "0xp1", "0x.p1",
  "9223372036854775807", "9223372036854775808", "-9223372036854775808",
  "-9223372036854775809", "18446744073709551616", "1" .. ("0"):rep(30),
  "0x7fffffffffffffff", "0xffffffffffffffff", "0x10000000000000000", "0x1ffffffffffffffff",
  "-0xffffffffffffffff", "0x8000000000000000", "1e308", "1e309", "-1e309", "1e-400",
  "", " ", "1e", "1e+", "e1", ".", "-", "+", "--1", "- 1", "1 2", "1_000", "1,5",
  "inf", "-inf", "nan", "NaN", "infinity", "0b101", "1f", "1.5.2", "1..2", "abc",
  "10\0", "\0", "0x10\0", "١", "1e1.5", "0x1e", "0x1e+1", "00x1", "0x0x1", "+-1",
} do
  show(s)
end

-- non-string arguments without a base
show(42)
show(4.5)
show(-0.0)
show(math.huge)
show(true)
show({})
show(print)
show(false)
print("nil arg (allowed, returns fail)", pcall(tonumber, nil))
print("no arg", pcall(tonumber))
print("multiple results", select("#", tonumber("1")), select("#", tonumber("x")))

-- tonumber does not use __tostring or other metamethods
print("metatable", tonumber(setmetatable({}, {__tostring = function() return "5" end})))

-- math.tointeger accepts only numbers and numeric strings with integral values
for _, v in ipairs{"8", "8.0", "8.5", "0x10", " 3 ", "x", 8, 8.0, 8.5} do
  local r = math.tointeger(v)
  print("tointeger", type(v), v, r, math.type(r))
end

-- strings converted in arithmetic follow the same rules as tonumber
for _, s in ipairs{"10", "0x10", "1e1", " 7 ", "0x1p1", "9223372036854775808"} do
  local v = s + 0
  print("coerce", s, math.type(v), v, v == tonumber(s))
end
