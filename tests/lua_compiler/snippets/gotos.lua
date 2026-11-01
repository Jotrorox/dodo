-- goto, labels, break and the scoping rules around them
for i = 1, 3 do
  for j = 1, 3 do
    if j == 2 then goto continue end
    if i == j then break end
    local x = i * j
    print(x)
    ::continue::
  end
end
do
  local k = 0
  ::top::
  k = k + 1
  local f = function() return k end
  if k < 3 then goto top end
end
do
  goto skip
  print("never")
  ::skip::
end
while true do
  local v = 1
  local function g() return v end
  if g() then break end
end
repeat
  local y = 5
  local function h() return y end
until h() > 3
repeat local z = 1 until z
local n = 0
::a:: n = n + 1
if n < 10 then goto a end
do
  local up = 1
  local fn = function() return up end
  do goto out end
  ::out::
end
for k2 in pairs({}) do
  if k2 then goto next end
  ::next::
end
