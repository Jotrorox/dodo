-- Benchmark: string building. Repeated `..` in loops (quadratic copying on
-- purpose, kept small), multi-operand concatenation, table.concat of many
-- pieces, string.rep with separators, and tostring of numbers. Stresses
-- string allocation, interning of short strings and the garbage collector.

local ROUNDS = 160

local function checksum(s, acc)
  -- cheap rolling hash over a sample of bytes
  local step = #s // 64 + 1
  for i = 1, #s, step do acc = (acc * 131 + s:byte(i)) % 1000000007 end
  return (acc + #s) % 1000000007
end

local acc = 0
for round = 1, ROUNDS do
  -- 1) naive accumulation with ..
  local s = ""
  for i = 1, 2000 do s = s .. (i % 10) end
  acc = checksum(s, acc)

  -- 2) multi-operand concatenation of short pieces (a single OP_CONCAT)
  local parts = {}
  for i = 1, 20000 do
    parts[i] = "<" .. i .. ":" .. (i * round) % 97 .. "/" .. "x" .. ">"
  end
  -- 3) table.concat with and without separator
  local joined = table.concat(parts)
  local joined2 = table.concat(parts, ", ", 100, 15000)
  acc = checksum(joined, acc)
  acc = checksum(joined2, acc)

  -- 4) string.rep, with separator
  local r1 = string.rep("abc", 10000 + round)
  local r2 = string.rep("xy", 5000, "--")
  acc = checksum(r1, acc)
  acc = checksum(r2, acc)

  -- 5) build lines and join them, a common output pattern
  local lines = {}
  for i = 1, 5000 do
    lines[#lines + 1] = "line " .. i .. " of round " .. round .. ": " .. string.rep("=", i % 13)
  end
  acc = checksum(table.concat(lines, "\n"), acc)
end
print("checksum", acc)
