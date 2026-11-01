-- Miscellaneous error behaviour: assert (default message, custom string
-- and non-string messages, returning all its arguments), stack overflow
-- caught by pcall, float->integer conversion failures ("number has no
-- integer representation"), string->number failures in for loops and
-- library calls, and select('#') of pcall results on error.

-- assert
print(pcall(assert, false))
print(pcall(assert, nil))
print(pcall(assert, false, "custom message"))
print(pcall(assert, nil, 42))
local t = {}
local ok, e = pcall(assert, false, t)
print(ok, e == t)
print(pcall(assert, false, nil))
print(select("#", pcall(assert, false, nil)))
print(assert(1, 2, 3))
print(assert("v", "unused message"))
print(select("#", assert(true, nil, nil)))
print(pcall(assert))
print(pcall(assert, 0))               -- 0 is true
-- assert does not add position information to its message
print(pcall(function() assert(false, "no position") end))
print(pcall(function() assert(false) end))

-- stack overflow: only check the message kind
local function rec(n) return 1 + rec(n + 1) end
local ok2, msg2 = pcall(rec, 1)
print("overflow caught:", ok2, type(msg2), string.find(msg2, "stack overflow", 1, true) ~= nil)
-- the interpreter still works after an overflow
print("after overflow:", pcall(function() return 1 + 1 end))
-- overflow through metamethods
local mt = {}
mt.__index = function(tbl, k) return tbl[k] end
local loop = setmetatable({}, mt)
local ok3, msg3 = pcall(function() return loop.x end)
print("metamethod overflow:", ok3, string.find(msg3, "stack overflow", 1, true) ~= nil)
-- infinite __index chain of tables
local a, b = {}, {}
setmetatable(a, {__index = b}); setmetatable(b, {__index = a})
local ok4, msg4 = pcall(function() return a.x end)
print("index chain:", ok4, (msg4:gsub("^.-:%d+: ", "")))

-- float to integer conversions
print(pcall(string.rep, "x", 1.5))
print(pcall(string.sub, "hello", 1.0, 2.0))
print(pcall(string.sub, "hello", 1.5))
print(pcall(table.concat, {}, "", 1.5))
print(pcall(function() return ("x"):rep(2^63) end))
print(pcall(string.char, 65.0, 66.5))
print(pcall(utf8.char, 1e100))
print(pcall(math.tointeger, "8"))
print(pcall(function() local t = {}; t[1.5] = 1; return t[1.5], t[3.0] end))
print(pcall(string.format, "%x", 2^53))
print(pcall(string.format, "%x", 0.5))
print(pcall(function() for i = 1, 3, 0 do end end))
print(pcall(function() for i = 1.0, 3, 0 do end end))
print(pcall(function() for i = "a", 2 do end end))
print(pcall(function() for i = 1, {} do end end))
print(pcall(function() for i = 1, 2, nil do end end))
print(pcall(function() for i = "1", 2 do end end))

-- string to number conversions in library functions
print(pcall(string.rep, "x", "2"))
print(pcall(string.rep, "x", "2.0"))
print(pcall(string.rep, "x", "2.5"))
print(pcall(string.rep, "x", "two"))
print(pcall(math.abs, "-3"))
print(pcall(math.abs, " 0x10 "))

-- select('#') on error returns
print(select("#", pcall(error)))
print(select("#", pcall(error, "a", 0)))
print(select("#", xpcall(error, function() end)))
print(select("#", pcall(function() return end)))
print(select("#", pcall(function() return nil, nil, nil end)))

-- error() with a number message: kept as number, no position
local ok5, e5 = pcall(function() error(7) end)
print(ok5, e5, math.type(e5))
