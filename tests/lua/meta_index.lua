-- __index: table and function forms, multi-level chains, rawget bypass,
-- arguments passed to the handler, only consulted for absent keys, results
-- truncated to one value, __index on non-table values via debug-free
-- means (strings), and errors raised by handlers.

local base = {greet = "hello", shared = "base"}
local mid = setmetatable({shared = "mid", midonly = "m"}, {__index = base})
local top = setmetatable({own = "top"}, {__index = mid})
print("chain", top.own, top.midonly, top.shared, top.greet, top.missing)
print("rawget", rawget(top, "own"), rawget(top, "greet"), rawget(top, "shared"))

-- deep chain of 10 levels
local deep = {bottom = "found"}
for i = 1, 10 do deep = setmetatable({["lvl" .. i] = i}, {__index = deep}) end
print("deep", deep.bottom, deep.lvl1, deep.lvl10, deep.nothing)

-- function form receives the table and the key
local calls = {}
local fobj = setmetatable({present = "p"}, {__index = function(t, k)
  calls[#calls + 1] = tostring(k)
  return "computed_" .. tostring(k), "extra"
end})
local tself = setmetatable({}, {__index = function(t, k) return t end})
print("function", fobj.present, fobj.abc, fobj[1], fobj[true])
print("handler self", tself.x == tself)
print("only for absent", table.concat(calls, ","))
-- extra return values are discarded
print("one result", select('#', fobj.zzz))
-- field present but false/nil
local fz = setmetatable({f = false}, {__index = function() return "default" end})
print("false is present", fz.f, fz.n)

-- mixing: function at the end of a table chain
local fend = setmetatable({}, {__index = setmetatable({a = 1}, {__index = function(_, k) return k .. "!" end})})
print("mixed chain", fend.a, fend.b)
-- a handler further down a chain receives the table at its own level,
-- not the table originally indexed
local lvl2 = setmetatable({}, {__index = function(t, k) return t end})
local lvl1 = setmetatable({}, {__index = lvl2})
print("receiver", lvl1.x == lvl2, lvl1.x == lvl1)

-- __index changes take effect immediately
local mt = {}
local dyn = setmetatable({}, mt)
print("before", dyn.k)
mt.__index = {k = "now"}
print("after", dyn.k)
mt.__index = nil
print("removed", dyn.k)

-- metatable whose __index is the metatable itself (class idiom)
local Class = {}
Class.__index = Class
function Class.new(x) return setmetatable({x = x}, Class) end
function Class:get() return self.x end
function Class:double() return self.x * 2 end
local obj = Class.new(21)
print("class", obj:get(), obj:double(), obj.new == Class.new)

-- inheritance
local Sub = setmetatable({}, {__index = Class})
Sub.__index = Sub
function Sub.new(x) return setmetatable(Class.new(x), Sub) end
function Sub:double() return "sub" .. Class.double(self) end
local so = Sub.new(5)
print("inherit", so:get(), so:double())

-- strings use the string table as __index
local s = "Hello"
print("string methods", s:upper(), s:lower(), s:len(), ("x"):rep(3), s:sub(2, 3))
print("string index field", s.upper == string.upper, ("abc").len == string.len)
print("number literal method", pcall(function() local n = 5; return n:foo() end))
-- getmetatable(string).__index is string
print("string mt", getmetatable("").__index == string)

-- errors from handlers propagate
local err = setmetatable({}, {__index = function(_, k) error("no field " .. k) end})
print("handler error", pcall(function() return err.zz end))
-- __index of a non-function non-table value: indexing it
local weird = setmetatable({}, {__index = 5})
print("number __index", pcall(function() return weird.x end))
local weird2 = setmetatable({}, {__index = "str"})
print("string __index", weird2.len == string.len, weird2.nosuch)
-- loop detection in __index chains
local l1, l2 = {}, {}
setmetatable(l1, {__index = l2})
setmetatable(l2, {__index = l1})
print("loop", pcall(function() return l1.x end))
-- rawget errors
print("rawget non-table", pcall(rawget, "s", 1))
print("rawget no key", pcall(rawget, {}))
