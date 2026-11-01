-- Upvalues: sibling closures share one variable, upvalues survive the
-- return of the creating function, independent instances per call,
-- counters/generators, recursion via local function, deep (3+ level)
-- nesting where intermediate functions do not use the variable, and
-- parameters captured as upvalues.

-- counters
local function counter(start)
  local n = start or 0
  return function() n = n + 1; return n end
end
local c1, c2 = counter(), counter(10)
print("counters", c1(), c1(), c2(), c1(), c2())

-- siblings share the same variable
local function account(balance)
  local function deposit(x) balance = balance + x; return balance end
  local function withdraw(x) balance = balance - x; return balance end
  local function get() return balance end
  return deposit, withdraw, get
end
local dep, wd, get = account(100)
dep(50); wd(30)
print("shared", get())
local dep2, _, get2 = account(0)
dep2(1)
print("independent", get(), get2())

-- the enclosing function can still modify the variable after creating the
-- closure, and the closure sees it (while the function is active)
local function late_binding()
  local x = 1
  local f = function() return x end
  x = 2
  local r1 = f()
  x = 3
  return r1, f()
end
print("late binding", late_binding())

-- upvalue modified by closure is seen by enclosing function
local function modify()
  local v = "before"
  local function set() v = "after" end
  set()
  return v
end
print("closure modifies", modify())

-- deep nesting: middle levels do not mention the variable
local function level1()
  local secret = "s1"
  local count = 0
  return function()          -- level 2 does not use 'secret'
    return function()        -- level 3 does not use it either
      return function()      -- level 4 uses it
        count = count + 1
        return secret .. ":" .. count
      end
    end
  end
end
local l2 = level1()
local l4a = l2()()
local l4b = l2()()
print("deep", l4a(), l4b(), l4a())

-- recursion through a local function (the name is in scope in its body)
local function fact(n) if n <= 1 then return 1 end return n * fact(n - 1) end
print("fact", fact(10), fact(20))
-- mutual recursion with forward-declared locals
local is_even, is_odd
function is_even(n) if n == 0 then return true end return is_odd(n - 1) end
function is_odd(n) if n == 0 then return false end return is_even(n - 1) end
print("mutual", is_even(10), is_odd(7), is_even(7))
-- rebinding the recursive name changes later calls
local function fib(n) if n < 2 then return n end return fib(n - 1) + fib(n - 2) end
local orig = fib
print("fib", fib(15))
fib = function() return 100 end
print("rebound", orig(5), orig(1))

-- parameters are captured like locals
local function adder(x) return function(y) return x + y end end
local add5, add10 = adder(5), adder(10)
print("adders", add5(1), add10(1), adder(-1)(1))

-- generator style closure
local function range(n)
  local i = 0
  return function()
    i = i + 1
    if i <= n then return i end
  end
end
local acc = {}
for v in range(5) do acc[#acc + 1] = v end
print("generator", table.concat(acc, ","))

-- memoization with an upvalue cache
local function memoize(f)
  local cache, calls = {}, 0
  return function(n)
    if cache[n] == nil then calls = calls + 1; cache[n] = f(n) end
    return cache[n]
  end, function() return calls end
end
local sq, sqcalls = memoize(function(n) return n * n end)
sq(4); sq(4); sq(5); sq(4)
print("memo", sq(5), sqcalls())

-- closures stored in tables keep their upvalues
local objs = {}
for i = 1, 3 do
  local name = "obj" .. i
  objs[i] = {get = function() return name end, set = function(v) name = v end}
end
objs[2].set("changed")
print("in tables", objs[1].get(), objs[2].get(), objs[3].get())

-- upvalue that is itself a function updated later
local helper = function() return "old" end
local function uses_helper() return helper() end
helper = function() return "new" end
print("upvalue function", uses_helper())

-- many upvalues in one closure
local u1, u2, u3, u4, u5, u6, u7, u8 = 1, 2, 3, 4, 5, 6, 7, 8
local function many() return u1 + u2 + u3 + u4 + u5 + u6 + u7 + u8 end
u8 = 80
print("many upvalues", many())
