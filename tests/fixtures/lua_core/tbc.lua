local function closer(name)
  return setmetatable({}, {__close = function(self, err) print("close", name, err) end})
end
do
  local a <close> = closer("a")
  local b <close> = closer("b")
  local c <const> = 10
  print("in block", c)
end
print("after block")
local function f()
  local x <close> = closer("x")
  return "ret"
end
print(f())
print(pcall(function()
  local y <close> = closer("y")
  error("boom")
end))
for i = 1, 2 do
  local z <close> = closer("z" .. i)
  if i == 1 then goto cont end
  print("i", i)
  ::cont::
end
while true do
  local w <close> = closer("w")
  break
end
local ok, e = pcall(function()
  local p <close> = setmetatable({}, {__close = function() error("close error") end})
  local q <close> = closer("q")
  error("original")
end)
print(ok, e)
print(pcall(function() local bad <close> = {} end))
print(pcall(function() local n <close> = nil; local f <close> = false; return "ok" end))
-- generic for closing value
local function iter()
  local closed = setmetatable({}, {__close = function() print("iterator closed") end})
  local i = 0
  return function() i = i + 1; if i <= 3 then return i end end, nil, nil, closed
end
for v in iter() do if v == 2 then break end print("v", v) end
for v in iter() do print("v", v) end
-- tbc in coroutine closed by coroutine.close
local co = coroutine.create(function()
  local r <close> = closer("co")
  coroutine.yield(1)
end)
coroutine.resume(co)
print(coroutine.close(co))
-- yield inside __close
local co2 = coroutine.wrap(function()
  do
    local t <close> = setmetatable({}, {__close = function() print("closing, yielding"); coroutine.yield("from close"); print("resumed in close") end})
  end
  return "after"
end)
print(co2())
print(co2())
-- tail call with tbc is not allowed? just return through
local function g()
  local h <close> = closer("h")
  return (function() return "inner" end)()
end
print(g())
-- multiple returns with tbc
local function m()
  local mm <close> = closer("mm")
  return 1, 2, 3
end
print(m())
