-- Numeric for loops: positive/negative steps, empty ranges, single
-- iterations, loops near math.maxinteger/mininteger that must not overflow
-- or wrap (5.4 precomputes the iteration count), the loop variable being a
-- fresh local copy, and the errors for zero steps and non-numeric bounds.

local function collect(a, b, c)
  local out = {}
  if c == nil then
    for i = a, b do out[#out + 1] = tostring(i) end
  else
    for i = a, b, c do out[#out + 1] = tostring(i) end
  end
  return "[" .. table.concat(out, " ") .. "]"
end

print("1..5", collect(1, 5))
print("5..1", collect(5, 1))
print("5..1,-1", collect(5, 1, -1))
print("1..10,3", collect(1, 10, 3))
print("10..1,-3", collect(10, 1, -3))
print("1..1", collect(1, 1))
print("1..0", collect(1, 0))
print("0..-1,-1", collect(0, -1, -1))
print("-3..3,2", collect(-3, 3, 2))
print("big step", collect(1, 5, 100))
print("big neg step", collect(1, -5, -100))

-- near the integer limits: no overflow, no infinite loop
local mx, mn = math.maxinteger, math.mininteger
print("to maxint", collect(mx - 2, mx))
print("to minint", collect(mn + 2, mn, -1))
print("maxint step", collect(0, mx, mx))
print("minint step", collect(0, mn, mn))
print("maxint..maxint", collect(mx, mx))
print("minint..minint", collect(mn, mn))
print("large step past end", collect(mx - 5, mx, 4))
print("neg large step", collect(mn + 5, mn, -4))
print("huge step from minint", collect(mn, mx, mx))
local cnt = 0
for i = mx - 1, mx do cnt = cnt + 1 end
print("count at max", cnt)
cnt = 0
for i = mn, mn + 2 do cnt = cnt + 1 end
print("count at min", cnt)
-- float limit with integer start: clipped to integers
print("float limit", collect(1, 3.5))
print("float limit neg", collect(3, 0.5, -1))
print("limit huge float", collect(mx - 1, 1e100))
print("limit -huge", collect(mn + 1, -1e100, -1))
print("limit inf", collect(mx - 1, math.huge))
print("limit below start", collect(1, -math.huge))
print("limit nan", collect(1, 0/0))
print("integer start, float limit types", collect(1, 2.0))

-- modifying the loop variable does not affect iteration
local seen = {}
for i = 1, 3 do
  seen[#seen + 1] = i
  i = i * 10
  seen[#seen + 1] = i
end
print("modified var", table.concat(seen, " "))
-- the bounds are evaluated once
local limit = 3
local iters = 0
for i = 1, limit do limit = 10; iters = iters + 1 end
print("bounds once", iters)
local evals = 0
local function bound() evals = evals + 1; return 4 end
for _ = 1, bound() do end
print("bound evaluated", evals)
-- loop variable is local to the loop
local i = "outer"
for i = 1, 2 do end
print("var scope", i)

-- errors
local function try(label, src)
  local ok, err = pcall(load(src, "=for"))
  print(label, ok, err)
end
try("step zero", "for i = 1, 10, 0 do end")
try("step zero float", "for i = 1.0, 10, 0.0 do end")
try("init string", "for i = 'a', 2 do end")
-- numeric strings are coerced (the loop then runs with float values)
try("init numeric string", "local r = {} for i = '1', 2 do r[#r + 1] = i end return table.concat(r, ',')")
try("limit string", "for i = 1, 'x' do end")
try("step string", "local r = {} for i = 1, 2, '1' do r[#r + 1] = i end return table.concat(r, ',')")
try("limit numeric string", "local r = {} for i = 1, '2' do r[#r + 1] = i end return table.concat(r, ',')")
try("init nil", "local x; for i = x, 2 do end")
try("limit table", "for i = 1, {} do end")
try("step bool", "for i = 1, 2, true do end")
-- zero step is checked even when the loop would not run
try("step zero empty", "for i = 2, 1, 0 do end")
