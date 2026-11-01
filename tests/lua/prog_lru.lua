-- Program test: an LRU cache built from a doubly-linked list with sentinel
-- nodes plus a hash map from key to node. Prints an eviction trace for a
-- scripted access sequence, uses an eviction callback, a memoizing wrapper
-- with hit/miss statistics, mixed key types (strings, integers, floats that
-- normalize to integers, booleans, tables as identity keys), resize with
-- shrink-eviction, and a cross-check against a naive array-based LRU driven
-- by a deterministic LCG.

local LRU = {}
LRU.__index = LRU

function LRU.new(capacity, on_evict)
  assert(math.type(capacity) == "integer" and capacity > 0, "capacity must be a positive integer")
  local head, tail = {}, {}
  head.next, tail.prev = tail, head
  return setmetatable({ cap = capacity, size = 0, map = {}, head = head, tail = tail,
                        on_evict = on_evict, hits = 0, misses = 0 }, LRU)
end

local function unlink(node)
  node.prev.next = node.next
  node.next.prev = node.prev
end

local function push_front(self, node)
  node.next = self.head.next
  node.prev = self.head
  self.head.next.prev = node
  self.head.next = node
end

function LRU:get(key)
  local node = self.map[key]
  if not node then
    self.misses = self.misses + 1
    return nil
  end
  self.hits = self.hits + 1
  unlink(node)
  push_front(self, node)
  return node.value
end

function LRU:evict_one()
  local victim = self.tail.prev
  unlink(victim)
  self.map[victim.key] = nil
  self.size = self.size - 1
  if self.on_evict then self.on_evict(victim.key, victim.value) end
end

function LRU:put(key, value)
  local node = self.map[key]
  if node then
    node.value = value
    unlink(node)
    push_front(self, node)
    return
  end
  if self.size >= self.cap then self:evict_one() end
  node = { key = key, value = value }
  self.map[key] = node
  push_front(self, node)
  self.size = self.size + 1
end

function LRU:delete(key)
  local node = self.map[key]
  if not node then return false end
  unlink(node)
  self.map[key] = nil
  self.size = self.size - 1
  return true
end

function LRU:resize(cap)
  self.cap = cap
  while self.size > cap do self:evict_one() end
end

-- iterate from most to least recently used
function LRU:items()
  local node = self.head
  return function()
    node = node.next
    if node ~= self.tail then return node.key, node.value end
  end
end

local function keystr(k)
  if type(k) == "table" then return k.name or "<table>" end
  return tostring(k)
end

function LRU:__tostring()
  local parts = {}
  for k, v in self:items() do parts[#parts + 1] = keystr(k) .. "=" .. tostring(v) end
  return "[" .. table.concat(parts, " ") .. "]"
end

function LRU:__len() return self.size end

-- scripted trace -------------------------------------------------------

local evicted = {}
local cache = LRU.new(3, function(k, v) evicted[#evicted + 1] = keystr(k) .. ":" .. tostring(v) end)
local script = {
  {"put", "a", 1}, {"put", "b", 2}, {"put", "c", 3}, {"get", "a"}, {"put", "d", 4},
  {"get", "b"}, {"get", "c"}, {"put", "e", 5}, {"put", "c", 33}, {"get", "a"},
  {"del", "d"}, {"put", "f", 6}, {"put", "g", 7}, {"get", "zz"}, {"put", "e", 55},
}
for step, op in ipairs(script) do
  local result = ""
  if op[1] == "put" then cache:put(op[2], op[3])
  elseif op[1] == "get" then result = " -> " .. tostring(cache:get(op[2]))
  else result = " -> " .. tostring(cache:delete(op[2])) end
  print(string.format("%2d %-3s %-2s%s  %s  size=%d", step, op[1], op[2], result, tostring(cache), #cache))
end
print("evicted:", table.concat(evicted, " "))
print("hits/misses:", cache.hits, cache.misses)

-- mixed key types --------------------------------------------------------

local t1, t2 = {name = "T1"}, {name = "T2"}
local mixed = LRU.new(4)
mixed:put(1, "int one")
mixed:put(1.0, "float one (same key)")
mixed:put("1", "string one")
mixed:put(true, "yes")
mixed:put(t1, "table1")
mixed:put(2^53, "big")
print("mixed:", tostring(mixed))
print("lookup 1:", mixed:get(1), "lookup '1':", mixed:get("1"), "t2:", mixed:get(t2))
mixed:resize(2)
print("after resize 2:", tostring(mixed), #mixed)

-- memoization ------------------------------------------------------------

local function memoize(fn, cap)
  local c = LRU.new(cap)
  return function(n)
    local v = c:get(n)
    if v == nil then
      v = fn(n)
      c:put(n, v)
    end
    return v
  end, c
end

local slow_calls = 0
local fib
local memo_fib, fib_cache = memoize(function(n)
  slow_calls = slow_calls + 1
  if n < 2 then return n end
  return fib(n - 1) + fib(n - 2)
end, 100)
fib = memo_fib
print("fib(80):", fib(80), "calls:", slow_calls, "hits:", fib_cache.hits, "misses:", fib_cache.misses)

-- small capacity forces recomputation
slow_calls = 0
local fib2
local memo2, cache2 = memoize(function(n)
  slow_calls = slow_calls + 1
  if n < 2 then return n end
  return fib2(n - 1) + fib2(n - 2)
end, 3)
fib2 = memo2
print("fib2(25):", fib2(25), "calls:", slow_calls, "size:", #cache2)

-- cross-check against a naive implementation --------------------------------

local Naive = {}
Naive.__index = Naive
function Naive.new(cap) return setmetatable({cap = cap, keys = {}, vals = {}}, Naive) end
function Naive:find(k)
  for i, key in ipairs(self.keys) do if key == k then return i end end
end
function Naive:get(k)
  local i = self:find(k)
  if not i then return nil end
  local key, val = table.remove(self.keys, i), table.remove(self.vals, i)
  table.insert(self.keys, 1, key); table.insert(self.vals, 1, val)
  return val
end
function Naive:put(k, v)
  local i = self:find(k)
  if i then table.remove(self.keys, i); table.remove(self.vals, i)
  elseif #self.keys >= self.cap then self.keys[#self.keys] = nil; self.vals[#self.vals] = nil end
  table.insert(self.keys, 1, k); table.insert(self.vals, 1, v)
end

local seed = 12345
local function rand(n)
  seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
  return seed % n
end

local fast, naive = LRU.new(16), Naive.new(16)
local mismatches, gets, found = 0, 0, 0
for i = 1, 5000 do
  local k = "k" .. rand(40)
  if rand(3) == 0 then
    fast:put(k, i); naive:put(k, i)
  else
    gets = gets + 1
    local a, b = fast:get(k), naive:get(k)
    if a ~= b then mismatches = mismatches + 1 end
    if a then found = found + 1 end
  end
end
local order_ok = true
local idx = 0
for k in fast:items() do
  idx = idx + 1
  if naive.keys[idx] ~= k then order_ok = false end
end
print("cross-check:", "gets", gets, "found", found, "mismatches", mismatches,
      "order", order_ok, "size", #fast, idx)
print("final order:", tostring(fast):sub(1, 60))
print(pcall(LRU.new, 0))
