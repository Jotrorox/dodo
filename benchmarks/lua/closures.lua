-- Benchmark: closures and upvalues. Creates many closures (counters,
-- adders, composed functions), calls them through tables, and reads/writes
-- shared upvalues from nested functions, including upvalues that are
-- closed when the enclosing loop iteration ends.

local N = 3000000

local function make_counter()
  local count = 0
  return function(step)
    count = count + step
    return count
  end
end

local function make_adder(k)
  return function(x) return x + k end
end

local function compose(f, g)
  return function(x) return f(g(x)) end
end

local total = 0

-- closure creation in a loop; each iteration captures a fresh local
local fns = {}
for i = 1, N do
  local j = i % 100
  fns[i % 1000 + 1] = function() return j end
  total = total + fns[i % 1000 + 1]()
end

-- counters with private state
local counters = {}
for i = 1, 100 do counters[i] = make_counter() end
for i = 1, N * 4 do
  total = total + counters[i % 100 + 1](1)
end

-- composition chains
for i = 1, N // 10 do
  local f = compose(make_adder(i), compose(make_adder(1), make_adder(2)))
  total = total + f(i)
end

-- shared upvalue between sibling closures
local function make_account(balance)
  local function deposit(x) balance = balance + x end
  local function withdraw(x) balance = balance - x end
  local function get() return balance end
  return deposit, withdraw, get
end
local dep, wd, get = make_account(0)
for i = 1, N * 4 do
  dep(i % 7)
  wd(i % 3)
end
total = total + get()

-- deep upvalue access through nesting levels
local function outer()
  local a, b, c = 1, 2, 3
  return function()
    return function()
      return function(x)
        a = a + x
        return a + b + c
      end
    end
  end
end
local inner = outer()()()
for i = 1, N * 3 do total = total + inner(i % 5) % 11 end

print("total", total)
