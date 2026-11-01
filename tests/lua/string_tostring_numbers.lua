-- Strings: conversion of integers and floats to strings (tostring, concat, %.14g)
local values = {
  {"0", 0}, {"-1", -1}, {"maxint", math.maxinteger}, {"minint", math.mininteger},
  {"0.0", 0.0}, {"-0.0", -0.0}, {"1.0", 1.0}, {"100.0", 100.0}, {"-5.0", -5.0},
  {"0.1", 0.1}, {"1/3", 1 / 3}, {"2/3", 2 / 3}, {"-1/3", -1 / 3}, {"0.5", 0.5},
  {"1e14", 1e14}, {"1e15", 1e15}, {"1e16", 1e16}, {"123456789012.0", 123456789012.0},
  {"12345678901234.0", 12345678901234.0}, {"123456789012345.0", 123456789012345.0},
  {"2^53", 2^53}, {"2^63", 2^63}, {"-2^63", -2^63}, {"1e100", 1e100}, {"1e-5", 1e-5},
  {"1e-4", 1e-4}, {"0.0001234", 0.0001234}, {"1e300*1e10", 1e300 * 1e10},
  {"huge", math.huge}, {"-huge", -math.huge}, {"pi", math.pi}, {"-pi", -math.pi},
  {"min subnormal", 2^-1074}, {"min normal", 2^-1022}, {"max float", 1.7976931348623157e308},
  {"3.14159265358979", 3.14159265358979}, {"99999999999999.9", 99999999999999.9},
  {"999999999999999.0", 999999999999999.0}, {"0.1+0.2", 0.1 + 0.2}, {"1e15+0.5", 1e15 + 0.5},
  {"2^24", 2.0^24}, {"-1e-300", -1e-300}, {"5e-324*0.5", 5e-324 * 0.5},
}
for _, p in ipairs(values) do
  local v = p[2]
  print(string.format("%-20s %-8s %-24s %-24s %s", p[1], math.type(v), tostring(v), v .. "",
    string.format("%.14g", v)))
end

-- integer-valued floats get ".0" only when %.14g output looks like an integer
print(1e15, 1e14, 1e15 / 10, 2^49, 2^50 + 0.0)
-- integers never get ".0"
print(1 << 62, 10 // 3, 7 % 3)
-- tostring and string.format("%s") agree
print(string.format("%s %s %s %s", 1, 1.0, -0.0, 1e100))
-- concatenation coerces both integers and floats
print(1 .. "", 1.0 .. "", -0.0 .. "|", 2^63 .. "", math.huge .. "", 0x10 .. "")
-- %d with float arguments that have exact integer values
print(string.format("%d %d", 3.0, -2^63))
-- converting back from the string form
for _, v in ipairs{0.1, 1 / 3, 1e15, 2^63, 123.456} do
  local s = tostring(v)
  print(s, tonumber(s) == v, math.type(tonumber(s)))
end
