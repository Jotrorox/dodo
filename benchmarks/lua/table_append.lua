-- Benchmark: array growth and length. Appending with t[#t+1] = v,
-- reading #t in loop conditions, removing from the end with t[#t] = nil,
-- and arrays that grow from empty. Stresses OP_LEN (the border search),
-- SETTABLE/SETI with integer keys and array-part resizing.

local N = 7000

local total = 0
for round = 1, N do
  local t = {}
  for i = 1, 4000 do t[#t + 1] = i end
  local s = 0
  local i = 1
  while i <= #t do s = s + t[i]; i = i + 1 end
  while #t > 2000 do t[#t] = nil end
  for j = 1, 1000 do t[#t + 1] = j * 2 end
  total = total + s + #t
end
print(total)
