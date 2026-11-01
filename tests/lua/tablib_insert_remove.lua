-- table.insert and table.remove: appending, inserting at positions,
-- argument-count and position errors, removing from the ends, from empty
-- tables, at #t+1, and returned values.

local function dump(t)
  local parts = {}
  for i = 1, #t do parts[i] = tostring(t[i]) end
  return "{" .. table.concat(parts, ",") .. "}"
end

local function try(label, f, ...)
  local r = table.pack(pcall(f, ...))
  if r[1] then
    local parts = {}
    for i = 2, r.n do parts[#parts + 1] = tostring(r[i]) end
    print(label, "ok", r.n - 1, table.concat(parts, " "))
  else
    print(label, "err", r[2])
  end
end

-- insert
local t = {}
table.insert(t, "a")
table.insert(t, "b")
print("append", dump(t))
table.insert(t, 1, "front")
print("front", dump(t))
table.insert(t, 3, "mid")
print("mid", dump(t))
table.insert(t, #t + 1, "end")
print("pos #t+1", dump(t))
print("insert returns", select('#', table.insert(t, "x")))
-- float positions with exact integer value are accepted
table.insert(t, 2.0, "float2")
print("float pos", dump(t))

-- insert errors
try("insert 0 args", table.insert)
try("insert 1 arg", table.insert, {})
try("insert 4 args", table.insert, {}, 1, 2, 3)
try("insert pos 0", table.insert, {1, 2}, 0, "x")
try("insert pos #t+2", table.insert, {1, 2}, 4, "x")
try("insert pos -1", table.insert, {1, 2}, -1, "x")
try("insert pos 1 on empty", table.insert, {}, 1, "x")
try("insert pos 2 on empty", table.insert, {}, 2, "x")
try("insert pos float", table.insert, {1, 2}, 1.5, "x")
try("insert pos string", table.insert, {1, 2}, "1", "x")
try("insert pos bad string", table.insert, {1, 2}, "a", "x")
try("insert non-table", table.insert, nil, 1)
try("insert string", table.insert, "abc", 1)
-- inserting nil appends nothing visible but is legal
local n = {1, 2}
table.insert(n, nil)
print("insert nil", #n)
table.insert(n, 1, nil)
print("insert nil at 1", n[1], n[2], n[3])

-- remove
local r = {"a", "b", "c", "d", "e"}
print("remove last", table.remove(r), dump(r))
print("remove first", table.remove(r, 1), dump(r))
print("remove mid", table.remove(r, 2), dump(r))
print("remove #t", table.remove(r, #r), dump(r))
print("remove only", table.remove(r), dump(r))
-- empty table: returns nil (one value)
try("remove empty", table.remove, {})
try("remove empty pos 0", table.remove, {}, 0)
try("remove empty pos #t+1", table.remove, {}, 1)
try("remove empty pos 2", table.remove, {}, 2)
try("remove empty pos -1", table.remove, {}, -1)
-- remove(t, #t + 1) is allowed and returns t[#t+1] (nil)
local q = {1, 2, 3}
try("remove #t+1", table.remove, q, 4)
print("unchanged", dump(q))
try("remove #t+2", table.remove, q, 5)
try("remove 0", table.remove, q, 0)
try("remove -1", table.remove, q, -1)
try("remove non-table", table.remove, 5)
-- remove on table with element at [0] and #t == 0
local z = {[0] = "zero"}
try("remove pos 0 with [0]", table.remove, z, 0)
print("after", z[0])
-- remove shifts down and clears last slot
local s = {1, 2, 3, 4}
table.remove(s, 2)
print("shifted", s[1], s[2], s[3], s[4], #s)

-- insert/remove as a stack and queue
local stack = {}
for i = 1, 5 do table.insert(stack, i) end
local out = {}
while #stack > 0 do out[#out + 1] = table.remove(stack) end
print("stack", table.concat(out, ""))
local queue = {}
for i = 1, 5 do table.insert(queue, i) end
out = {}
while #queue > 0 do out[#out + 1] = table.remove(queue, 1) end
print("queue", table.concat(out, ""))
local pq = {}
for i = 1, 5 do table.insert(pq, 1, i) end
print("insert front", dump(pq))

-- function name in argument errors depends on how the function was called:
-- through pcall it is the global name, via a call expression the field name
try("direct insert", function() table.insert({1}, 5, "x") end)
try("direct remove", function() table.remove({1}, 7) end)
local tinsert = table.insert
try("local alias", function() tinsert({1}, 5, "x") end)
