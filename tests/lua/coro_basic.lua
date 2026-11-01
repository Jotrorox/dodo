-- Coroutine basics: create/resume/yield passing values in both directions,
-- multiple values, no values, nil holes, the first resume's arguments
-- becoming the body's parameters, and the final return values.

local function show(...)
  local n = select("#", ...)
  local t = {}
  for i = 1, n do t[i] = tostring((select(i, ...))) end
  return n .. ":[" .. table.concat(t, ",") .. "]"
end

-- values flowing both ways
local co = coroutine.create(function(a, b)
  print("body got", a, b)
  local c, d = coroutine.yield(a + b, a * b)
  print("yield 1 returned", c, d)
  local e = coroutine.yield()
  print("yield 2 returned", e)
  local f, g, h = coroutine.yield(nil, nil, 3)
  print("yield 3 returned", f, g, h)
  return "done", a, e
end)
print(type(co))
print("resume 1", show(coroutine.resume(co, 2, 5)))
print("resume 2", show(coroutine.resume(co, "x", "y", "extra")))
print("resume 3", show(coroutine.resume(co)))
print("resume 4", show(coroutine.resume(co, nil, nil, nil)))
print("status", coroutine.status(co))

-- excess and missing first-resume arguments
local co2 = coroutine.create(function(...)
  print("varargs", show(...))
  local x = coroutine.yield(select("#", ...))
  return show(x)
end)
print(show(coroutine.resume(co2, 1, nil, 3, nil)))
print(show(coroutine.resume(co2)))

-- body returning nothing
local co3 = coroutine.create(function() end)
print("empty body", show(coroutine.resume(co3)))

-- yield of many values
local co4 = coroutine.create(function()
  coroutine.yield(table.unpack({1, 2, 3, 4, 5, 6, 7, 8, 9, 10}))
  return table.unpack({}, 1, 3)
end)
print(show(coroutine.resume(co4)))
print(show(coroutine.resume(co4)))

-- a coroutine resuming another coroutine; values are not mixed up
local inner = coroutine.create(function(x)
  while true do x = coroutine.yield("inner:" .. x) end
end)
local outer = coroutine.create(function(x)
  while true do
    local ok, v = coroutine.resume(inner, x)
    x = coroutine.yield("outer(" .. v .. ")")
  end
end)
for i = 1, 3 do print(coroutine.resume(outer, i)) end

-- yield returns resume arguments; resume returns yield arguments: counter
local acc = coroutine.wrap(function(total)
  while true do
    local add = coroutine.yield(total)
    total = total + add
  end
end)
print(acc(10), acc(5), acc(-3), acc(0.5))

-- the same function can back several independent coroutines
local function counter(start)
  for i = start, start + 2 do coroutine.yield(i) end
  return "end"
end
local c1, c2 = coroutine.create(counter), coroutine.create(counter)
for _ = 1, 4 do
  print(select(2, coroutine.resume(c1, 100)), select(2, coroutine.resume(c2, 200)))
end

-- coroutine.create argument checks
print(pcall(coroutine.create, 42))
print(pcall(coroutine.create))
print(pcall(coroutine.resume, 42))
print(pcall(coroutine.resume))
-- C functions can be coroutine bodies
local cc = coroutine.create(print)
print(coroutine.resume(cc, "printed from C body"))
print(coroutine.status(cc))
local cy = coroutine.create(coroutine.yield)
print(coroutine.resume(cy, "a", "b"))
print(coroutine.resume(cy, "c"))
print(coroutine.status(cy))
