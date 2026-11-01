-- table.concat(list [, sep [, i [, j]]]): separators, ranges, numbers
-- (integer and float formatting), empty ranges, and the error for
-- non-string/non-number elements.

local function try(label, f, ...)
  local ok, res = pcall(f, ...)
  print(label, ok, res)
end

local t = {"a", "b", "c", "d"}
print("default", table.concat(t))
print("sep", table.concat(t, ", "))
print("range", table.concat(t, "-", 2, 3))
print("from i", table.concat(t, "-", 3))
print("single", table.concat(t, "-", 2, 2))
print("empty range", "[" .. table.concat(t, "-", 3, 2) .. "]")
print("empty table", "[" .. table.concat({}) .. "]")
print("empty table sep", "[" .. table.concat({}, ",") .. "]")
print("empty sep", table.concat(t, ""))
print("multichar sep", table.concat({1, 2, 3}, "<=>"))
print("sep with nul", #table.concat({"x", "y"}, "\0"))

-- numbers are converted with the same rules as tostring/..
print("ints", table.concat({1, 2, 3}, " "))
print("floats", table.concat({1.0, 2.5, -0.0, 1e100}, " "))
print("mixed", table.concat({1, "two", 3.0}, "|"))
print("maxint", table.concat({math.maxinteger, math.mininteger}, " "))
print("inf", table.concat({math.huge, -math.huge}, " "))
-- numeric separator is converted too
print("number sep", table.concat({"a", "b"}, 0))

-- indices outside the sequence are accessed raw-ish (via t[i])
local sparse = {[0] = "zero", "one", "two"}
print("from 0", table.concat(sparse, ",", 0, 2))
local neg = {[-1] = "m", [0] = "z", [1] = "p"}
print("negative", table.concat(neg, ",", -1, 1))

-- errors
try("bad element", table.concat, {"a", {}, "c"})
try("bad element 3", table.concat, {1, 2, true})
try("nil inside", table.concat, {"a", "b"}, ",", 1, 3)
try("bad sep", table.concat, {"a"}, {})
try("bad list", table.concat, "abc")
try("nil list", table.concat)
try("bad i", table.concat, {"a"}, ",", "x")
try("float i", table.concat, {"a", "b"}, ",", 1.5)
try("direct", function() return table.concat({1, print}) end)
-- i and j given as integral floats are fine
print("float idx", table.concat({"a", "b", "c"}, ",", 1.0, 2.0))

-- long result
local parts = {}
for i = 1, 1000 do parts[i] = i % 10 end
local s = table.concat(parts)
print("long", #s, s:sub(1, 12), s:sub(-5))
-- range at extreme indices with no elements in between
print("extreme empty", "[" .. table.concat({}, ",", math.maxinteger, math.maxinteger - 1) .. "]")
try("extreme one", table.concat, {}, ",", math.maxinteger, math.maxinteger)
