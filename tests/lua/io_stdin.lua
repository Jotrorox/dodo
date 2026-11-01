--! stdin: fixtures/stdin_mixed.txt
-- Reading standard input (fed from fixtures/stdin_mixed.txt): io.read
-- with several "n" formats, a failing "n" in a multi-format call,
-- io.read("l"/"L"), io.lines() over the rest of stdin, and reads at EOF.

print(io.input() == io.stdin, io.type(io.stdin))
local a, b = io.read("n", "n")
print(math.type(a), a, math.type(b), b)
local c, d = io.read("n", "n")
print(math.type(c), c, d)
-- the failed numeral consumed only the leading space; read the rest of the line
print(string.format("%q", io.read("L")))
print(string.format("%q", io.stdin:read("l")))
local n = 0
for line in io.lines() do
  n = n + 1
  print(n, string.format("%q", line))
end
print("at EOF:", io.read("l"), io.read("n"), string.format("%q", io.read("a")), io.read(0))
print(io.type(io.stdin))
