-- table.sort: default ordering for numbers and strings, custom comparators
-- (descending, by field, multi-key), mixed int/float, comparison errors
-- for mixed types, and the "invalid order function" error (normalized,
-- since detection depends on the algorithm). No ties are observable.

local function dump(t)
  local parts = {}
  for i = 1, #t do parts[i] = tostring(t[i]) end
  return table.concat(parts, " ")
end

local t = {5, 2, 8, 1, 9, 3, 7, 4, 6, 10}
table.sort(t)
print("asc", dump(t))
table.sort(t, function(a, b) return a > b end)
print("desc", dump(t))
print("sort returns", select('#', table.sort({3, 1, 2})))

-- already sorted / reverse sorted / tiny inputs
local s = {1, 2, 3, 4, 5}
table.sort(s); print("sorted", dump(s))
s = {5, 4, 3, 2, 1}
table.sort(s); print("reverse", dump(s))
s = {}; table.sort(s); print("empty", #s)
s = {42}; table.sort(s); print("one", dump(s))
s = {2, 1}; table.sort(s); print("two", dump(s))

-- duplicates: equal numbers are indistinguishable, so ties are invisible
s = {3, 1, 3, 2, 1, 3, 2}
table.sort(s); print("dups", dump(s))

-- strings (byte-wise in the C locale)
local w = {"pear", "Apple", "banana", "apple", "Banana", "cherry", "", "a", "ab"}
table.sort(w)
print("strings", table.concat(w, ","))
table.sort(w, function(a, b) return a:lower() < b:lower() or (a:lower() == b:lower() and a < b) end)
print("case-insensitive", table.concat(w, ","))

-- mixed integers and floats compare numerically
local m = {3, 1.5, -2, 2.25, 0, -0.5, math.huge, -math.huge, math.maxinteger, math.mininteger}
table.sort(m)
print("mixed numbers", dump(m))

-- sort records by field, then by second key
local people = {
  {name = "dora", age = 30}, {name = "al", age = 25}, {name = "cy", age = 30},
  {name = "bo", age = 25}, {name = "ed", age = 40},
}
table.sort(people, function(a, b)
  if a.age ~= b.age then return a.age < b.age end
  return a.name < b.name
end)
local names = {}
for i, p in ipairs(people) do names[i] = p.name .. p.age end
print("records", table.concat(names, " "))

-- larger input with a deterministic pseudo-random permutation
local big = {}
local x = 12345
for i = 1, 500 do
  x = (x * 1103515245 + 12345) % 2147483648
  big[i] = x
end
table.sort(big)
local ok = true
for i = 2, #big do if big[i - 1] > big[i] then ok = false end end
print("big sorted", ok, #big)
table.sort(big, function(a, b) return a > b end)
ok = true
for i = 2, #big do if big[i - 1] < big[i] then ok = false end end
print("big desc", ok)

-- comparator receives elements of the table; count calls are positive
local calls = 0
table.sort({3, 2, 1}, function(a, b) calls = calls + 1; return a < b end)
print("comparator called", calls > 0)

-- errors
-- which operand ends up on the left depends on the sort algorithm, so the
-- "attempt to compare X with Y" message is normalized to a sorted pair
local function try(label, f)
  local ok2, err = pcall(f)
  if not ok2 then
    local a, b = string.match(err, "attempt to compare (%a+) with (%a+)$")
    if a then
      if b < a then a, b = b, a end
      err = "attempt to compare {" .. a .. ", " .. b .. "}"
    end
  end
  print(label, ok2, err)
end
try("mixed types", function() table.sort({1, "x"}) end)
try("mixed types 2", function() table.sort({"x", 1, "y", 2}) end)
try("tables", function() table.sort({{}, {}}) end)
try("bools", function() table.sort({true, false}) end)
try("nil in range", function()
  local h = setmetatable({1, nil, 3}, {__len = function() return 3 end})
  table.sort(h)
end)
try("bad comparator type", function() table.sort({1, 2}, 3) end)
try("bad table", function() table.sort("abc") end)
try("comparator error", function() table.sort({1, 2, 3}, function() error("boom") end) end)

-- an inconsistent comparator either works or raises "invalid order function"
local function check_invalid(label, cmp)
  local arr = {}
  for i = 1, 100 do arr[i] = (i * 37) % 101 end
  local ok3, err = pcall(table.sort, arr, cmp)
  print(label, ok3 or (string.find(err, "invalid order function", 1, true) ~= nil))
end
check_invalid("always true", function() return true end)
check_invalid("<=", function(a, b) return a <= b end)
