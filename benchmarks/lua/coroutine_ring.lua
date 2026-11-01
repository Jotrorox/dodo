-- Benchmark: coroutine switching. (1) a ring of coroutines passing a token
-- around many times, (2) a generator pipeline (producer -> filter -> map ->
-- consumer) built with coroutine.wrap. Stresses resume/yield, value passing
-- across coroutine boundaries and coroutine creation.

local RING = 503
local PASSES = 6000000
local PIPELINE = 1000000
local SHORT_LIVED = 150000

-- (1) token ring: each coroutine receives a counter, decrements it, and
-- hands it to the next one; the one that sees 0 reports its id.
local ring = {}
for id = 1, RING do
  ring[id] = coroutine.create(function(token)
    while true do
      if token == 0 then return id end
      token = coroutine.yield(token - 1)
    end
  end)
end
local token, id, winner = PASSES, 1, nil
while true do
  local ok, v = coroutine.resume(ring[id], token)
  if coroutine.status(ring[id]) == "dead" then winner = v break end
  token = v
  id = id % RING + 1
end
print("ring winner", winner)

-- (2) generator pipeline
local function producer(n)
  return coroutine.wrap(function()
    for i = 1, n do coroutine.yield(i) end
  end)
end
local function filter(gen, pred)
  return coroutine.wrap(function()
    for v in gen do if pred(v) then coroutine.yield(v) end end
  end)
end
local function map(gen, f)
  return coroutine.wrap(function()
    for v in gen do coroutine.yield(f(v)) end
  end)
end

local sum, count = 0, 0
for v in map(filter(producer(PIPELINE), function(x) return x % 3 ~= 0 end), function(x) return x * 2 end) do
  sum = sum + v
  count = count + 1
end
-- many short-lived coroutines
local created = 0
for i = 1, SHORT_LIVED do
  local co = coroutine.wrap(function(a) local b = coroutine.yield(a + 1); return a + b end)
  created = created + co(i) + co(1)
end
print("pipeline", count, sum, "created", created)
