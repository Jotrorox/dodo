-- Closures created in loops: each iteration of numeric for, generic for,
-- while/repeat with a local inside gets a fresh variable; a variable
-- declared outside the loop is shared; closing over the loop variable of
-- a loop exited with break; upvalues outliving loops.

local function results(fs)
  local out = {}
  for i = 1, #fs do out[i] = tostring(fs[i]()) end
  return table.concat(out, " ")
end

-- numeric for: fresh i per iteration
local fs = {}
for i = 1, 5 do fs[#fs + 1] = function() return i end end
print("numeric for", results(fs))
-- modifying the captured copy affects only that closure's variable
fs = {}
for i = 1, 3 do
  fs[#fs + 1] = function() i = i + 10; return i end
end
print("modify copy", results(fs), results(fs))

-- generic for: fresh k, v per iteration
fs = {}
for idx, v in ipairs({"a", "b", "c"}) do fs[#fs + 1] = function() return idx .. v end end
print("generic for", results(fs))

-- while with a local inside the body: fresh per iteration
fs = {}
local n = 0
while n < 4 do
  n = n + 1
  local copy = n
  fs[#fs + 1] = function() return copy end
end
print("while local", results(fs))
-- while capturing the outer variable: all share it
fs = {}
local m = 0
while m < 4 do
  m = m + 1
  fs[#fs + 1] = function() return m end
end
print("while shared", results(fs))

-- repeat: local in body, visible in until
fs = {}
local r = 0
repeat
  r = r + 1
  local mine = r * r
  fs[#fs + 1] = function() return mine end
until mine >= 16
print("repeat", results(fs))

-- closures sharing a per-iteration variable among themselves
local pairs_of = {}
for i = 1, 3 do
  local get = function() return i end
  local inc = function() i = i + 100 end
  pairs_of[i] = {get, inc}
end
pairs_of[2][2]()
print("per-iteration sharing", pairs_of[1][1](), pairs_of[2][1](), pairs_of[3][1]())

-- break keeps the captured variable alive
local saved
for i = 1, 10 do
  if i == 7 then saved = function() return i end; break end
end
print("after break", saved())

-- nested loops: each level captured separately
fs = {}
for i = 1, 2 do
  for j = 1, 2 do
    fs[#fs + 1] = function() return i * 10 + j end
  end
end
print("nested", results(fs))

-- closures created in a loop referencing a variable declared before the
-- loop and updated in it
fs = {}
local total = 0
for i = 1, 3 do
  total = total + i
  fs[#fs + 1] = function() return total end
end
print("outer accumulated", results(fs))

-- loop with goto continue still creates fresh locals
fs = {}
for i = 1, 6 do
  if i % 2 == 0 then goto continue end
  do
    local odd = i
    fs[#fs + 1] = function() return odd end
  end
  ::continue::
end
print("goto continue", results(fs))

-- float loop variables
fs = {}
for x = 0.5, 2.0, 0.5 do fs[#fs + 1] = function() return x end end
print("float for", results(fs))

-- closures over a generic-for control variable from a custom iterator
local function chars(s)
  return function(str, i)
    i = i + 1
    if i <= #str then return i, str:sub(i, i) end
  end, s, 0
end
fs = {}
for i, ch in chars("xyz") do fs[#fs + 1] = function() return i .. ch end end
print("custom iterator", results(fs))
