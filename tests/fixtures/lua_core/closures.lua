local function counter()
  local n = 0
  return function() n = n + 1; return n end
end
local c1, c2 = counter(), counter()
print(c1(), c1(), c2(), c1())
local fs = {}
for i = 1, 5 do fs[i] = function() return i end end
print(fs[1](), fs[3](), fs[5]())
local function va(...)
  local a, b = ...
  return select('#', ...), a, b, ...
end
print(va())
print(va(1))
print(va(1, nil, 3))
print((va(1, 2, 3)))
local function pack(...) return {n = select('#', ...), ...} end
local t = pack(1, nil, nil)
print(t.n, #t >= 0)
local function tail(n) if n == 0 then return "done" end return tail(n - 1) end
print(tail(1000000))
local function deep(n) if n == 0 then return 0 end return 1 + deep(n - 1) end
print(deep(10000))
print(select(-1, 1, 2, 3), select(-3, 1, 2, 3))
print(pcall(select, -4, 1, 2, 3))
print(pcall(select, 0))
local function mr() return 1, 2, 3 end
local x, y, z, w = mr()
print(x, y, z, w)
print(({mr(), mr()})[4], #{mr(), mr()})
print(mr(), mr())
print((mr()))
local s = 0
local function sum(...) for i = 1, select('#', ...) do s = s + select(i, ...) end end
sum(1, 2, 3, 4)
print(s)
-- upvalues shared across closures
local function mk()
  local v = 0
  local function get() return v end
  local function set(x) v = x end
  return get, set
end
local g, st = mk()
st(42); print(g())
-- loops create fresh locals
local acc = {}
local i = 1
while i <= 3 do local j = i; acc[i] = function() return j end; i = i + 1 end
print(acc[1](), acc[2](), acc[3]())
-- goto
do
  local k = 0
  ::top::
  k = k + 1
  if k < 5 then goto top end
  print("k", k)
end
for i = 1, 3 do
  for j = 1, 3 do
    if j == 2 then goto continue end
    print(i, j)
    ::continue::
  end
end
-- recursion via local function
local function fact(n) return n <= 1 and 1 or n * fact(n - 1) end
print(fact(20), fact(21))
-- varargs in table constructor & calls
local function cnt(...) return select('#', ...) end
print(cnt(nil, nil), cnt(table == 1), cnt(mr()), cnt(mr(), 1))
-- method calls
local obj = {v = 10}
function obj:add(x) self.v = self.v + x; return self end
print(obj:add(5):add(5).v)
-- long argument lists
local function many(...) return select('#', ...) end
print(many(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30))
local big = {}
for q = 1, 250 do big[q] = q end
local unpackish
unpackish = function(t, i) i = i or 1; if i <= #t then return t[i], unpackish(t, i + 1) end end
print(select('#', unpackish(big)))
