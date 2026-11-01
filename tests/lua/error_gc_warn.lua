--! stderr: full
-- Errors raised inside __gc finalizers are never propagated to the code
-- that triggered the collection; instead they become warnings
-- ("Lua warning: error in __gc" ...). Warnings are off by default, so the
-- first failing finalizer is silent; after warn("@on") they reach stderr.
-- Non-string error objects in finalizers are reported too.

local function make(msg)
  setmetatable({}, {__gc = function() error(msg, 0) end})
end

make("silent failure")
print("collect 1:", pcall(collectgarbage))
io.stdout:flush()

warn("@on")
make("loud failure")
print("collect 2:", pcall(collectgarbage))
io.stdout:flush()

setmetatable({}, {__gc = function() error({}) end})
print("collect 3:", pcall(collectgarbage))
io.stdout:flush()

-- a finalizer that succeeds after the failing ones still runs
setmetatable({}, {__gc = function() print("good finalizer ran") end})
collectgarbage()
warn("@off")
make("silent again")
collectgarbage()
print("end")
