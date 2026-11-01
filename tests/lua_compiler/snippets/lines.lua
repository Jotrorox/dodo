-- line information across multi-line constructs
local function f(
  a,
  b
)
  return a +
    b
end
local t = {
  1,
  2,
  x = f(
    1,
    2
  ),
}
print(
  f(1, 2),
  t.x
)
local s = "a" ..
  "b" ..
  f(3,
    4)
if t
  and t.x
then
  print(t)
end
function t.method(self)
  return self
end
