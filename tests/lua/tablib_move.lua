-- table.move(a1, f, e, t [, a2]): copying within one table (overlapping in
-- both directions), into another table, empty ranges, the returned table,
-- and argument errors including "too many elements to move".

local function dump(t, n)
  local parts = {}
  for i = 1, n do parts[i] = tostring(t[i]) end
  return "{" .. table.concat(parts, ",") .. "}"
end

local function try(label, f, ...)
  local ok, err = pcall(f, ...)
  print(label, ok, ok and "" or err)
end

-- shift right, overlapping (must copy from the end)
local a = {1, 2, 3, 4, 5}
table.move(a, 1, 3, 2)
print("overlap right", dump(a, 5))
-- shift left, overlapping
a = {1, 2, 3, 4, 5}
table.move(a, 2, 5, 1)
print("overlap left", dump(a, 5))
-- shift right far beyond the end
a = {1, 2, 3}
table.move(a, 1, 3, 5)
print("past end", dump(a, 7))
-- same position: no-op
a = {1, 2, 3}
table.move(a, 1, 3, 1)
print("same", dump(a, 3))

-- into another table, returns the destination
local src = {"a", "b", "c", "d"}
local dst = {"x", "y"}
local r = table.move(src, 2, 4, 3, dst)
print("other", r == dst, dump(dst, 5), dump(src, 4))
-- default destination is a1, return value is a1
local self = {1, 2}
print("returns a1", table.move(self, 1, 2, 3) == self, dump(self, 4))
-- move into a fresh table to copy
local copy = table.move({10, 20, 30}, 1, 3, 1, {})
print("copy", dump(copy, 3))

-- empty ranges: e < f does nothing (even with nonsense t)
local e = {1, 2, 3}
table.move(e, 3, 1, 100)
print("empty range", dump(e, 3), e[100])
table.move(e, 1, 0, 1, {})
print("e = f-1", dump(e, 3))

-- negative and zero indices are fine
local z = {[-2] = "m2", [-1] = "m1", [0] = "z"}
local zt = table.move(z, -2, 0, 1, {})
print("negative src", dump(zt, 3))
local nt = table.move({1, 2, 3}, 1, 3, -1, {})
print("negative dst", nt[-1], nt[0], nt[1])

-- holes are copied as nils (overwriting)
local h = {1, nil, 3}
local hd = table.move(h, 1, 3, 1, {"x", "y", "z"})
print("holes", hd[1], hd[2], hd[3])

-- large-ish move
local big = {}
for i = 1, 1000 do big[i] = i end
table.move(big, 1, 1000, 501)
print("big", big[1], big[500], big[501], big[1000], big[1500])

-- errors
try("no args", table.move)
try("missing e", table.move, {}, 1)
try("missing t", table.move, {}, 1, 2)
try("bad a1", table.move, nil, 1, 2, 3)
try("bad a2", table.move, {}, 1, 2, 3, "s")
try("float f", table.move, {}, 1.5, 2, 3)
try("too many", table.move, {}, -1, math.maxinteger, 2)
try("too many 2", table.move, {}, math.mininteger, -1, 1)
try("wrap around", table.move, {}, 1, math.maxinteger, 2)
try("dest wrap", table.move, {1, 2}, 1, 2, math.maxinteger)
-- f = 0, e = maxinteger: e - f reaches maxinteger, which is rejected
try("0..maxint", table.move, {}, 0, math.maxinteger, 1)
-- direct call to see the call-site function name
try("direct", function() table.move({}, 1, 2) end)
