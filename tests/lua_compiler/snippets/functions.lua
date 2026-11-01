-- functions, methods, varargs, calls and returns
local obj = {}
function obj.f(a, b) return a + b end
function obj:m(x) return self, x end
function obj.inner.deep:method(...) return ... end
local function va(...)
  local a, b = ...
  local t = {...}
  local n = select('#', ...)
  return n, a, b, t, ...
end
local function tail(x) return va(x, x) end
local function paren(x) return (va(x)) end
local function multi() return 1, 2, va() end
local function noret() end
local function f3(a, b, c, ...) return a, b, c end
print(obj:m(1), obj.f(1, 2), va(1, 2, 3), tail(1), paren(2), multi())
print(f3(va()))
local s = obj:m "str"
local u = obj:m {1, 2}
local v = obj.f {3}
local w = print "x"
local x = ("abc"):upper()
local y = obj.inner.deep:method(1, 2, 3)
local function outer()
  local a = 1
  return function()
    local b = 2
    return function()
      a = a + b
      return a, b
    end
  end
end
local z = outer()()()
local function vararg_in_table(...) return {n = select('#', ...), ...} end
local function vararg_mid(...) return ..., 1 end
return z, vararg_in_table(1, nil, 3), vararg_mid(4)
