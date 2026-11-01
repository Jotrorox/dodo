-- Benchmark: string comparison. Equality of interned short strings and of
-- long strings with equal contents, `<`/`<=` ordering of strings sharing
-- prefixes, strings as table keys compared on lookup, and sorting short
-- strings. Stresses EQ/EQK with strings, LT/LE on strings and l_strcmp.

local N = 10000000

local words = {}
local seed = 12345
for i = 1, 200 do
  seed = (seed * 1103515245 + 12345) % 2147483648
  local len = 3 + (seed >> 16) % 10
  local w = {}
  for j = 1, len do
    seed = (seed * 1103515245 + 12345) % 2147483648
    w[j] = string.char(97 + (seed >> 16) % 6)
  end
  words[i] = table.concat(w)
end
local long1 = string.rep("abcdefghij", 8) .. "x"
local long2 = string.rep("abcdefghij", 8) .. "x"
local long3 = string.rep("abcdefghij", 8) .. "y"

local eq, lt, le, longeq = 0, 0, 0, 0
for i = 1, N do
  local a = words[i % 200 + 1]
  local b = words[(i * 7) % 200 + 1]
  if a == b then eq = eq + 1 end
  if a == "abc" then eq = eq + 1 end
  if a < b then lt = lt + 1 end
  if a <= b then le = le + 1 end
  if long1 == long2 then longeq = longeq + 1 end
  if long1 ~= long3 then longeq = longeq + 1 end
  if long1 < long3 then lt = lt + 1 end
end
local sorted = 0
for r = 1, 1500 do
  local copy = table.move(words, 1, #words, 1, {})
  table.sort(copy)
  if copy[1] <= copy[#copy] then sorted = sorted + 1 end
end
print(eq, lt, le, longeq, sorted)
