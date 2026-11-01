-- When the script dies with an uncaught error, lua.c prints the message
-- to stderr and still closes the state, so pending finalizers run (their
-- output appears on stdout) and the exit status is 1.

local keep = setmetatable({}, {__gc = function() print("finalizer ran after the error") end})
print("about to fail")
error("fatal problem")
