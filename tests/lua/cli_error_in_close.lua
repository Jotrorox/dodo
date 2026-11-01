-- An uncaught error while a to-be-closed variable of the main chunk is
-- pending: the __close handler runs (receiving the error object) before
-- lua.c reports the error, so its output appears on stdout; exit 1.
-- Only the first line of the error is printed: in PUC Lua the message
-- handler of lua.c runs before the unwinding, so the object seen by
-- __close already has the traceback appended.

local res <close> = setmetatable({}, {__close = function(_, err)
  print("closing resource, error was:", (tostring(err):match("^[^\n]*")))
end})
print("working")
error("failure with pending tbc", 0)
