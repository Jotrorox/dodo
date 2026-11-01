-- An uncaught error whose error object is a table with __tostring: the
-- standalone interpreter's message handler converts it with __tostring
-- (no traceback position is added to non-string objects), pending <close>
-- variables of the main chunk are closed with the error object first, and
-- the script exits with status 1.

local E = {}
E.__index = E
E.__tostring = function(self) return "CustomError: " .. self.msg end

-- caught first, to show the object survives pcall unchanged
local ok, err = pcall(error, setmetatable({msg = "caught"}, E))
print("caught", ok, getmetatable(err) == E, tostring(err))

-- error objects of other types through pcall
print("number", pcall(error, 42))
print("nil", pcall(error, nil))
print("level 0", pcall(error, "no position", 0))
print("level 1", pcall(function() error("with position") end))
print("level 2", pcall(function() local function f() error("caller pos", 2) end; f() end))

local guard <close> = setmetatable({}, {__close = function(_, e)
  print("closing main chunk with", tostring(e))
  io.stdout:flush()
end})
error(setmetatable({msg = "uncaught"}, E))
