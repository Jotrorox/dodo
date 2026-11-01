local co = coroutine.create(function(a, b)
  print("start", a, b)
  local c = coroutine.yield(a + b)
  print("got", c)
  local d, e = coroutine.yield(c * 2)
  print("got2", d, e)
  return "end", 99
end)
print(coroutine.status(co))
print(coroutine.resume(co, 1, 2))
print(coroutine.status(co))
print(coroutine.resume(co, 10))
print(coroutine.resume(co, "x", "y"))
print(coroutine.status(co))
print(coroutine.resume(co))
local gen = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i) end end)
print(gen(), gen(), gen())
-- yield across pcall
local co2 = coroutine.create(function()
  local ok, v = pcall(function()
    local x = coroutine.yield("in pcall")
    error("after yield " .. x)
  end)
  print("pcall result", ok, v)
  local ok3, v3 = pcall(coroutine.yield, "second")
  print("second", ok3, v3)
  return "finished"
end)
print(coroutine.resume(co2))
print(coroutine.resume(co2, "X"))
print(coroutine.resume(co2, "Y"))
print(coroutine.status(co2))
-- errors inside coroutines
local co3 = coroutine.create(function() error("oops") end)
print(coroutine.resume(co3))
print(coroutine.status(co3))
print(coroutine.resume(co3))
print(coroutine.resume(coroutine.running()))
print(coroutine.isyieldable())
print(coroutine.running())
print(pcall(coroutine.yield, 1))
-- wrap errors
local w = coroutine.wrap(function() error("werr") end)
print(pcall(w))
local w2 = coroutine.wrap(function() error({}) end)
print(pcall(w2))
-- yield inside metamethods
local mt = {__index = function(t, k) return coroutine.yield(k) end,
            __add = function(a, b) return coroutine.yield("add") end,
            __lt = function(a, b) return coroutine.yield("lt") end,
            __concat = function(a, b) return coroutine.yield("concat") end,
            __eq = function(a, b) return coroutine.yield("eq") end,
            __len = function(a) return coroutine.yield("len") end,
            __newindex = function(t, k, v) coroutine.yield("newindex") end,
            __call = function(self, x) return coroutine.yield("call") end}
local co4 = coroutine.wrap(function()
  local o = setmetatable({}, mt)
  local o2 = setmetatable({}, mt)
  local v = o.foo
  print("index ->", v)
  print("add ->", o + 1)
  print("lt ->", o < o2)
  print("concat ->", "a" .. o .. "b")
  print("eq ->", o == o2)
  print("len ->", #o)
  o.x = 1
  print("call ->", o(5))
  return "done"
end)
print(co4())
print(co4("FOO"))
print(co4(7))
print(co4(true))
print(co4("CAT"))
print(co4(false))
print(co4(3))
print(co4())
print(co4("CALLED"))
-- yield in generic for iterator
local function iter()
  return coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i, i * i) end end)
end
for i, sq in iter() do print("iter", i, sq) end
-- coroutine.close
local co5 = coroutine.create(function() coroutine.yield() end)
coroutine.resume(co5)
print(coroutine.close(co5), coroutine.status(co5))
local co6 = coroutine.create(function() error("x") end)
coroutine.resume(co6)
print(coroutine.close(co6))
print(pcall(coroutine.close, coroutine.running()))
-- isyieldable inside
print(coroutine.wrap(function() return coroutine.isyieldable() end)())
-- status running / normal
local outer
outer = coroutine.create(function()
  local inner = coroutine.create(function() print("outer status from inner:", coroutine.status(outer)) end)
  coroutine.resume(inner)
  print("self status:", coroutine.status(outer))
end)
coroutine.resume(outer)
-- resume dead
print(coroutine.resume(outer))
-- many coroutines
local count = 0
for i = 1, 1000 do
  local c = coroutine.wrap(function(x) local y = coroutine.yield(x + 1); return y * 2 end)
  count = count + c(i) + c(i)
end
print(count)
-- yield across pcall with error after resume caught by pcall
local co7 = coroutine.wrap(function()
  local r = {pcall(function() coroutine.yield(1); error("E") end)}
  return r[1], r[2]
end)
print(co7())
print(co7())
-- xpcall with handler across yields
local co8 = coroutine.wrap(function()
  return xpcall(function() coroutine.yield("y"); error("bad") end, function(m) return "H:" .. m end)
end)
print(co8())
print(co8())
-- table.sort-like non-yieldable: yield from main
print(pcall(coroutine.yield))
-- wrap generator in loop
local function range(n) return coroutine.wrap(function() for i = 1, n do coroutine.yield(i) end end) end
local tot = 0
for v in range(100) do tot = tot + v end
print(tot)
