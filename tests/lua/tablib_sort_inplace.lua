-- table.sort permutes the table in place: comparators see the table while it
-- is being sorted, an error leaves a partially sorted permutation, and proxy
-- tables see every read and write through __index / __newindex.

local function show(t, n)
  local out = {}
  for i = 1, n or #t do out[i] = tostring(t[i]) end
  return table.concat(out, " ")
end

-- 1. The comparator observes the table mid-sort.
local t = {5, 3, 8, 1, 9, 2, 7, 4, 6, 0}
local snapshots = {}
table.sort(t, function(a, b)
  if #snapshots < 6 then snapshots[#snapshots + 1] = show(t) end
  return a < b
end)
for i, s in ipairs(snapshots) do print("snap " .. i, s) end
print("sorted", show(t))

-- 2. An error in the comparator leaves the partial permutation.
for _, stop in ipairs({1, 3, 7, 15, 30}) do
  local u = {}
  for i = 1, 20 do u[i] = (i * 7) % 20 end
  local calls = 0
  local ok, err = pcall(table.sort, u, function(a, b)
    calls = calls + 1
    if calls == stop then error("stop at " .. stop, 0) end
    return a < b
  end)
  print(ok, err, show(u))
end

-- 3. Mixed types without comparator: the error depends on which pair is
--    compared first, and the table is left as far as the sort got.
local m = {3, "x", 1, 2, {}, 5, 4}
print(pcall(table.sort, m))
print(show(m):gsub("table: 0x%x+", "table"))

-- 4. A proxy table sees the exact sequence of reads and writes.
local store = {6, 2, 9, 4, 1, 8, 3, 7, 5}
local log = {}
local proxy = setmetatable({}, {
  __index = function(_, k) log[#log + 1] = "r" .. k return store[k] end,
  __newindex = function(_, k, v) log[#log + 1] = "w" .. k .. "=" .. v store[k] = v end,
  __len = function() return #store end,
})
table.sort(proxy)
print(show(store))
print(#log, table.concat(log, " ", 1, 40))

-- 5. Same, with a comparator and a partially filled table: present slots are
--    accessed raw, missing ones through the metamethods.
local half = setmetatable({}, {
  __index = function(_, k) log[#log + 1] = "R" .. k return 100 - k end,
  __newindex = function(t, k, v) log[#log + 1] = "W" .. k rawset(t, k, v) end,
  __len = function() return 8 end,
})
half[2], half[5], half[7] = 50, 10, 99
log = {}
table.sort(half, function(a, b) return a > b end)
print(show(half, 8))
print(table.concat(log, " "))

-- 6. An inconsistent comparator.
local bad = {}
for i = 1, 30 do bad[i] = (i * 13) % 30 end
print(pcall(table.sort, bad, function(a, b) return true end))
print(pcall(table.sort, {1, 2, 3}, function(a, b) return a <= b end))

-- 7. The comparator modifies the table it sorts.
local w = {4, 3, 2, 1, 8, 7, 6, 5}
local k = 0
table.sort(w, function(a, b)
  k = k + 1
  if k == 5 then w[1] = 100 end
  return a < b
end)
print(show(w))

-- 8. Equal floats of both signs and mixed integers/floats keep the order the
--    reference algorithm gives them.
local z = {0.0, -0.0, 1, 1.0, -0.0, 0.0, 2, 0, -0.0, 1.0}
table.sort(z)
local zs = {}
for i, v in ipairs(z) do zs[i] = string.format("%s(%s)", tostring(v), math.type(v)) end
print(table.concat(zs, " "))

-- 9. Large arrays still sort correctly through every path.
local big = {}
for i = 1, 2000 do big[i] = (i * 7919) % 2003 end
table.sort(big)
local okb = true
for i = 2, #big do if big[i - 1] > big[i] then okb = false end end
print("ints", okb)
for i = 1, 2000 do big[i] = tostring((i * 7919) % 2003) end
table.sort(big, function(a, b) return a > b end)
print("strs", big[1], big[2000])
