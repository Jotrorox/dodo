-- Benchmark: n-body simulation of the Jovian planets (symplectic Euler).
-- Stresses float arithmetic, field access on small tables and math.sqrt.
-- Only +,-,*,/ and sqrt are used (all correctly rounded in IEEE 754), so the
-- printed energies are reproducible; they are still rounded to 9 digits.

local STEPS = 1000000

local PI = 3.141592653589793
local SOLAR_MASS = 4 * PI * PI
local DAYS_PER_YEAR = 365.24

local bodies = {
  { -- sun
    x = 0, y = 0, z = 0, vx = 0, vy = 0, vz = 0, mass = SOLAR_MASS },
  { -- jupiter
    x = 4.84143144246472090e+00, y = -1.16032004402742839e+00, z = -1.03622044471123109e-01,
    vx = 1.66007664274403694e-03 * DAYS_PER_YEAR, vy = 7.69901118419740425e-03 * DAYS_PER_YEAR,
    vz = -6.90460016972063023e-05 * DAYS_PER_YEAR, mass = 9.54791938424326609e-04 * SOLAR_MASS },
  { -- saturn
    x = 8.34336671824457987e+00, y = 4.12479856412430479e+00, z = -4.03523417114321381e-01,
    vx = -2.76742510726862411e-03 * DAYS_PER_YEAR, vy = 4.99852801234917238e-03 * DAYS_PER_YEAR,
    vz = 2.30417297573763929e-05 * DAYS_PER_YEAR, mass = 2.85885980666130812e-04 * SOLAR_MASS },
  { -- uranus
    x = 1.28943695621391310e+01, y = -1.51111514016986312e+01, z = -2.23307578892655734e-01,
    vx = 2.96460137564761618e-03 * DAYS_PER_YEAR, vy = 2.37847173959480950e-03 * DAYS_PER_YEAR,
    vz = -2.96589568540237556e-05 * DAYS_PER_YEAR, mass = 4.36624404335156298e-05 * SOLAR_MASS },
  { -- neptune
    x = 1.53796971148509165e+01, y = -2.59193146099879641e+01, z = 1.79258772950371181e-01,
    vx = 2.68067772490389322e-03 * DAYS_PER_YEAR, vy = 1.62824170038242295e-03 * DAYS_PER_YEAR,
    vz = -9.51592254519715870e-05 * DAYS_PER_YEAR, mass = 5.15138902046611451e-05 * SOLAR_MASS },
}

local function advance(nbody, dt)
  for i = 1, nbody do
    local bi = bodies[i]
    local bix, biy, biz, bimass = bi.x, bi.y, bi.z, bi.mass
    local bivx, bivy, bivz = bi.vx, bi.vy, bi.vz
    for j = i + 1, nbody do
      local bj = bodies[j]
      local dx, dy, dz = bix - bj.x, biy - bj.y, biz - bj.z
      local d2 = dx * dx + dy * dy + dz * dz
      local mag = dt / (d2 * math.sqrt(d2))
      local bm = bj.mass * mag
      bivx, bivy, bivz = bivx - dx * bm, bivy - dy * bm, bivz - dz * bm
      bm = bimass * mag
      bj.vx, bj.vy, bj.vz = bj.vx + dx * bm, bj.vy + dy * bm, bj.vz + dz * bm
    end
    bi.vx, bi.vy, bi.vz = bivx, bivy, bivz
    bi.x, bi.y, bi.z = bix + dt * bivx, biy + dt * bivy, biz + dt * bivz
  end
end

local function energy(nbody)
  local e = 0
  for i = 1, nbody do
    local bi = bodies[i]
    e = e + 0.5 * bi.mass * (bi.vx * bi.vx + bi.vy * bi.vy + bi.vz * bi.vz)
    for j = i + 1, nbody do
      local bj = bodies[j]
      local dx, dy, dz = bi.x - bj.x, bi.y - bj.y, bi.z - bj.z
      e = e - bi.mass * bj.mass / math.sqrt(dx * dx + dy * dy + dz * dz)
    end
  end
  return e
end

local function offset_momentum(nbody)
  local px, py, pz = 0, 0, 0
  for i = 1, nbody do
    local b = bodies[i]
    px, py, pz = px + b.vx * b.mass, py + b.vy * b.mass, pz + b.vz * b.mass
  end
  bodies[1].vx = -px / SOLAR_MASS
  bodies[1].vy = -py / SOLAR_MASS
  bodies[1].vz = -pz / SOLAR_MASS
end

local n = #bodies
offset_momentum(n)
print(string.format("%.9f", energy(n)))
for _ = 1, STEPS do advance(n, 0.01) end
print(string.format("%.9f", energy(n)))
