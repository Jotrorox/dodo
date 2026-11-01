-- local declarations, attributes and compile-time constants
local a, b, c = 1, 2
local d, e = f()
local g <const> = 10
local h <const> = "str"
local i <const> = 2.5
local j <const> = true
local k <const> = nil
local l <const> = g * 2 + i
local m <const> = {}
local n <close> = nil
local o, p <const>, q = 1, 2, 3
local r <const>, s = 1
local function rec(x) if x > 0 then return rec(x - 1) end return g end
local t = g + h .. tostring(j) .. tostring(k)
do
  local u <close> = setmetatable({}, {__close = function() end})
  local v = u
end
local w = l + m[1]
local x1, x2, x3
local x4; local x5
local x6 = nil; local x7 = nil
print(a, b, c, d, e, g, h, i, j, k, l, m, n, o, p, q, r, s, t, w, rec(3))
