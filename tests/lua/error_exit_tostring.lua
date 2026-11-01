-- Uncaught error whose value is a table with a __tostring metamethod:
-- lua.c's message handler uses the __tostring result as the message, so
-- stderr shows "lua: custom error object (code 7)" and the exit status is 1.
-- Output before the error must still be flushed to stdout.

local E = {}
E.__index = E
E.__tostring = function(self)
  return "custom error object (code " .. self.code .. ")"
end

local function fail(code)
  error(setmetatable({code = code}, E))
end

print("before error")
local ok, e = pcall(fail, 3)
print("caught:", ok, tostring(e), getmetatable(e) == E)
io.write("partial line without newline; ")
fail(7)
print("not reached")
