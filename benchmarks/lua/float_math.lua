-- Benchmark: float-heavy numerics. Simpson-rule integration of functions
-- using math.sin/math.exp/math.sqrt, a dense matrix multiplication, and a
-- Newton iteration. Results are printed with %.6f so that last-bit
-- differences between libm implementations do not affect the output.

local INTERVALS = 9000000
local MATRIX = 240

local function simpson(f, a, b, n)
  local h = (b - a) / n
  local s = f(a) + f(b)
  for i = 1, n - 1, 2 do s = s + 4 * f(a + i * h) end
  for i = 2, n - 2, 2 do s = s + 2 * f(a + i * h) end
  return s * h / 3
end

local sin, exp, sqrt = math.sin, math.exp, math.sqrt
local i1 = simpson(sin, 0, math.pi, INTERVALS)
local i2 = simpson(function(x) return exp(-x * x) end, -3, 3, INTERVALS)
local i3 = simpson(function(x) return sqrt(1 - x * x) end, -1, 1, INTERVALS)
print(string.format("integrals %.6f %.6f %.6f", i1, i2, i3))

-- matrix multiply C = A * B
local n = MATRIX
local A, B, C = {}, {}, {}
for i = 1, n do
  local ai, bi = {}, {}
  for j = 1, n do
    ai[j] = math.sin(i * 0.37 + j * 0.11)
    bi[j] = math.cos(i * 0.13 - j * 0.29)
  end
  A[i], B[i] = ai, bi
end
for i = 1, n do
  local ai, ci = A[i], {}
  for j = 1, n do
    local s = 0.0
    for k = 1, n do s = s + ai[k] * B[k][j] end
    ci[j] = s
  end
  C[i] = ci
end
local trace, frob = 0.0, 0.0
for i = 1, n do
  trace = trace + C[i][i]
  for j = 1, n do frob = frob + C[i][j] * C[i][j] end
end
print(string.format("matrix trace %.6f frobenius %.6f", trace, sqrt(frob)))

-- Newton's method for x^3 - 2x - 5 from many starting points
local roots = 0.0
for s = 1, 20000 do
  local x = s * 0.001 + 1
  for _ = 1, 30 do
    local fx = x * x * x - 2 * x - 5
    local dfx = 3 * x * x - 2
    x = x - fx / dfx
  end
  roots = roots + x
end
print(string.format("newton mean root %.6f", roots / 20000))
