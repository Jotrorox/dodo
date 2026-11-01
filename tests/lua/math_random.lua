-- math.random and math.randomseed without printing any random values:
-- ranges of random(), random(m), random(m, n), random(0), subtypes,
-- argument errors ("interval is empty", too many arguments), extreme
-- ranges, and the reproducibility of a seeded sequence within one run.

local ok = true
for _ = 1, 2000 do
  local r = math.random()
  if not (math.type(r) == "float" and r >= 0 and r < 1) then ok = false end
end
print("random() in [0,1):", ok)

ok = true
local seen = {}
for _ = 1, 2000 do
  local r = math.random(6)
  if not (math.type(r) == "integer" and r >= 1 and r <= 6) then ok = false end
  seen[r] = true
end
print("random(6) in [1,6]:", ok, "all faces seen:", #seen == 6)

ok = true
for _ = 1, 2000 do
  local r = math.random(-3, 3)
  if not (math.type(r) == "integer" and r >= -3 and r <= 3) then ok = false end
end
print("random(-3,3):", ok)

print("random(5,5):", math.random(5, 5), "random(1):", math.random(1))
print("random(0) is integer:", math.type(math.random(0)))
print("full range:", math.type(math.random(math.mininteger, math.maxinteger)))
ok = true
for _ = 1, 100 do
  local r = math.random(math.maxinteger - 1, math.maxinteger)
  if r ~= math.maxinteger - 1 and r ~= math.maxinteger then ok = false end
end
print("top of range:", ok)
-- float arguments with integral values are accepted
print("random(3.0):", math.type(math.random(3.0)), math.random(2.0, 2.0))

-- errors
print(pcall(math.random, 0, -1))
print(pcall(math.random, 2, 1))
print(pcall(math.random, -1))
print(pcall(math.random, 1, 2, 3))
print(pcall(math.random, 1.5))
print(pcall(math.random, "x"))
print(pcall(math.random, math.maxinteger, math.mininteger))

-- randomseed: same seed gives the same sequence (values not printed)
local function seq(...)
  math.randomseed(...)
  local t = {}
  for i = 1, 10 do t[i] = math.random(1000000) end
  return table.concat(t, ",")
end
print("same seed, same sequence:", seq(42) == seq(42), seq(1, 2) == seq(1, 2))
print("different seeds differ:", seq(42) ~= seq(43))
print("non-integral float seed:", pcall(math.randomseed, 1.5))
print("integral float seed:", (pcall(math.randomseed, 7.0)))
-- randomseed returns the two seed components (5.4.4+); print only types
local a, b = math.randomseed(123)
print("randomseed(123) returns:", math.type(a), math.type(b))
local c, d = math.randomseed()
print("randomseed() returns:", math.type(c), math.type(d))
-- re-seeding with the returned values reproduces the sequence
local s1, s2 = math.randomseed()
local first = {math.random(1 << 40), math.random(1 << 40)}
math.randomseed(s1, s2)
print("reseed with returned values:", math.random(1 << 40) == first[1], math.random(1 << 40) == first[2])
print(pcall(math.randomseed, "x"))
