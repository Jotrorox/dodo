-- __newindex: only for absent keys, table form (stores into the other
-- table, recursively honoring its metamethods), function form, rawset
-- bypass, read-only tables, tracking proxies, and interaction with
-- existing keys and keys set to nil.

local log = {}
local t = setmetatable({existing = 1}, {__newindex = function(tbl, k, v)
  log[#log + 1] = tostring(k) .. "=" .. tostring(v)
  rawset(tbl, k, v)
end})
t.existing = 2          -- present: no metamethod
t.fresh = "a"           -- absent: metamethod
t.fresh = "b"           -- now present: no metamethod
t[1] = "one"
print("log", table.concat(log, " "))
print("values", t.existing, t.fresh, t[1])
-- assigning nil to an absent key still triggers __newindex
log = {}
t.never = nil
print("nil assign", table.concat(log, " "))
-- after setting a present key to nil it becomes absent again
t.fresh = nil
log = {}
t.fresh = "again"
print("re-absent", table.concat(log, " "))

-- table form: stores go to the target table
local store = {}
local front = setmetatable({}, {__newindex = store})
front.x = 10
print("table form", rawget(front, "x"), store.x)
-- reading through front does not see store unless __index set
print("no index", front.x)
-- chained table form honours the target's metamethods
local final = {}
local inner = setmetatable({}, {__newindex = final})
local outer = setmetatable({}, {__newindex = inner})
outer.k = "v"
print("chain", rawget(outer, "k"), rawget(inner, "k"), final.k)
-- chain stops at a table that already has the key
local hasit = {k = "old"}
local via = setmetatable({}, {__newindex = hasit})
via.k = "new"
print("stops", hasit.k)

-- rawset bypasses the metamethod and returns the table
log = {}
local r = rawset(t, "raw", 1)
print("rawset", r == t, #log, rawget(t, "raw"))
print("rawset errors", pcall(rawset, {}, nil, 1))
print("rawset non-table", pcall(rawset, 1, 2, 3))
print("rawset missing value", pcall(rawset, {}, 1))

-- read-only table idiom
local function readonly(src)
  return setmetatable({}, {
    __index = src,
    __newindex = function(_, k) error("attempt to modify read-only field '" .. tostring(k) .. "'", 2) end,
    __len = function() return #src end,
  })
end
local ro = readonly({1, 2, 3, name = "ro"})
print("ro read", ro[1], ro.name, #ro)
print("ro write", pcall(function() ro.name = "x" end))
print("ro write new", pcall(function() ro[4] = 4 end))

-- default values and change tracking proxy
local function tracked(inner_t)
  local changes = {}
  local p = setmetatable({}, {
    __index = inner_t,
    __newindex = function(_, k, v) changes[#changes + 1] = k; inner_t[k] = v end,
  })
  return p, changes
end
local p, changes = tracked({a = 1})
p.a = 2; p.b = 3; p.a = 4
print("tracked", table.concat(changes, ","), p.a, p.b)

-- __newindex with a non-table, non-function value
local bad = setmetatable({}, {__newindex = 5})
print("number __newindex", pcall(function() bad.x = 1 end))
-- errors inside the handler propagate
local boom = setmetatable({}, {__newindex = function() error("denied", 0) end})
print("handler error", pcall(function() boom.x = 1 end))
-- __newindex loop detection
local a1, a2 = {}, {}
setmetatable(a1, {__newindex = a2})
setmetatable(a2, {__newindex = a1})
print("loop", pcall(function() a1.x = 1 end))

-- multiple assignment through __newindex, all values computed first
local order = {}
local m = setmetatable({}, {__newindex = function(_, k, v) order[#order + 1] = k .. v end})
m.a, m.b = "1", "2"
table.sort(order)
print("multi", table.concat(order, " "))
-- table.insert uses __newindex in 5.4
local ins = {}
local ip = setmetatable({}, {__newindex = function(_, k, v) ins[#ins + 1] = k .. ":" .. v end})
table.insert(ip, "z")
print("insert", table.concat(ins))
