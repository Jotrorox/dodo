-- Benchmark: string concatenation in loops. Repeated s = s .. piece
-- (copying the growing string), multi-operand concatenation with numbers,
-- and short concatenations whose results are interned. Stresses OP_CONCAT,
-- number-to-string conversion and string creation.

local N = 2400

local total = 0
for r = 1, N do
  local s = ""
  for i = 1, 2000 do s = s .. "x" end
  local t = ""
  for i = 1, 300 do t = t .. i .. "," end
  local short = 0
  for i = 1, 3000 do
    local k = "id" .. (i % 50)
    if k == "id7" then short = short + 1 end
  end
  total = total + #s + #t + short
end
print(total)
