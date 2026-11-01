-- debug.sethook/gethook: line hooks (collected line numbers of a small
-- function), call/return hooks (event names and called function names),
-- count hooks, gethook results, hooks per coroutine, and removing hooks.

local function target(n)
  local s = 0
  for i = 1, n do
    s = s + i
  end
  return s
end

-- line hook: lines executed inside target only
local lines = {}
debug.sethook(function(ev, line)
  local info = debug.getinfo(2, "S")
  if info.linedefined == 5 then lines[#lines + 1] = line end
end, "l")
target(2)
debug.sethook()
print("lines:", table.concat(lines, ","))

-- gethook
print("no hook:", debug.gethook())
local function hk() end
debug.sethook(hk, "cr", 0)
local h, mask, count = debug.gethook()
print("gethook:", h == hk, mask, count)
debug.sethook(hk, "l", 5)
h, mask, count = debug.gethook()
print("gethook:", h == hk, mask, count)
debug.sethook()
print("removed:", debug.gethook())

-- call and return hooks
local events = {}
local function leaf() return 1 end
local function mid() local r = leaf(); return r end
local function tailmid() return leaf() end
debug.sethook(function(ev)
  local info = debug.getinfo(2, "nS")
  if info.what == "Lua" and info.short_src == "debug_hooks.lua" then
    events[#events + 1] = ev .. ":" .. tostring(info.name)
  end
end, "cr")
mid()
tailmid()
debug.sethook()
print("events:", table.concat(events, " "))

-- call hook sees C functions too
local cevents = {}
debug.sethook(function(ev)
  local info = debug.getinfo(2, "nS")
  if info.what == "C" and info.name == "type" then cevents[#cevents + 1] = ev end
end, "cr")
local _ = type(1)
debug.sethook()
print("C events:", table.concat(cevents, " "))

-- count hook: fires every N instructions
local fired = 0
debug.sethook(function(ev) if ev == "count" then fired = fired + 1 end end, "", 10)
target(200)
debug.sethook()
print("count hook fired:", fired > 10)
-- a count of 0 with an empty mask disables the hook
debug.sethook(function() end, "", 0)
print("empty mask:", debug.gethook())
debug.sethook()

-- hooks are per thread
local co = coroutine.create(function()
  print("hook inside coroutine:", debug.gethook() ~= nil)
end)
debug.sethook(hk, "l")
coroutine.resume(co)
debug.sethook()
local co2 = coroutine.create(function() coroutine.yield() end)
debug.sethook(co2, hk, "c")
print("hook on other coroutine:", debug.gethook(co2) == hk, debug.gethook() == nil)
debug.sethook(co2)
print("removed from coroutine:", debug.gethook(co2))

-- the hook function receives (event, line) for line events
local args
debug.sethook(function(...) if not args then args = {select("#", ...), ...} end end, "l")
local z = 1
debug.sethook()
print("hook args:", args[1], args[2], math.type(args[3]))

-- errors inside a hook propagate to the hooked code
local ok, err = pcall(function()
  debug.sethook(function() debug.sethook(); error("from hook", 0) end, "l")
  local a = 1
  return a
end)
debug.sethook()
print("hook error:", ok, err)

print(pcall(debug.sethook, 42, "l"))
