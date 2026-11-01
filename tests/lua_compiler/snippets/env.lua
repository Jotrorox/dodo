-- _ENV handling: globals through the upvalue, a local or a constant
x = 1
local y = x
print(y)
do
  local _ENV = {print = print}
  z = 2
  print(z)
end
local function f()
  local _ENV <const> = {w = 1}
  return w
end
local function g(_ENV)
  a = b
  return function() return a + _ENV.c end
end
_ENV.q = 3
local e = _ENV
function h() return e.x, _ENV end
return f, g, h
