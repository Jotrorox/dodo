-- table.pack and table.unpack: the n field, nil holes with explicit ranges,
-- empty and reversed ranges, round trips, negative indices, and the
-- "too many results to unpack" error.

local function show(...)
  local n = select('#', ...)
  local parts = {}
  for i = 1, n do parts[i] = tostring((select(i, ...))) end
  return n .. ":[" .. table.concat(parts, ",") .. "]"
end

local function try(label, f, ...)
  local r = table.pack(pcall(f, ...))
  if r[1] then print(label, "ok", show(table.unpack(r, 2, r.n)))
  else print(label, "err", r[2]) end
end

-- pack
local p = table.pack()
print("pack()", p.n, p[1])
p = table.pack(nil)
print("pack(nil)", p.n, p[1])
p = table.pack(1, nil, 3, nil)
print("pack holes", p.n, p[1], p[2], p[3], p[4])
p = table.pack("a", "b")
print("pack ab", p.n, p[1], p[2])
-- n is a regular field
local keys = {}
for k in pairs(table.pack(10, 20)) do keys[#keys + 1] = tostring(k) end
table.sort(keys)
print("pack keys", table.concat(keys, " "))

-- unpack
print("unpack", show(table.unpack({1, 2, 3})))
print("unpack empty", show(table.unpack({})))
print("unpack i", show(table.unpack({1, 2, 3}, 2)))
print("unpack i j", show(table.unpack({1, 2, 3}, 2, 3)))
print("unpack j beyond", show(table.unpack({1, 2, 3}, 2, 5)))
print("unpack i > j", show(table.unpack({1, 2, 3}, 3, 2)))
print("unpack i = j", show(table.unpack({1, 2, 3}, 2, 2)))
print("unpack negative", show(table.unpack({[-1] = "a", [0] = "b", "c"}, -1, 1)))
print("unpack nil range", show(table.unpack({}, 1, 3)))
print("unpack float idx", show(table.unpack({1, 2, 3}, 1.0, 2.0)))
print("unpack string idx", show(table.unpack({1, 2, 3}, "2", "3")))

-- round trip with nils through pack.n
local function roundtrip(...)
  local t = table.pack(...)
  return table.unpack(t, 1, t.n)
end
print("roundtrip", show(roundtrip(nil, 2, nil)))
print("roundtrip empty", show(roundtrip()))
print("roundtrip trailing nils", show(roundtrip(1, nil, nil)))

-- unpack as the last argument expands, otherwise truncated
print("expand", show(0, table.unpack({1, 2})))
print("truncate", show(table.unpack({1, 2}), 0))
print("paren", show((table.unpack({1, 2}))))

-- unpack is the global table.unpack only (no global unpack in 5.4)
print("global unpack", unpack)

-- large unpack: 250 values
local big = {}
for i = 1, 250 do big[i] = i end
local function count(...) return select('#', ...), (select(250, ...)) end
print("big", count(table.unpack(big)))
local function sum(...)
  local s = 0
  for i = 1, select('#', ...) do s = s + (select(i, ...)) end
  return s
end
print("big sum", sum(table.unpack(big)))

-- errors
try("unpack non-table", table.unpack, 5)
try("unpack nil", table.unpack)
try("too many", table.unpack, {}, 1, 1e8)
try("too many maxint", table.unpack, {}, math.mininteger, math.maxinteger)
try("too many 2", table.unpack, {}, 0, math.maxinteger)
try("bad i", table.unpack, {}, "x")
try("float i", table.unpack, {}, 1.5)
-- extreme but empty or one-element ranges work
try("maxint single", table.unpack, {}, math.maxinteger, math.maxinteger)
try("minint empty", table.unpack, {}, math.mininteger + 1, math.mininteger)
