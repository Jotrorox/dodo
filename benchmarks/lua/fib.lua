-- Benchmark: naive doubly-recursive Fibonacci. Stresses Lua function calls,
-- argument passing, returns, integer comparison and addition.

local N = 38

local function fib(n)
  if n < 2 then return n end
  return fib(n - 1) + fib(n - 2)
end

print(string.format("fib(%d) = %d", N, fib(N)))
