--! env: DODO_LUA_GC_STRESS=1
-- A suspended coroutine that is reachable only through an open upvalue
-- referring to itself (its own parameter, captured by a closure stored
-- elsewhere) must stay alive across collections, whatever the collector
-- was doing when the closure was stored. A weak table shows whether any
-- of them was collected. (Run with our collector's stress setting, which
-- makes it work at every check point; the reference ignores it.)
local alive = setmetatable({}, {__mode = "k"})
local fs = {}
local function body(self)
  local pad = {}
  fs[#fs + 1] = function() return self, pad end
  coroutine.yield()
  return "resumed"
end
local function count()
  local n = 0
  for _ in pairs(alive) do n = n + 1 end
  return n
end
for _, mode in ipairs({"incremental", "generational"}) do
  collectgarbage(mode)
  fs = {}
  for i = 1, 300 do
    local co = coroutine.create(body)
    alive[co] = true
    coroutine.resume(co, co)
  end
  collectgarbage()
  collectgarbage()
  print(mode, "alive", count(), #fs)
  local good = 0
  for _, f in ipairs(fs) do
    local co = f()
    if alive[co] and coroutine.status(co) == "suspended" then
      local ok, v = coroutine.resume(co)
      if ok and v == "resumed" then good = good + 1 end
    end
  end
  print(mode, "resumed", good)
  fs = {}
  collectgarbage()
  collectgarbage()
  print(mode, "after release", count())
end
