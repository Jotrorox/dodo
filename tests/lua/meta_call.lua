-- __call: callable tables receive themselves plus the arguments, return
-- multiple results, nested callables (__call is itself a callable table),
-- method syntax, use with pcall/table.sort/string.gsub, tail calls, and
-- the errors for non-callable values.

local function show(...)
  local parts = {}
  for i = 1, select('#', ...) do
    local v = select(i, ...)
    parts[i] = type(v) == "table" and "tbl" or tostring(v)
  end
  return select('#', ...) .. ":" .. table.concat(parts, ",")
end

local obj = setmetatable({name = "obj"}, {__call = function(self, ...)
  return self.name, ...
end})
print("basic", show(obj()))
print("args", show(obj(1, nil, 3)))
print("trailing nil", show(obj(nil, nil)))
local me = setmetatable({}, {__call = function(s) return s end})
print("receives self", me() == me)

-- callable stored as a field and called with method syntax
local holder = {f = obj}
print("method syntax", show(holder:f("x")))   -- obj(holder, "x")

-- nested: __call is itself a callable table
local inner = setmetatable({}, {__call = function(self, a, b, c)
  return "inner", type(a), type(b), c
end})
local outer = setmetatable({}, {__call = inner})
print("nested", show(outer("arg")))
-- three levels deep
local lvl3 = setmetatable({}, {__call = outer})
print("three levels", show(lvl3("z")))

-- library functions that type-check for "function" reject callable tables
local cmp = setmetatable({}, {__call = function(_, a, b) return a > b end})
print("sort cmp", pcall(table.sort, {3, 1, 2}, cmp))
-- gsub treats a table replacement as a lookup table, never calling it
local rep = setmetatable({hello = "HI"}, {__call = function(_, w) return w:upper() end})
print("gsub", (string.gsub("hello world", "%w+", rep)))
print("pcall callable", pcall(obj, "p"))
print("xpcall handler", pcall(xpcall, function() error("e", 0) end,
  setmetatable({}, {__call = function(_, m) return "handled " .. m end})))
-- but a callable table is fine as the function xpcall protects
print("xpcall body", xpcall(obj, print, "b"))
-- callable as a generic-for iterator
local countdown = setmetatable({}, {__call = function(_, state, ctl)
  if ctl > 0 then return ctl - 1 end
end})
local seq = {}
for i in countdown, nil, 4 do seq[#seq + 1] = i end
print("for iterator", table.concat(seq, ","))

-- tail call through __call
local depth = 0
local tc
tc = setmetatable({}, {__call = function(self, n)
  if n == 0 then return "done" end
  return self(n - 1)
end})
print("tail calls", tc(10000))

-- __call with varargs counting
local counter = setmetatable({}, {__call = function(_, ...) return select('#', ...) end})
print("count", counter(), counter(nil), counter(1, 2, 3), counter(table.unpack({1, 2, 3, 4})))

-- __call on userdata-free values: changing __call dynamically
local mt = {}
local dyn = setmetatable({}, mt)
print("no __call", pcall(dyn))
mt.__call = function() return "now callable" end
print("added __call", dyn())

-- errors
print("call nil", pcall(function() local x; x() end))
print("call number", pcall(function() local n = 1; n() end))
print("call table", pcall(function() local t = {}; t() end))
print("call field", pcall(function() local t = {}; t.method() end))
print("call method", pcall(function() local t = {}; t:method() end))
print("call global", pcall(function() undefined_fn() end))
print("call string", pcall(function() ("s")() end))
print("bad __call", pcall(setmetatable({}, {__call = 42})))
print("call result", pcall(function() local function f() return nil end; f()() end))
