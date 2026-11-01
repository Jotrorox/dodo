-- select: '#' counts including trailing nils, positive and negative
-- indices, index beyond the count, index errors, float/string indices; and
-- table.pack/unpack round trips of argument lists with nils.

local function show(...)
  local parts = {}
  for i = 1, select('#', ...) do parts[i] = tostring((select(i, ...))) end
  return select('#', ...) .. ":[" .. table.concat(parts, ",") .. "]"
end
local function try(label, ...)
  local r = table.pack(pcall(select, ...))
  if r[1] then print(label, "ok", show(table.unpack(r, 2, r.n)))
  else print(label, "err", r[2]) end
end

print("count", select('#'), select('#', nil), select('#', nil, nil), select('#', 1, nil, 3, nil))
print("count from fn", select('#', (function() return 1, 2, 3 end)()))
print("count none", select('#', (function() end)()))
print("count paren", select('#', ((function() return 1, 2, 3 end)())))

try("select 1", 1, "a", "b", "c")
try("select 2", 2, "a", "b", "c")
try("select 3", 3, "a", "b", "c")
try("select 4", 4, "a", "b", "c")
try("select 100", 100, "a", "b", "c")
try("select -1", -1, "a", "b", "c")
try("select -2", -2, "a", "b", "c")
try("select -3", -3, "a", "b", "c")
try("select -4", -4, "a", "b", "c")
try("select 0", 0, "a")
try("select 0 empty", 0)
try("select -1 empty", -1)
try("select 1 empty", 1)
try("select 2.0", 2.0, "a", "b")
try("select '2'", "2", "a", "b")
try("select 1.5", 1.5, "a")
try("select 'x'", "x", "a")
try("select nil", nil, "a")
try("select no args")
try("select '#' only", "#")
try("select '#x'", "#x", 1)
try("select minint", math.mininteger, 1)
try("select nils", 2, nil, nil, nil)
try("select -1 nils", -1, nil, nil)

-- direct call shows the call-site name
print("direct", pcall(function() return select(0) end))

-- in-function use
local function last(...) return (select(-1, ...)) end
print("last", last(1, 2, 3), pcall(last))
local function nth(n, ...) return (select(n, ...)) end
print("nth", nth(2, "a", "b", "c"), nth(5, "a"))
local function rest(...) return select(2, ...) end
print("rest", show(rest(1, 2, 3)), show(rest(1)), show(rest(1, nil)))

-- pack/unpack round trips with nils
local function rt(...)
  local t = table.pack(...)
  return table.unpack(t, 1, t.n)
end
print("rt", show(rt()), show(rt(nil)), show(rt(nil, nil)), show(rt(1, nil)), show(rt(nil, 2, nil)))
-- {...} loses trailing nils for #, but select('#') keeps them
local function compare(...)
  local t = {...}
  return select('#', ...), table.pack(...).n
end
print("counts", compare(1, 2, nil))
print("counts all nil", compare(nil, nil, nil))

-- iterate varargs including nils
local function each(...)
  local out = {}
  for i = 1, select('#', ...) do
    local v = select(i, ...)
    out[#out + 1] = type(v)
  end
  return table.concat(out, " ")
end
print("each", each(1, nil, "s", nil, {}, nil))

-- select with many values
local big = {}
for i = 1, 300 do big[i] = i end
print("big count", select('#', table.unpack(big)))
print("big pick", (select(299, table.unpack(big))), (select(-300, table.unpack(big))))
print("big tail", select('#', select(250, table.unpack(big))))
