-- Float numeric for loops: fractional steps (printed with %.1f to avoid
-- accumulated-error noise), the loop variable becoming float when the
-- start or the step is a float, integer loops staying integer when only
-- the limit is a float, negative float steps, and infinite limits.

local function collect(a, b, c, fmt)
  local out = {}
  for x = a, b, c do out[#out + 1] = fmt and string.format(fmt, x) or (math.type(x) .. ":" .. tostring(x)) end
  return table.concat(out, " ")
end

print("0.1 steps", collect(0, 0.5, 0.1, "%.1f"))
print("count 0.1 steps", select(2, collect(0, 1, 0.1, "%.1f"):gsub("%S+", "")))
print("negative 0.25", collect(1, 0, -0.25, "%.2f"))
print("half steps", collect(0.5, 3, 0.5))
-- types: any float in start or step makes the loop float
print("float start", collect(1.0, 3, 1))
print("float step", collect(1, 3, 1.0))
print("float limit only", collect(1, 3.0, 1))
print("all ints", collect(1, 3, 1))
print("float limit fraction", collect(1, 2.9, 1))
-- float accumulation: computed by repeated addition of the step
local acc = {}
for x = 0, 0.3, 0.1 do acc[#acc + 1] = x end
print("float count", #acc)
-- large floats beyond 2^53 still terminate
local n = 0
for x = 2^53, 2^53 + 4, 2 do n = n + 1 end
print("big floats", n)
-- infinite limit with float loop and break
n = 0
for x = 1.0, math.huge do n = n + 1; if n == 5 then break end end
print("to inf", n)
n = 0
for x = -1.0, -math.huge, -1 do n = n + 1; if n == 3 then break end end
print("to -inf", n)
-- start beyond limit: no iterations
print("empty float", "[" .. collect(1.5, 1.0, 0.1) .. "]")
print("empty neg float", "[" .. collect(1.0, 1.5, -0.1) .. "]")
-- NaN bounds in a float loop: the reference's "skip loop" test is
-- "limit < init" (or "init < limit" for negative steps), which is false
-- for NaN, so the body runs once; the continue test "idx <= limit" then
-- fails. (Iterations are counted to avoid printing nan/-nan.)
local function iterations(a, b, c)
  local k = 0
  for _ = a, b, c do k = k + 1; if k > 10 then break end end
  return k
end
print("nan start", iterations(0/0, 1, 1))
print("nan limit", iterations(1.0, 0/0, 1))
print("nan limit int loop", iterations(1, 0/0, 1))
-- NaN step: not zero; 0 < NaN is false so it is treated as a negative
-- step, and init < limit skips the loop
print("nan step", iterations(1, 2, 0/0), iterations(2, 1, 0/0))
print("float zero step", pcall(collect, 1.0, 2.0, 0.0))
print("negative zero step", pcall(collect, 1, 2, -0.0))
-- exact single iteration
print("single", collect(2.5, 2.5, 1))
-- int start and float step: values are floats
print("int start float step", collect(0, 1, 0.5))
-- very small step
n = 0
for x = 0, 1e-9, 1e-10 do n = n + 1 end
print("tiny step count", n >= 10 and n <= 11)
-- variable is a fresh copy each iteration also for floats
local fs = {}
for x = 0.5, 1.5, 0.5 do fs[#fs + 1] = function() return x end end
print("closures", fs[1](), fs[2](), fs[3]())
