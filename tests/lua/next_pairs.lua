-- next and pairs: traversal visits every key exactly once (checked by
-- sorting), next(t) on empty tables, next with the last key, __pairs
-- overriding, clearing fields during traversal, invalid keys to next,
-- and argument errors.

local function sorted_keys(t)
  local ks = {}
  for k in pairs(t) do ks[#ks + 1] = tostring(k) end
  table.sort(ks)
  return table.concat(ks, " ")
end

print("next empty", next({}))
print("next empty nil", next({}, nil))
print("select# next empty", select('#', next({})))
local one = {x = 1}
print("next one", next(one))
print("next after last", next(one, "x"))
local arr = {10}
print("next arr", next(arr))
print("next arr after", next(arr, 1))

-- every key once, mixed array and hash part
local t = {1, 2, 3, a = "A", b = "B", [1.5] = "f", [true] = "t"}
print("keys", sorted_keys(t))
local n = 0
for k, v in pairs(t) do
  n = n + 1
  assert(t[k] == v)
end
print("count", n)

-- manual traversal with next
local count, k = 0, nil
repeat
  k = next(t, k)
  if k ~= nil then count = count + 1 end
until k == nil
print("manual next", count)

-- pairs returns next, t, nil
local f, s, init = pairs(t)
print("pairs triple", f == next, s == t, init)

-- clearing fields during traversal is allowed
local c = {}
for i = 1, 20 do c[i] = i; c["k" .. i] = i end
for key in pairs(c) do c[key] = nil end
print("cleared", next(c))
-- assigning existing fields during traversal is allowed
local d = {a = 1, b = 2, c = 3, 4, 5}
for key, v in pairs(d) do d[key] = v * 10 end
print("doubled", d.a, d.b, d.c, d[1], d[2])

-- __pairs metamethod
local mt = {__pairs = function(self)
  local i = 0
  return function(_, _)
    i = i + 1
    if i <= 3 then return i, "item" .. i end
  end, self, nil
end}
local virt = setmetatable({real = true}, mt)
for k2, v in pairs(virt) do print("__pairs", k2, v) end
-- __pairs returning more than 3 values: extras dropped by pairs
local mt2 = {__pairs = function(t2) return next, t2, nil, "extra" end}
print("__pairs results", select('#', pairs(setmetatable({}, mt2))))
-- next ignores __pairs
print("next ignores __pairs", next(virt))
-- __pairs receives the table itself
local seen
setmetatable(virt, {__pairs = function(x) seen = x; return next, {}, nil end})
for _ in pairs(virt) do end
print("__pairs arg", seen == virt)
-- __pairs that is not callable
local badp = setmetatable({}, {__pairs = 42})
print("bad __pairs", pcall(pairs, badp))
-- pairs on a table with __index: __index is not consulted
local withidx = setmetatable({own = 1}, {__index = {inherited = 2}})
print("pairs ignores __index", sorted_keys(withidx))

-- errors
-- pairs only checks that an argument is present; the error for a
-- non-table appears when the returned next is called
local okp, pf, ps = pcall(pairs, nil)
print("pairs nil", okp, pf == next, ps)
okp, pf, ps = pcall(pairs, 1)
print("pairs number", okp, pf == next, ps)
print("for pairs nil", pcall(function() for _ in pairs(nil) do end end))
print("pairs none", pcall(pairs))
print("next nil", pcall(next, nil))
print("next string", pcall(next, "abc"))
print("next bad key", pcall(next, {a = 1}, "zzz"))
print("next bad int key", pcall(next, {1, 2}, 99999))
print("for over nil", pcall(function() for _ in nil do end end))
print("for over table", pcall(function() for _ in {} do end end))

-- integer and float keys: next returns normalized integer keys
local nk = {}
nk[3.0] = "x"
local key = next(nk)
print("normalized key", key, math.type(key))

-- nested traversal of the same table is independent
local nest = {a = 1, b = 2}
local pairsCount = 0
for _ in pairs(nest) do for _ in pairs(nest) do pairsCount = pairsCount + 1 end end
print("nested", pairsCount)
