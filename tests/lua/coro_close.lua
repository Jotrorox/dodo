-- coroutine.close: on fresh, suspended (with pending to-be-closed
-- variables, closed in reverse order), dead-normally and dead-with-error
-- coroutines; errors raised by __close handlers during close; closing
-- the running coroutine or a normal one is an error.

local function closer(name, fail)
  return setmetatable({}, {__close = function(_, err)
    print("  closing " .. name, "err=" .. tostring(err))
    if fail then error(fail, 0) end
  end})
end

-- fresh coroutine
local fresh = coroutine.create(function() print("never runs") end)
print("close fresh:", coroutine.close(fresh))
print("status:", coroutine.status(fresh))
print("resume closed:", coroutine.resume(fresh))

-- suspended with pending tbc variables
local co = coroutine.create(function()
  local a <close> = closer("a")
  local b <close> = closer("b")
  do
    local c <close> = closer("c")
    coroutine.yield("in block")
  end
  print("unreachable after close")
end)
print("resume:", coroutine.resume(co))
print("close suspended:", coroutine.close(co))
print("status:", coroutine.status(co))
print("close again:", coroutine.close(co))

-- dead normally
local done = coroutine.create(function() return 1 end)
coroutine.resume(done)
print("close dead ok:", coroutine.close(done))

-- dead with an error: close returns false plus the error object
local failed = coroutine.create(function() error("failure here") end)
print("resume:", coroutine.resume(failed))
print("close dead err:", coroutine.close(failed))
print("close again:", coroutine.close(failed))
local tbl = {}
local failed2 = coroutine.create(function() error(tbl) end)
coroutine.resume(failed2)
local ok, e = coroutine.close(failed2)
print("close dead table err:", ok, e == tbl)

-- __close raising an error while closing a suspended coroutine
local co2 = coroutine.create(function()
  local x <close> = closer("x")
  local y <close> = closer("y", "y failed")
  local z <close> = closer("z")
  coroutine.yield()
end)
coroutine.resume(co2)
print("close with failing handler:", coroutine.close(co2))
print("status:", coroutine.status(co2))

-- two failing handlers: the later error replaces the earlier one
local co3 = coroutine.create(function()
  local p <close> = closer("p", "p failed")
  local q <close> = closer("q", "q failed")
  coroutine.yield()
end)
coroutine.resume(co3)
print("close with two failures:", coroutine.close(co3))

-- closing the running coroutine
local co4
co4 = coroutine.create(function()
  return pcall(coroutine.close, co4)
end)
print("close running:", coroutine.resume(co4))

-- closing a normal coroutine (it resumed the current one)
local outer
outer = coroutine.create(function()
  local inner = coroutine.create(function()
    return pcall(coroutine.close, outer)
  end)
  return coroutine.resume(inner)
end)
print("close normal:", coroutine.resume(outer))

-- closing the main thread
local main = coroutine.running()
print("close main:", pcall(coroutine.close, main))
print("close non-thread:", pcall(coroutine.close, 1))

-- are tbc variables closed when a coroutine errors during resume?
local co5 = coroutine.create(function()
  local v <close> = closer("v")
  error("raised", 0)
end)
print("resume erroring:", coroutine.resume(co5))
-- ...no: an erroring coroutine keeps its tbc variables pending until closed
print("status:", coroutine.status(co5))
print("close erroring:", coroutine.close(co5))
