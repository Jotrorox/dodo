-- pcall and xpcall: returned values on success and failure, extra
-- arguments, message handlers receiving the original error object,
-- handler return values, errors inside the handler, nested protected
-- calls, and argument errors.

local function count(...) return select("#", ...), ... end

-- success values
print(pcall(function() return 1, nil, 3 end))
print(count(pcall(function() end)))
print(count(pcall(function() return nil end)))
print(pcall(math.max, 3, 9, 4))
print(pcall(select, "#", 1, 2, 3))

-- failure values
print(count(pcall(error, "e", 0)))
print(count(pcall(error)))
print(pcall(nil))
print(pcall(42))
print(pcall({}))
print(pcall(pcall))
print(pcall(pcall, error, "inner", 0))

-- xpcall with arguments
print(xpcall(function(a, b) return a + b end, print, 3, 4))
print(xpcall(function(...) return select("#", ...) end, print, nil, nil))

-- handler receives the original object (not a string)
local obj = setmetatable({}, {__tostring = function() return "OBJ" end})
print(xpcall(function() error(obj) end, function(e)
  print("handler got same object:", e == obj, tostring(e))
  return "replaced", "second"
end))

-- handler sees string with position; handler gets exactly one argument
print(xpcall(function() error("boom") end, function(...)
  print("handler nargs:", select("#", ...))
  return (...):upper()
end))

-- only the handler's first result is returned (exactly one value)
print(count(xpcall(error, function() return 1, 2, 3 end, "x")))
print(count(xpcall(error, function() end, "x")))

-- handler can call debug.traceback; print only its first line
print(xpcall(function() error("tb", 0) end, function(m)
  local tb = debug.traceback(m, 1)
  return tb:match("^[^\n]*") .. " | " .. tostring(tb:find("\nstack traceback:\n", 1, true) ~= nil)
end))

-- runtime error object passed to handler
print(xpcall(function() local x; return x.y end, function(m) return "H: " .. m end))

-- error inside the handler
local ok, msg = xpcall(function() error("first") end, function(m) error("second") end)
print(ok, type(msg))
ok, msg = xpcall(function() error("first") end, function(m) local z = nil; return z.q end)
print(ok, type(msg))

-- handler that is not callable
print(pcall(xpcall, function() error("x") end, 42))
print(pcall(xpcall, function() return "fine" end, 42))
print(pcall(xpcall, function() end))

-- nested pcall: inner failure does not affect outer
print(pcall(function()
  local a, b = pcall(error, "inner", 0)
  local c, d = pcall(function() return "ok-inner" end)
  return a, b, c, d
end))
print(pcall(function()
  pcall(error, "swallowed")
  error("outer", 0)
end))

-- xpcall nested in xpcall: each uses its own handler
print(xpcall(function()
  local r = {xpcall(function() error("in", 0) end, function(m) return "inner:" .. m end)}
  error(table.concat({tostring(r[1]), r[2]}, "/"), 0)
end, function(m) return "outer:" .. m end))

-- pcall returning many values
local t = {pcall(table.unpack, {1, 2, 3, 4, 5, 6, 7, 8, 9, 10})}
print(#t, t[1], t[11])

-- error values that are false/nil
print(pcall(error, false))
print(pcall(error, nil))

-- pcall with a callable table
local callable = setmetatable({}, {__call = function(self, x) return "called", x end})
print(pcall(callable, 5))
print(xpcall(callable, print, 6))
