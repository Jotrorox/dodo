-- The 5.4 table library goes through metamethods: __len determines the
-- length used by insert/remove/concat/unpack/sort, __index supplies
-- elements, __newindex receives stores. Proxies over a backing store
-- work with every table function.

local log = {}
local function proxy(backing, n)
  return setmetatable({}, {
    __len = function() log[#log + 1] = "len"; return n end,
    __index = function(_, k) log[#log + 1] = "get" .. tostring(k); return backing[k] end,
    __newindex = function(_, k, v) log[#log + 1] = "set" .. tostring(k) .. "=" .. tostring(v); backing[k] = v end,
  })
end
local function flush(label)
  print(label, table.concat(log, " "))
  log = {}
end

-- concat via __index and __len
local back = {"a", "b", "c"}
local p = proxy(back, 3)
print("concat", table.concat(p, ","))
flush("  log")

-- unpack via __index (explicit range does not need __len)
print("unpack", table.unpack(p, 1, 2))
flush("  log")
print("unpack len", table.unpack(p))
flush("  log")

-- insert at end: uses __len then __newindex
back = {"a", "b"}
p = proxy(back, 2)
table.insert(p, "c")
flush("insert end")
print("  backing", back[1], back[2], back[3])

-- insert in middle shifts through __index/__newindex
back = {"a", "b"}
p = proxy(back, 2)
table.insert(p, 1, "z")
flush("insert front")
print("  backing", back[1], back[2], back[3])

-- remove
back = {"a", "b", "c"}
p = proxy(back, 3)
print("remove", table.remove(p, 1))
flush("  log")
print("  backing", back[1], back[2], back[3])

-- move between proxies
local b1, b2 = {1, 2, 3}, {}
table.move(proxy(b1, 3), 1, 3, 2, proxy(b2, 0))
flush("move")
print("  dest", b2[1], b2[2], b2[3], b2[4])

-- sort through proxy: only check the final contents
local sb = {5, 3, 4, 1, 2}
table.sort(proxy(sb, 5))
log = {}
print("sort", table.concat(sb, " "))

-- __len returning a float with integer value is accepted; others error
local fl = setmetatable({}, {__len = function() return 2.0 end, __index = function(_, k) return "v" .. k end})
print("float len", table.concat(fl, ","))
local bad = setmetatable({}, {__len = function() return 2.5 end})
print("bad len", pcall(table.concat, bad))
local bad2 = setmetatable({}, {__len = function() return "x" end})
print("bad len str", pcall(table.insert, bad2, 1))
local strlen = setmetatable({}, {__len = function() return "3" end, __index = function(_, k) return k end})
print("string len", pcall(table.concat, strlen, ","))

-- __index chain with a table instead of a function
local base = {"x", "y", "z"}
local derived = setmetatable({}, {__index = base, __len = function() return #base end})
print("index table", table.concat(derived, "+"), table.unpack(derived))

-- raw table without metamethods but with __index only: # is raw (0)
local only_index = setmetatable({}, {__index = base})
print("no __len", #only_index, table.concat(only_index))

-- ipairs honours __index; # uses __len
local count = 0
for i, v in ipairs(derived) do count = count + 1 end
print("ipairs over proxy", count, #derived)

-- table.pack is plain; table.unpack of a pack-result with metatable
local packed = setmetatable(table.pack(1, nil, 3), {__index = function() return "dflt" end})
print("unpack with __index", table.unpack(packed, 1, packed.n))
