-- goto and labels: forward and backward jumps, the continue idiom,
-- breaking out of nested loops, labels at the end of blocks (allowed even
-- after local declarations), label visibility in nested blocks and
-- functions, loops built from goto, and closures inside goto loops.

-- forward jump skipping code
do
  local s = "a"
  goto skip
  s = s .. "b"
  ::skip::
  s = s .. "c"
  print("forward", s)
end

-- backward jump as a loop
do
  local i, out = 1, {}
  ::top::
  out[#out + 1] = i
  i = i + 1
  if i <= 5 then goto top end
  print("backward", table.concat(out, ","))
end

-- continue idiom in numeric, generic and while loops
local odds = {}
for i = 1, 10 do
  if i % 2 == 0 then goto continue end
  odds[#odds + 1] = i
  ::continue::
end
print("continue for", table.concat(odds, ","))
local kept = {}
for _, w in ipairs({"keep", "skip", "keep2", "skip"}) do
  if w == "skip" then goto next_word end
  kept[#kept + 1] = w
  ::next_word::
end
print("continue generic", table.concat(kept, ","))
local n, evens = 0, {}
while n < 8 do
  n = n + 1
  if n % 2 == 1 then goto cont end
  evens[#evens + 1] = n
  ::cont::
end
print("continue while", table.concat(evens, ","))
-- continue in repeat: the label must come before locals used in 'until'
local r, rs = 0, {}
repeat
  r = r + 1
  if r == 2 then goto rcont end
  rs[#rs + 1] = r
  ::rcont::
until r >= 4
print("continue repeat", table.concat(rs, ","))

-- breaking out of nested loops
local found
for i = 1, 5 do
  for j = 1, 5 do
    if i * j == 12 then found = i .. "x" .. j; goto done end
  end
end
::done::
print("nested break", found)

-- label at the end of a block after a local declaration is allowed
do
  local x = 1
  if x == 1 then goto finish end
  local y = 2
  print("not reached", y)
  ::finish::
end
print("end label ok")

-- same label name in different (non-nested) blocks
do goto l1; print("skipped A"); ::l1:: end
do goto l1; print("skipped B"); ::l1:: end
print("reused names ok")
-- nested block can jump to a label in an enclosing block
do
  local trace = {}
  do
    do
      trace[#trace + 1] = "inner"
      goto outer_label
    end
  end
  trace[#trace + 1] = "skipped"
  ::outer_label::
  trace[#trace + 1] = "after"
  print("enclosing label", table.concat(trace, ","))
end

-- closures inside a goto loop capture a fresh local each pass
do
  local fs, i = {}, 1
  ::again::
  local captured = i
  fs[#fs + 1] = function() return captured end
  i = i + 1
  if i <= 3 then goto again end
  print("goto closures", fs[1](), fs[2](), fs[3]())
end

-- goto in a function: labels are per function
local function find(t, target)
  for i, v in ipairs(t) do
    if v == target then goto found end
  end
  do return "missing" end
  ::found::
  return "present"
end
print("function labels", find({1, 2, 3}, 2), find({1, 2, 3}, 9))

-- state machine with gotos
local function machine(input)
  local i, out = 0, {}
  ::start::
  i = i + 1
  local c = input:sub(i, i)
  if c == "" then goto stop end
  if c == "a" then out[#out + 1] = "A"; goto start end
  out[#out + 1] = "?"
  goto start
  ::stop::
  return table.concat(out)
end
print("machine", machine("abca"))

-- 'goto' and labels named like keywords are not allowed, but 'continue'
-- is an ordinary name in Lua
local continue = "ordinary"
print("continue is a name", continue)
