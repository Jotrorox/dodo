print(collectgarbage("isrunning"))
print(type(collectgarbage("count")))
print(collectgarbage("stop"), collectgarbage("isrunning"))
print(collectgarbage("restart"), collectgarbage("isrunning"))
print(collectgarbage("incremental"), collectgarbage("generational"), collectgarbage("incremental"))
print(collectgarbage(), collectgarbage("collect"))
print(pcall(collectgarbage, "bogus"))
-- weak values
local wv = setmetatable({}, {__mode = "v"})
wv[1] = {}; wv[2] = "str"; wv[3] = 42
local keep = {}
wv[4] = keep
collectgarbage()
print(wv[1], wv[2], wv[3], wv[4] == keep)
-- weak keys
local wk = setmetatable({}, {__mode = "k"})
local k1 = {}
wk[k1] = 1; wk[{}] = 2
collectgarbage()
local n = 0
for k, v in pairs(wk) do n = n + 1 end
print(n, wk[k1])
-- ephemeron: value refers to key
local eph = setmetatable({}, {__mode = "k"})
do
  local key = {}
  eph[key] = {ref = key}
end
collectgarbage()
n = 0
for k, v in pairs(eph) do n = n + 1 end
print("ephemeron entries", n)
-- finalizers
local order = {}
for i = 1, 5 do setmetatable({}, {__gc = function() order[#order + 1] = i end}) end
collectgarbage()
print(#order, order[1], order[5])
-- finalizer resurrection
local saved
do
  local o = setmetatable({name = "resurrected"}, {__gc = function(x) saved = x end})
end
collectgarbage()
print(saved and saved.name)
-- __gc set after setmetatable does not count
local fired = false
local mtx = {}
local obj = setmetatable({}, mtx)
mtx.__gc = function() fired = true end
obj = nil
collectgarbage()
print("late gc fired", fired)
-- errors in finalizers become warnings (warnings off by default)
setmetatable({}, {__gc = function() error("in gc") end})
collectgarbage()
print("survived gc error")
-- weak value with resurrected object
local wt = setmetatable({}, {__mode = "v"})
do
  local x = setmetatable({}, {__gc = function() end})
  wt[1] = x
end
collectgarbage()
print("weak cleared before finalizer", wt[1])
-- big allocation churn
local t = {}
for i = 1, 200000 do t[i % 100] = {i} end
collectgarbage()
print("churn ok", collectgarbage("count") < 10000)
for i = 1, 50 do collectgarbage("step") end
local finished = false
for i = 1, 10000 do if collectgarbage("step", 1) then finished = true break end end
print("step finished", finished)
print(collectgarbage("setpause", 150), collectgarbage("setpause", 200))
print(collectgarbage("setstepmul", 200), collectgarbage("setstepmul", 100))
-- strings survive
local strs = {}
for i = 1, 1000 do strs[i] = "s" .. i end
collectgarbage()
print(strs[1], strs[1000])
-- closures and upvalues under GC
local fns = {}
for i = 1, 1000 do local v = {i}; fns[i] = function() return v[1] end end
collectgarbage()
local s = 0
for i = 1, 1000 do s = s + fns[i]() end
print(s)
-- coroutines collected
for i = 1, 100 do local co = coroutine.create(function() coroutine.yield() end); coroutine.resume(co) end
collectgarbage()
print("coroutines ok")
-- finalizer order at close
setmetatable({}, {__gc = function() print("finalized at close 1") end})
setmetatable({}, {__gc = function() print("finalized at close 2") end})
