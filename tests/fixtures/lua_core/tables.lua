local t = {1, 2, 3, nil, 5}
print(#t)
print(#{1, nil, 3}, #{nil, nil, 3}, #{nil}, #{}, #{1, 2, nil})
local a = {}
for i = 1, 100 do a[#a + 1] = i end
print(#a, a[50])
a[100] = nil; print(#a)
a[99] = nil; print(#a)
local h = {}
h[1] = 1; h[2] = 2; h[4] = 4
print(#h == 2 or #h == 4)
local n = 0
local t2 = {x = 1, y = 2, z = 3, 10, 20}
for k, v in pairs(t2) do n = n + 1 end
print(n)
-- delete during traversal
local d = {}
for i = 1, 20 do d["k" .. i] = i end
for k, v in pairs(d) do d[k] = nil end
print(next(d))
print(pcall(next, {}, "nokey"))
print(pcall(function() local t = {} t[nil] = 1 end))
print(pcall(function() local t = {} t[0/0] = 1 end))
print(rawget({}, nil), ({})[0/0])
local f = {}
f[1.0] = "one"; f[2^53] = "big"
print(f[1], f[2^53 | 0])
local ints = {}
for i = 10, 1, -1 do ints[i * 10] = i end
local keys = {}
for k in pairs(ints) do keys[#keys + 1] = k end
print(#keys)
for i = 1, #keys do print(keys[i]) end
local mt = {__index = function(t, k) return k * 2 end, __newindex = function(t, k, v) rawset(t, k, v + 1) end}
local o = setmetatable({}, mt)
print(o[21]); o[1] = 5; print(rawget(o, 1))
local chain = setmetatable({}, {__index = setmetatable({a = 1}, {__index = {b = 2}})})
print(chain.a, chain.b, chain.c)
local loop = setmetatable({}, {})
getmetatable(loop).__index = loop
print(pcall(function() return loop.x end))
print(getmetatable(setmetatable({}, {__metatable = "locked"})))
print(pcall(setmetatable, setmetatable({}, {__metatable = 1}), {}))
local len = setmetatable({}, {__len = function() return 42 end})
print(#len, rawlen(len), rawlen("abc"))
local cnt = setmetatable({}, {__call = function(self, a, b) return a + b end})
print(cnt(3, 4))
local eq1 = setmetatable({}, {__eq = function() return true end})
local eq2 = setmetatable({}, {__eq = function() return true end})
print(eq1 == eq2, eq1 ~= eq2, rawequal(eq1, eq2))
local cc = setmetatable({}, {__concat = function(a, b) return "cat" end})
print(cc .. "x", "x" .. cc, 1 .. cc)
local lt = {__lt = function(a, b) return true end, __le = function(a, b) return false end}
local A, B = setmetatable({}, lt), setmetatable({}, lt)
print(A < B, A <= B, A > B, A >= B)
local arith = setmetatable({}, {__add = function(a, b) return "add" end, __unm = function() return "unm" end, __bnot = function() return "bnot" end, __shl = function() return "shl" end, __idiv = function() return "idiv" end})
print(arith + 1, 1 + arith, -arith, ~arith, arith << 1, 2 // arith)
local tt = {}
for i = 1, 10 do tt[i] = i end
for i = 1, 10 do tt[i] = nil end
print(#tt, next(tt))
local s = {10, 20, 30}

print(select(2, ipairs(s)) == s)
local p = setmetatable({}, {__pairs = function(t) return function(_, k) if not k then return 1, "one" end end, t, nil end})
for k, v in pairs(p) do print(k, v) end
local w = {}
w[3] = 3; w[1] = 1; w[2] = 2
print(#w)
local big = {}
for i = 1, 1000 do big[i] = i end
for i = 1, 1000, 2 do big[i] = nil end
local c = 0
for k, v in pairs(big) do c = c + 1 end
print(c)
