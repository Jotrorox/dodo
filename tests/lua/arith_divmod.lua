-- Arithmetic: floor division and modulo with negative operands, zero divisors
local function row(a, b)
  local q = (pcall(function() return a // b end)) and a // b or select(2, pcall(function() return a // b end))
  local r = (pcall(function() return a % b end)) and a % b or select(2, pcall(function() return a % b end))
  print(string.format("%s // %s = %s (%s)   %s %% %s = %s (%s)", a, b, q, math.type(q) or "error", a, b, r, math.type(r) or "error"))
end

print("-- integers")
for _, p in ipairs{{7, 3}, {-7, 3}, {7, -3}, {-7, -3}, {0, 3}, {0, -3}, {6, 3}, {-6, 3},
                   {1, 7}, {-1, 7}, {math.maxinteger, 2}, {math.mininteger, 2},
                   {math.mininteger, -1}, {math.mininteger, math.maxinteger},
                   {math.maxinteger, math.mininteger}, {5, 1}, {-5, -1}} do
  row(p[1], p[2])
end

print("-- floats")
for _, p in ipairs{{7.5, 2}, {-7.5, 2}, {7.5, -2}, {-7.5, -2}, {7, 0.5}, {5.25, 0.5},
                   {-0.0, 1}, {0.0, -1}, {3.0, 3}, {-3.0, 3}, {1e308, 1e-308}} do
  row(p[1], p[2])
end

print("-- float with infinities")
local inf = math.huge
for _, p in ipairs{{5, inf}, {-5, inf}, {5, -inf}, {-5, -inf}, {0, inf}, {inf, 2}} do
  local a, b = p[1], p[2]
  local q, r = a // b, a % b
  print(a, b, "//", q, "%", r ~= r and "nan" or r)
end

print("-- zero divisors")
local function try(label, f)
  local ok, v = pcall(f)
  if ok then
    print(label, "ok", math.type(v), v ~= v and "nan" or v)
  else
    print(label, "err", v)
  end
end
local zero, one = 0, 1
try("1//0", function() return one // zero end)
try("1%0", function() return one % zero end)
try("0//0", function() return zero // zero end)
try("0%0", function() return zero % zero end)
try("1.0//0", function() return 1.0 // zero end)
try("1//0.0", function() return one // 0.0 end)
try("-1//0.0", function() return -1 // 0.0 end)
try("0.0//0.0", function() return 0.0 // 0.0 end)
try("1%0.0", function() return one % 0.0 end)
try("1.0%0", function() return 1.0 % zero end)
try("1/0", function() return one / zero end)
try("-1/0", function() return -1 / zero end)
try("0/0", function() return zero / zero end)
try("1/-0.0", function() return one / -0.0 end)
try("minint//-1", function() return math.mininteger // -1 end)
try("minint%-1", function() return math.mininteger % -1 end)
try("constant 1//0", load("return 1 // 0"))
try("constant 1%0", load("return 1 % 0"))
try("math.fmod(7,0)", function() return math.fmod(7, 0) end)
try("math.fmod(7,0.0)", function() return math.fmod(7, 0.0) end)

print("-- math.fmod truncates toward zero")
for _, p in ipairs{{7, 3}, {-7, 3}, {7, -3}, {-7, -3}, {7.5, 2}, {-7.5, 2},
                   {math.mininteger, -1}, {5, math.huge}} do
  local v = math.fmod(p[1], p[2])
  print("fmod", p[1], p[2], math.type(v), v)
end

print("-- identity a == (a//b)*b + a%b")
local ok = true
for a = -9, 9 do
  for b = -4, 4 do
    if b ~= 0 and (a // b) * b + a % b ~= a then ok = false end
    if b ~= 0 and a % b ~= 0 and (a % b < 0) ~= (b < 0) then ok = false end
  end
end
print("identity holds", ok)
