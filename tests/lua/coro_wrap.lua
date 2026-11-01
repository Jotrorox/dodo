-- coroutine.wrap: values passed through, errors propagated to the caller
-- with position information (string errors get the position where they
-- were raised, non-string errors pass through untouched), calling a dead
-- wrapped coroutine, and wrap used as a generic-for iterator.

local function gen(n)
  return coroutine.wrap(function()
    for i = 1, n do coroutine.yield(i, i * i) end
  end)
end
for i, sq in gen(4) do io.write(i, "=", sq, " ") end
print()

-- values both ways
local w = coroutine.wrap(function(a)
  local b = coroutine.yield(a .. "1")
  local c, d = coroutine.yield(b .. "2")
  return c, d, "ret"
end)
print(w("x"))
print(w("y"))
print(w("p", "q"))

-- calling a dead wrap
print(pcall(w))
print(pcall(w, "more"))

-- error inside a wrapped coroutine: string gets position info
local bad = coroutine.wrap(function()
  coroutine.yield(1)
  error("oops")
end)
print(bad())
local ok, msg = pcall(bad)
print(ok, msg)
-- after the error, the wrap is dead
print(pcall(bad))

-- error with level 0: no position
local bad0 = coroutine.wrap(function() error("plain", 0) end)
print(pcall(bad0))

-- error with a table value: passes through as the same object
local errobj = {code = 42}
local badt = coroutine.wrap(function() error(errobj) end)
local ok2, e2 = pcall(badt)
print(ok2, e2 == errobj, type(e2), e2.code)

-- runtime error inside wrap (not via error())
local badrt = coroutine.wrap(function() local t = nil; return t.x end)
print(pcall(badrt))

-- error with nil value
local badnil = coroutine.wrap(function() error(nil) end)
print(pcall(badnil))

-- nested wraps: error propagates through two levels
local outer = coroutine.wrap(function()
  local inner = coroutine.wrap(function() error("deep") end)
  inner()
end)
print(pcall(outer))

-- wrap caught by pcall inside another coroutine, which keeps running
local co = coroutine.create(function()
  local inner = coroutine.wrap(function() error({}) end)
  local ok3, e3 = pcall(inner)
  coroutine.yield(ok3, type(e3))
  return "survived"
end)
print(coroutine.resume(co))
print(coroutine.resume(co))

-- wrap with a non-function argument
print(pcall(coroutine.wrap, 1))
print(pcall(coroutine.wrap))

-- wrapped function yielding nothing returns nothing
local nothing = coroutine.wrap(function() coroutine.yield() end)
print(select("#", nothing()), select("#", nothing()))

-- wrapped function attempting to resume itself
local selfref
selfref = coroutine.wrap(function() return selfref() end)
print(pcall(selfref))

-- type of wrap result
print(type(coroutine.wrap(print)))
