-- Benchmark: upvalue access. Counters and accumulators kept in upvalues,
-- read and written from closures in tight loops, upvalues shared between
-- sibling closures, and upvalues two levels up. Stresses GETUPVAL, SETUPVAL
-- and calls of small closures.

local N = 16000000

local function counter()
  local n = 0
  return function() n = n + 1; return n end, function() return n end
end

local function make_acc(scale)
  local total, count = 0, 0
  local function add(x) total = total + x * scale; count = count + 1 end
  local function get() return total, count end
  return add, get
end

local function nested()
  local depth = 0
  return function()
    return function(x)
      depth = depth + x
      return depth
    end
  end
end

local inc, peek = counter()
local add, get = make_acc(3)
local deep = nested()()
local s = 0
for i = 1, N do
  inc()
  add(i & 7)
  s = s + deep(1) - peek()
end
local total, count = get()
print(peek(), total, count, s)
