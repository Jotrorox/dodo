-- Benchmark: table library operations. table.insert at the end and in the
-- middle, table.remove from both ends, table.sort with the default
-- comparator and with a Lua comparison function, table.concat, table.move,
-- table.unpack, and the length operator on growing/shrinking arrays.

local N = 1000000

local seed = 7
local function rand()
  seed = (seed * 16807) % 2147483647
  return seed
end

local check = 0

-- append / pop
local t = {}
for i = 1, N do table.insert(t, i) end
for i = 1, N // 2 do check = check + table.remove(t) end
check = check + #t

-- queue-ish: insert near the front of a small array, remove from front
local q = {}
for i = 1, 20000 do
  table.insert(q, 1 + (i % 50 < #q and i % 50 or 0), i)
  if #q > 100 then check = check + table.remove(q, 1) end
end

-- sorting integers (default comparator)
local nums = {}
for i = 1, N do nums[i] = rand() % 1000000 end
table.sort(nums)
for i = 1, N, 1000 do check = (check + nums[i]) % 1000000007 end

-- sorting records with a comparison function
local recs = {}
for i = 1, N // 4 do recs[i] = { key = rand() % 1000, id = i } end
table.sort(recs, function(a, b)
  if a.key ~= b.key then return a.key < b.key end
  return a.id < b.id
end)
for i = 1, #recs, 500 do check = (check + recs[i].key * 7 + recs[i].id) % 1000000007 end

-- sorting strings
local strs = {}
for i = 1, N // 4 do strs[i] = "s" .. (rand() % 100000) end
table.sort(strs, function(a, b) return a > b end)
check = (check + #strs[1] + #strs[#strs]) % 1000000007

-- move / unpack / concat
local a = {}
for i = 1, 1000 do a[i] = i end
for _ = 1, 300 do
  local b = table.move(a, 1, 1000, 1, {})
  table.move(b, 1, 500, 501)
  local x, y, z = table.unpack(b, 498, 500)
  check = (check + x + y + z + select("#", table.unpack(b, 1, 200))) % 1000000007
end
local small = {}
for i = 1, 100 do small[i] = i end
for _ = 1, 2000 do check = (check + #table.concat(small, ",")) % 1000000007 end

print("checksum", check)
