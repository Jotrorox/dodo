-- os.exit(3): the process exits immediately with status 3; buffered
-- output written before is flushed (C exit flushes stdio), code after
-- the call never runs, and pending to-be-closed variables are not closed
-- because the state is not closed (close argument absent).

collectgarbage("stop")   -- no automatic collection may finalize early
io.write("before exit, no newline; ")
print("printed")
local guard <close> = setmetatable({}, {__close = function() print("tbc closed (unexpected)") end})
setmetatable({}, {__gc = function() print("finalizer ran (unexpected)") end})
os.exit(3)
print("not reached")
