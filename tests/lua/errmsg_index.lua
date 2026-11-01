-- Runtime error messages for indexing non-tables, including the variable
-- description the compiler attaches: local, global, field, upvalue,
-- method, constant string keys, and indexing results of calls (no name).

local function try(f, ...)
  local ok, msg = pcall(f, ...)
  print(ok, msg)
end

try(function() local x; return x.y end)
try(function() local x; x.y = 1 end)
try(function() return undefinedglobal.field end)
try(function() undefinedglobal.field = 1 end)
try(function() local t = {}; return t.a.b end)
try(function() local t = {a = {}}; return t.a.b.c end)
try(function() local t = {}; t.a.b = 1 end)
local up
try(function() return up.x end)
try(function() up.x = 1 end)
try(function() local t = {}; return t[1].x end)
try(function() local t = {}; local k = "key"; return t[k].x end)
try(function() local s = "str"; return s.x.y end)
try(function() return (nil).x end)
try(function() local function f() end; return f().x end)
try(function() local function f() return 1 end; return f().x end)

-- other value types
try(function() local n = 5; return n.x end)
try(function() local n = 5.5; return n[1] end)
try(function() local b = true; return b.x end)
try(function() local f = print; return f.x end)
try(function() local co = coroutine.create(print); return co.x end)
try(function() local n = 1; n.x = 2 end)
try(function() local b = false; b[1] = 2 end)

-- strings index the string library; missing methods are nil -> call error
try(function() local s = "abc"; return s.nonexistent end)
try(function() local s = "abc"; return s:nonexistent() end)
try(function() return ("x"):rep(2) .. ("y").len end)

-- method call on nil / missing method
try(function() local obj = {}; obj:missing() end)
try(function() local obj; obj:m() end)
try(function() obj2:m() end)
try(function() local t = {inner = {}}; t.inner:go() end)

-- upvalue table with nil field
local cfg = {}
try(function() return cfg.section.key end)

-- index via __index chain that ends in a non-table
local proxy = setmetatable({}, {__index = 42})
try(function() return proxy.x end)
local proxy2 = setmetatable({}, {__newindex = "str"})
try(function() proxy2.x = 1 end)

-- global names with _ENV replaced
try(function() local _ENV = {}; return missing.x end)
try(function() local _ENV = {print = print}; x = nil; return x.y end)

-- integer, float and boolean keys
try(function() local t = {}; return t[2][3] end)
try(function() local t = {}; return t[2.5].z end)
try(function() local t = {}; return t[true].z end)
try(function() local a = {{}}; return a[1][2][3] end)

-- the key value in the message for a constant index on a nil upvalue
local nothing
try(function() return nothing[10] end)
try(function() return nothing["long key with spaces"] end)
