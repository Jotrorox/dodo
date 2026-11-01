-- os.exit(code, true) closes the Lua state before exiting: pending
-- pending to-be-closed variables of the main thread are closed first,
-- then all finalizers run (in reverse order of setmetatable), and the
-- exit status is the given code (here 7). Float codes with an integral
-- value are accepted.

collectgarbage("stop")   -- nothing may be finalized before os.exit
setmetatable({}, {__gc = function() print("finalizer 1") end})
setmetatable({}, {__gc = function() print("finalizer 2") end})
local keep = setmetatable({}, {__gc = function() print("finalizer of live object") end})
local tbc <close> = setmetatable({}, {__close = function(_, e) print("tbc closed, err:", e) end})
print("calling os.exit(7.0, true)")
os.exit(7.0, true)
print("not reached")
