-- __tostring (used by tostring and print, must return a string, receives
-- the object), __name (used by tostring when no __tostring and in argument
-- error messages), and their interaction with string.format("%s").

local P = {__tostring = function(self) return "Point(" .. self.x .. "," .. self.y .. ")" end}
local p = setmetatable({x = 1, y = 2}, P)
print(p)
print("tostring", tostring(p))
print("format %s", string.format("%s and %s", p, p))
print("concat needs tostring", tostring(p) .. "!")

-- __tostring returning non-strings
local function try(label, f, ...) print(label, pcall(f, ...)) end
try("returns number", tostring, setmetatable({}, {__tostring = function() return 42 end}))
try("returns nil", tostring, setmetatable({}, {__tostring = function() return nil end}))
try("returns table", tostring, setmetatable({}, {__tostring = function() return {} end}))
try("returns bool", tostring, setmetatable({}, {__tostring = function() return true end}))
try("print non-string", print, setmetatable({}, {__tostring = function() return 1.5 end}))
try("format non-string", string.format, "%s", setmetatable({}, {__tostring = function() return false end}))
-- errors inside __tostring propagate
try("error inside", tostring, setmetatable({}, {__tostring = function() error("ts", 0) end}))
-- multiple results: only the first is used
print("multi", tostring(setmetatable({}, {__tostring = function() return "first", "second" end})))
-- the object itself is passed
local seen
local S = setmetatable({}, {__tostring = function(o) seen = o; return "S" end})
tostring(S)
print("receives self", seen == S)
-- non-callable __tostring
try("non-callable", tostring, setmetatable({}, {__tostring = "str"}))

-- __name: tostring falls back to "NAME: 0x..." (address normalized here)
local named = setmetatable({}, {__name = "MyType"})
local s = tostring(named)
print("__name tostring", (s:gsub("0x%x+", "ADDR"):gsub(": [%x]+$", ": ADDR")))
print("__name prefix", s:match("^MyType: ") ~= nil)
-- __name must be a string to be used
local numname = setmetatable({}, {__name = 123})
print("non-string __name", tostring(numname):match("^table: ") ~= nil)
-- __tostring takes precedence over __name
local both = setmetatable({}, {__name = "Ignored", __tostring = function() return "custom" end})
print("precedence", tostring(both))

-- __name in argument error messages from the standard library
local obj = setmetatable({}, {__name = "Widget"})
try("arg error with __name", string.rep, obj, 2)
try("arg error math", math.floor, obj)
try("arg error method", function() return ("x"):rep(obj) end)
-- plain tables use the type name
try("arg error plain", string.rep, {}, 2)
try("arg error nil", string.upper)

-- tostring of primitive values
print("primitives", tostring(nil), tostring(true), tostring(false), tostring(12), tostring(-0.0),
  tostring(1e15), tostring(1e16), tostring(2^63), tostring(0.1), tostring(1/0), tostring(-1/0))
print("nan", tostring(0/0):match("nan") ~= nil)
print("tostring no arg", pcall(tostring))
print("tostring nil arg", tostring(nil))
-- strings have no __tostring of their own: tostring returns them unchanged
print("string id", tostring("abc") == "abc")
-- __tostring on the string metatable is absent
print("string mt __tostring", getmetatable("").__tostring)
-- print uses __tostring for each argument, separated by tabs
print(p, 1, nil, p)
