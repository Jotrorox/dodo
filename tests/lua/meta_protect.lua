-- getmetatable/setmetatable: __metatable protection (getmetatable returns
-- the field, setmetatable refuses), setmetatable argument checking and
-- return value, removing a metatable with nil, metatables of non-table
-- values (strings share one; numbers etc. have none from Lua).

local function try(label, f, ...) print(label, pcall(f, ...)) end

local t = {}
local mt = {}
print("returns table", setmetatable(t, mt) == t)
print("getmetatable", getmetatable(t) == mt)
print("remove", setmetatable(t, nil) == t, getmetatable(t))
print("no mt", getmetatable({}))

-- __metatable protection
local prot = setmetatable({}, {__metatable = "locked"})
print("protected get", getmetatable(prot))
try("protected set", setmetatable, prot, {})
try("protected remove", setmetatable, prot, nil)
-- __metatable can be any value, including false
local pf = setmetatable({}, {__metatable = false})
print("false field", getmetatable(pf))
try("false field set", setmetatable, pf, {})
local ptab = {}
local pt = setmetatable({}, {__metatable = ptab})
print("table field", getmetatable(pt) == ptab)
-- metamethods still work while protected
local pm = setmetatable({}, {__metatable = "nope", __index = function() return "via index" end})
print("still works", pm.anything)
-- rawequal/rawget unaffected
print("raw", rawget(pm, "anything"))
-- direct call (shows the called name in the message)
try("direct", function() setmetatable(prot, {}) end)

-- setmetatable argument errors
try("no args", setmetatable)
try("one arg", setmetatable, {})
try("non-table target", setmetatable, 1, {})
try("string target", setmetatable, "s", {})
try("bad mt", setmetatable, {}, 5)
try("bad mt string", setmetatable, {}, "x")
try("bad mt bool", setmetatable, {}, true)
try("nil target", setmetatable, nil, {})
try("direct bad mt", function() setmetatable({}, 1) end)

-- getmetatable on various types
print("string mt", type(getmetatable("")), getmetatable("a") == getmetatable("b"))
print("string mt index", getmetatable("").__index == string)
print("number mt", getmetatable(1), getmetatable(1.5))
print("bool mt", getmetatable(true), getmetatable(nil))
print("function mt", getmetatable(print), getmetatable(function() end))
try("getmetatable no arg", getmetatable)

-- one metatable shared by many tables
local shared = {__index = {kind = "shared"}}
local a, b = setmetatable({}, shared), setmetatable({}, shared)
print("shared", a.kind, b.kind, getmetatable(a) == getmetatable(b))
shared.__index = {kind = "changed"}
print("shared change", a.kind, b.kind)
-- a table can be its own metatable
local self = {}
self.__index = function(_, k) return "self:" .. k end
setmetatable(self, self)
print("own metatable", self.x, getmetatable(self) == self)
-- metatable fields are looked up raw: __index in the metatable's own
-- metatable does not count
local base_mt = setmetatable({}, {__index = {__index = function() return "inherited" end}})
local inh = setmetatable({}, base_mt)
print("raw metafield lookup", inh.x)
-- __metatable inherited the same way is ignored
local base_mt2 = setmetatable({}, {__index = {__metatable = "hidden"}})
local inh2 = setmetatable({}, base_mt2)
print("raw __metatable", getmetatable(inh2) == base_mt2)
