-- collectgarbage options and their return values (printing only types or
-- stable values), mode switching (incremental/generational return the
-- previous mode), stop/restart/isrunning, invalid options, weak tables
-- (weak keys, weak values, ephemerons) cleared by a full collection, and
-- finalizers (__gc) run by collect, including resurrection order.

print("collect:", collectgarbage("collect"), collectgarbage())
local kb = collectgarbage("count")
print("count:", math.type(kb), kb > 0)
print("step type:", type(collectgarbage("step")), type(collectgarbage("step", 100)))
print("isrunning:", collectgarbage("isrunning"))
print("stop:", collectgarbage("stop"), collectgarbage("isrunning"))
print("restart:", collectgarbage("restart"), collectgarbage("isrunning"))

-- mode switching returns the previous mode; note lua.c (the standalone
-- interpreter) switches the collector to generational mode at startup
print(collectgarbage("generational"))
print(collectgarbage("generational"))
print(collectgarbage("incremental"))
print(collectgarbage("incremental"))
print(collectgarbage("incremental", 100, 200, 10))
-- deprecated setpause/setstepmul still exist and return previous values
print(math.type(collectgarbage("setpause", 200)), math.type(collectgarbage("setstepmul", 100)))
print(math.type(collectgarbage("setpause", 200)))

-- invalid options
print(pcall(collectgarbage, "xyz"))
print(pcall(collectgarbage, 1))
print(pcall(collectgarbage, "step", "x"))

-- from here on, only explicit full collections run (deterministic order)
collectgarbage("stop")

-- weak values
local wv = setmetatable({}, {__mode = "v"})
wv[1] = {}
wv[2] = "strings are values, not collectable objects here"
wv[3] = 42
local keep = {}
wv[4] = keep
collectgarbage()
print("weak values:", wv[1], type(wv[2]), wv[3], wv[4] == keep)

-- weak keys
local wk = setmetatable({}, {__mode = "k"})
wk[{}] = "dropped"
wk[keep] = "kept"
wk["str"] = "string key stays"
collectgarbage()
local n = 0
for k, v in pairs(wk) do n = n + 1 end
print("weak keys:", n, wk[keep], wk.str)

-- ephemeron: value refers to its own key, still collected
local eph = setmetatable({}, {__mode = "k"})
do local k = {}; eph[k] = {ref = k} end
collectgarbage()
print("ephemeron entries:", next(eph) == nil)

-- fully weak table
local wkv = setmetatable({}, {__mode = "kv"})
wkv[{}] = 1; wkv[1] = {}; wkv.x = keep
collectgarbage()
local cnt = 0
for _ in pairs(wkv) do cnt = cnt + 1 end
print("kv entries:", cnt)

-- finalizers run on collect, in reverse order of being marked for finalization
local order = {}
for i = 1, 3 do setmetatable({}, {__gc = function() order[#order + 1] = i end}) end
collectgarbage()
print("finalizer order:", table.concat(order, ","))

-- __gc must be present when setmetatable is called
local late = {}
local mt = {}
setmetatable(late, mt)
mt.__gc = function() print("late __gc (should not run)") end
late = nil
collectgarbage()
print("late __gc skipped")

-- resurrection: the finalizer can store the object; it is not finalized twice
local saved
setmetatable({name = "phoenix"}, {__gc = function(o) saved = o end})
collectgarbage()
print("resurrected:", saved and saved.name)
saved = nil
collectgarbage()
print("after second collect:", saved)

-- a finalizer's object is removed from weak values before finalization
-- but kept in weak keys until after
local wvals = setmetatable({}, {__mode = "v"})
local wkeys = setmetatable({}, {__mode = "k"})
local seen_v, seen_k
do
  local obj = setmetatable({}, {__gc = function(o) seen_v = wvals[1]; seen_k = wkeys[o] end})
  wvals[1] = obj
  wkeys[obj] = "keyval"
end
collectgarbage()
print("in finalizer: weak value", seen_v, "weak key", seen_k)

-- collectgarbage inside a finalizer is allowed
setmetatable({}, {__gc = function() collectgarbage("step") end})
collectgarbage()
print("done")
