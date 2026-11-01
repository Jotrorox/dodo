local function p(...) print(...) end
p(1+2, 1-2, 3*4, 7/2, 7//2, -7//2, 7%3, -7%3, 7%-3, -7%-3)
p(7.5//2, -7.5//2, 7.5%2, -7.5%2, 5.25%-2)
p(2^0.5, 2^-1, (-8)^(1/3) ~= (-8)^(1/3))
p(1)
p(1/0, -1/0, 0/0 ~= 0/0)
p(3 & 5, 3 | 5, 3 ~ 5, ~0, 1 << 62, 1 << 63, 1 << 64, -1 >> 1, -1 >> 63, 1 << -1, 2 >> -1)
p(1 << 100, 1 >> -100)
p(9223372036854775807 + 1, -9223372036854775808 - 1)
p(-9223372036854775808 // -1, -9223372036854775808 % -1)
p(5 // 0.0, -5 // 0.0, 0/0 == 0/0)
p(3 == 3.0, 2^53 == 2^53 + 1, 9007199254740993 == 2^53)
p(9007199254740993 < 9007199254740994.0, 9007199254740993 <= 2^53)
p(1 < 1.5, 1.5 < 2, -0.0 == 0, 1e308 * 10)
p(10 // 3.0, 10.0 // 3, 1e15, 1e16, 123456789012345678)
p(0.1 + 0.2, 100.0, -0.0, 1e100, 2^63, -2^63)
p(255 // 1, 3.0 == 3, "10" + 0, "0x10" + 1, "1e2" * 1, " 5 " - 1)
p(pcall(function() return {} + 1 end))
p(pcall(function() local a = "x" return a & 1 end))
p(pcall(function() return 1 & 1.5 end))
p(pcall(function() local t = {} return t < t end))
p(pcall(function() return 1 < "x" end))
p(pcall(function() return {} < 1 end))
p(pcall(function() local x return -x end))
p(pcall(function() local x return #x end))
p(pcall(function() return 1 // 0 end))
p(pcall(function() return 1 % 0 end))
p(1 % (1/0) == nil)
p(5 % 3.0, 5.0 % -3, -5 % 3.5)
for i = 1, 3 do p(i) end
for i = 3, 1, -1 do p(i) end
for i = 1.0, 2.0, 0.5 do p(i) end
for i = 9223372036854775806, 9223372036854775807 do p(i) end
for i = -9223372036854775807, -9223372036854775808, -1 do p(i) end
for i = 1, 0 do p("never") end
p(pcall(function() for i = 1, 10, 0 do end end))
p(pcall(function() for i = "a", 10 do end end))
p(pcall(function() for i = 1, {} do end end))
for i = 1, 3.5 do p(i) end
for i = 1, (1/0) == nil and 2 or 0 do p(i) end
local big = 0
for i = 1, 1000000 do big = big + i end
p(big)
p(2^31, 2^31 | 0, 3.0 | 0)
p(tostring(1e15), tostring(-1e15), 1e14, 123456.789, 0.1, 1/3)
p(8 // 3 * 3 + 8 % 3, -8 // 3 * 3 + -8 % 3)
p(7 // -2, -7 // -2, 7.0 // -2)
