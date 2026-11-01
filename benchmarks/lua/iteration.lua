-- Benchmark: generic for loops. ipairs over arrays, pairs over arrays and
-- over hash tables with string keys, next called directly, and a closure
-- based iterator. Stresses TFORCALL/TFORLOOP, ipairs/pairs/next and
-- table traversal.

local N = 3200

local arr = {}
for i = 1, 5000 do arr[i] = i % 13 end
local hash = {}
for i = 1, 1000 do hash["k" .. i] = i end

local function range(n)
  local i = 0
  return function()
    i = i + 1
    if i <= n then return i end
  end
end

local a, b, c, d, e = 0, 0, 0, 0, 0
for r = 1, N do
  for _, v in ipairs(arr) do a = a + v end
  for k, v in pairs(arr) do b = b + k - v end
  for _, v in pairs(hash) do c = c + v end
  local k, v = next(hash)
  while k do d = d + v; k, v = next(hash, k) end
  for i in range(1000) do e = e + i end
end
print(a, b, c, d, e)
