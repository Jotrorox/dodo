-- __gc finalizers: run after a full collection of an unreachable object,
-- not run for reachable objects, __gc must be present when setmetatable is
-- called (setting it later does not mark the object), run in reverse order
-- of marking (object creation order for setmetatable calls), resurrection,
-- errors in finalizers become warnings (not propagated), and finalizers
-- run at state close (lua.c closes the state on normal exit).

local log = {}
local function fin(name) return {__gc = function(o) log[#log + 1] = name .. ":" .. tostring(o.id) end} end

do
  local a = setmetatable({id = 1}, fin("a"))
end
collectgarbage()
print("collected", table.concat(log, " "))
log = {}

-- reachable objects are not finalized
local keep = setmetatable({id = 2}, fin("keep"))
collectgarbage()
print("reachable", #log)

-- reverse order of marking: marked in creation order, finalized in reverse
do
  for i = 1, 5 do setmetatable({id = i}, fin("ord")) end
end
collectgarbage()
print("order", table.concat(log, " "))
log = {}

-- __gc added to the metatable after setmetatable: object not marked
local mt = {}
do local late = setmetatable({id = 3}, mt) end
mt.__gc = function(o) log[#log + 1] = "late" end
collectgarbage()
print("late __gc", #log)
-- but a placeholder field set before setmetatable marks the object, and
-- the function present at collection time is the one called
local mt2 = {__gc = true}
do local early = setmetatable({id = 4}, mt2) end
mt2.__gc = function(o) log[#log + 1] = "replaced:" .. o.id end
collectgarbage()
print("placeholder", table.concat(log, " "))
log = {}

-- a non-function __gc at collection time is silently ignored
do setmetatable({}, {__gc = 42}) end
collectgarbage()
print("non-function __gc ok", #log)

-- resurrection: the finalizer stores the object; it stays usable, and its
-- finalizer is not called again
local saved
do
  setmetatable({id = 5, data = "alive"}, {__gc = function(o) saved = o; log[#log + 1] = "fin5" end})
end
collectgarbage()
print("resurrected", saved and saved.data, table.concat(log, " "))
saved = nil
collectgarbage()
collectgarbage()
print("finalized once", table.concat(log, " "))
log = {}

-- objects referenced only by a finalized object are still accessible in
-- the finalizer
do
  local child = {name = "child"}
  setmetatable({id = 6, child = child}, {__gc = function(o) log[#log + 1] = o.child.name end})
end
collectgarbage()
print("child access", table.concat(log, " "))
log = {}

-- errors in finalizers do not propagate (they generate warnings, which are
-- off by default)
do setmetatable({}, {__gc = function() error("in gc") end}) end
local ok = pcall(collectgarbage)
print("gc error swallowed", ok)

-- finalizer that allocates and triggers further work
do
  setmetatable({id = 7}, {__gc = function(o)
    local t = {}
    for i = 1, 100 do t[i] = {i} end
    log[#log + 1] = "alloc" .. #t
  end})
end
collectgarbage()
print("alloc in gc", table.concat(log, " "))
log = {}

-- collectgarbage options with deterministic results
print("step returns boolean", type(collectgarbage("step")))
print("isrunning", collectgarbage("isrunning"))
collectgarbage("stop")
print("stopped", collectgarbage("isrunning"))
collectgarbage("restart")
print("restarted", collectgarbage("isrunning"))
print("count type", type(collectgarbage("count")))
print("bad option", pcall(collectgarbage, "nonsense"))
print("collect returns", collectgarbage("collect"))

-- finalizers pending at exit run when the state closes
local closer = setmetatable({}, {__gc = function() print("closing state: finalizer ran") end})
print("end of script")
