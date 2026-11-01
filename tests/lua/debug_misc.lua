-- debug.traceback (message handling, levels, non-string messages,
-- coroutine tracebacks; only selected/normalised lines are printed),
-- debug.getmetatable/setmetatable on non-table types (numbers, strings,
-- nil, booleans, functions) and bypassing __metatable, getuservalue /
-- setuservalue on non-userdata, and getregistry.

-- traceback basics
local function level3() return debug.traceback("msg here") end
local function level2() return (level3()) end
local function level1() return (level2()) end
local tb = level1()
local lines = {}
for l in tb:gmatch("[^\n]+") do lines[#lines + 1] = l end
print(lines[1])
print(lines[2])
print(lines[3])
print(lines[4])
print(lines[5])

-- no message: starts with "stack traceback:"
print((debug.traceback():match("^[^\n]*")))
-- level argument skips frames
local function lv() return debug.traceback("L", 2) end
local function calls_lv() return (lv()) end
local t2 = calls_lv()
print((t2:match("^L\nstack traceback:\n\t([^\n]*)")))
-- non-string message is returned untouched; numbers count as strings and
-- nil counts as "no message"
local tbl = {}
print(debug.traceback(tbl) == tbl, debug.traceback(true), debug.traceback(false))
print((debug.traceback(42):match("^[^\n]*\n[^\n]*")))
print((debug.traceback(nil):match("^[^\n]*")))
-- message with a __tostring object is still returned untouched
local o = setmetatable({}, {__tostring = function() return "O" end})
print(debug.traceback(o) == o)
-- empty string message
print(string.format("%q", debug.traceback("", 1):match("^[^\n]*\n[^\n]*")))

-- traceback of another coroutine
local co = coroutine.create(function()
  local function inside() coroutine.yield() end
  inside()
end)
coroutine.resume(co)
local ct = debug.traceback(co, "co msg")
local cl = {}
for l in ct:gmatch("[^\n]+") do cl[#cl + 1] = l end
print(cl[1], cl[2])
print(cl[3])
print(cl[4])
print((debug.traceback(co):match("^[^\n]*")))
print(#cl)

-- debug.getmetatable/setmetatable for non-table values
print(debug.getmetatable(1), debug.getmetatable(nil), debug.getmetatable(print))
print(debug.getmetatable("s").__index == string)
local nmt = {__index = function(n, k) return k .. ":" .. n end,
             __call = function(n, a) return n * a end}
print(debug.setmetatable(10, nmt) == 10)
print((5).foo, (2.5).bar, (7)(6))
print(getmetatable(3) == nmt)
debug.setmetatable(1, nil)
print(pcall(function() return (5).foo end))
-- booleans and nil
debug.setmetatable(true, {__tostring = function(b) return b and "YES" or "NO" end})
print(tostring(true), tostring(false))
debug.setmetatable(false, nil)
print(tostring(true))
debug.setmetatable(nil, {__index = function() return "nil indexed" end})
local nothing = nil
print(nothing.x)
debug.setmetatable(nil, nil)
print(pcall(function() return nothing.x end))
-- functions
debug.setmetatable(print, {__index = {kind = "function"}})
print(print.kind, (function() end).kind)
debug.setmetatable(print, nil)

-- __metatable protects getmetatable/setmetatable but not the debug versions
local prot = setmetatable({}, {__metatable = "locked"})
print(getmetatable(prot), pcall(setmetatable, prot, {}))
print(type(debug.getmetatable(prot)), debug.getmetatable(prot).__metatable)
print(debug.setmetatable(prot, nil) == prot, getmetatable(prot))
print(pcall(debug.setmetatable, {}, 42))

-- user values on non-userdata
print(debug.getuservalue(1))
print(debug.getuservalue({}, 1))
print(pcall(debug.setuservalue, {}, 1))
print(select("#", debug.getuservalue(1)), select("#", debug.getuservalue(io.stdout, 1)))
-- io files are full userdata with no user values
print(debug.getuservalue(io.stdout, 1))

-- registry
print(type(debug.getregistry()))
