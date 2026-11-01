-- Benchmark: Mandelbrot set membership on a SIZE x SIZE grid (50 iterations,
-- escape radius 2). Stresses float multiply/add in a tight inner loop and
-- integer bit operations to pack pixels. Prints a checksum over the packed
-- bitmap instead of the image itself.

local SIZE = 1600
local ITER = 50

local checksum, inside = 0, 0
for y = 0, SIZE - 1 do
  local ci = 2.0 * y / SIZE - 1.0
  local bits, nbits = 0, 0
  for x = 0, SIZE - 1 do
    local cr = 2.0 * x / SIZE - 1.5
    local zr, zi, tr, ti = 0.0, 0.0, 0.0, 0.0
    local i = 0
    while i < ITER and tr + ti <= 4.0 do
      zi = 2.0 * zr * zi + ci
      zr = tr - ti + cr
      tr = zr * zr
      ti = zi * zi
      i = i + 1
    end
    bits = bits << 1
    if tr + ti <= 4.0 then
      bits = bits | 1
      inside = inside + 1
    end
    nbits = nbits + 1
    if nbits == 8 or x == SIZE - 1 then
      checksum = (checksum * 31 + bits) % 4294967291
      bits, nbits = 0, 0
    end
  end
end
print(string.format("size %d: %d points inside, checksum %d", SIZE, inside, checksum))
