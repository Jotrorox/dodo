-- Program test: Conway's Game of Life. Two implementations that must agree:
-- a dense 2-D array on a torus and a sparse set keyed by packed integer
-- coordinates on an unbounded plane. Runs a glider, a blinker, a pulsar and
-- an R-pentomino for N generations, prints boards, populations, bounding
-- boxes, and detects period of oscillators via string snapshots in a table.

local function parse(pattern)
  local cells = {}
  local r = 0
  for line in pattern:gmatch("[^\n]+") do
    for c = 1, #line do
      if line:sub(c, c) == "O" then cells[#cells + 1] = {r, c - 1} end
    end
    r = r + 1
  end
  return cells
end

-- dense torus ------------------------------------------------------------

local Dense = {}
Dense.__index = Dense

function Dense.new(h, w)
  local g = {}
  for r = 0, h - 1 do
    local row = {}
    for c = 0, w - 1 do row[c] = 0 end
    g[r] = row
  end
  return setmetatable({h = h, w = w, g = g, gen = 0}, Dense)
end

function Dense:set(r, c) self.g[r % self.h][c % self.w] = 1 end

function Dense:step()
  local h, w, g = self.h, self.w, self.g
  local ng = {}
  for r = 0, h - 1 do
    local up, row, down = g[(r - 1) % h], g[r], g[(r + 1) % h]
    local nrow = {}
    for c = 0, w - 1 do
      local l, rr = (c - 1) % w, (c + 1) % w
      local n = up[l] + up[c] + up[rr] + row[l] + row[rr] + down[l] + down[c] + down[rr]
      nrow[c] = (n == 3 or (n == 2 and row[c] == 1)) and 1 or 0
    end
    ng[r] = nrow
  end
  self.g = ng
  self.gen = self.gen + 1
end

function Dense:population()
  local n = 0
  for r = 0, self.h - 1 do
    for c = 0, self.w - 1 do n = n + self.g[r][c] end
  end
  return n
end

function Dense:render()
  local lines = {}
  for r = 0, self.h - 1 do
    local row = {}
    for c = 0, self.w - 1 do row[c + 1] = self.g[r][c] == 1 and "#" or "." end
    lines[#lines + 1] = table.concat(row)
  end
  return table.concat(lines, "\n")
end

-- sparse unbounded --------------------------------------------------------

local OFFSET = 1 << 20
local function pack(r, c) return (r + OFFSET) * (1 << 21) + (c + OFFSET) end
local function unpack_rc(k) return k // (1 << 21) - OFFSET, k % (1 << 21) - OFFSET end

local Sparse = {}
Sparse.__index = Sparse

function Sparse.new() return setmetatable({live = {}, gen = 0}, Sparse) end
function Sparse:set(r, c) self.live[pack(r, c)] = true end

function Sparse:step()
  local counts = {}
  for k in pairs(self.live) do
    local r, c = unpack_rc(k)
    for dr = -1, 1 do
      for dc = -1, 1 do
        if dr ~= 0 or dc ~= 0 then
          local nk = pack(r + dr, c + dc)
          counts[nk] = (counts[nk] or 0) + 1
        end
      end
    end
  end
  local nl = {}
  for k, n in pairs(counts) do
    if n == 3 or (n == 2 and self.live[k]) then nl[k] = true end
  end
  self.live = nl
  self.gen = self.gen + 1
end

function Sparse:population()
  local n = 0
  for _ in pairs(self.live) do n = n + 1 end
  return n
end

function Sparse:bbox()
  local minr, maxr, minc, maxc = math.huge, -math.huge, math.huge, -math.huge
  for k in pairs(self.live) do
    local r, c = unpack_rc(k)
    minr, maxr = math.min(minr, r), math.max(maxr, r)
    minc, maxc = math.min(minc, c), math.max(maxc, c)
  end
  if minr == math.huge then return "empty" end
  return string.format("rows %d..%d cols %d..%d", minr, maxr, minc, maxc)
end

-- canonical snapshot: sorted packed keys
function Sparse:snapshot()
  local keys = {}
  for k in pairs(self.live) do keys[#keys + 1] = k end
  table.sort(keys)
  return table.concat(keys, ",")
end

-- patterns -----------------------------------------------------------------

local glider = parse(".O.\n..O\nOOO")
local pulsar = parse([[
..OOO...OOO..
.............
O....O.O....O
O....O.O....O
O....O.O....O
..OOO...OOO..
.............
..OOO...OOO..
O....O.O....O
O....O.O....O
O....O.O....O
.............
..OOO...OOO..]])
local rpent = parse(".OO\nOO.\n.O.")

-- glider on a small torus: after 4*8 generations it returns to start
local d = Dense.new(8, 8)
for _, rc in ipairs(glider) do d:set(rc[1], rc[2]) end
local start = d:render()
print("glider gen 0:\n" .. start)
for _ = 1, 4 do d:step() end
print("glider gen 4 (pop " .. d:population() .. "):\n" .. d:render())
for _ = 5, 32 do d:step() end
print("glider back at start after 32:", d:render() == start)

-- dense and sparse agree on a glider while it does not wrap
local d2, s2 = Dense.new(20, 20), Sparse.new()
for _, rc in ipairs(glider) do d2:set(rc[1] + 2, rc[2] + 2); s2:set(rc[1] + 2, rc[2] + 2) end
local agree = true
for _ = 1, 30 do
  d2:step(); s2:step()
  local s_from_dense = Sparse.new()
  for r = 0, 19 do for c = 0, 19 do if d2.g[r][c] == 1 then s_from_dense:set(r, c) end end end
  if s_from_dense:snapshot() ~= s2:snapshot() then agree = false end
end
print("dense/sparse agree for 30 gens:", agree, s2:bbox())

-- oscillator period detection
local function period(cells, max_gen)
  local s = Sparse.new()
  for _, rc in ipairs(cells) do s:set(rc[1], rc[2]) end
  local seen = {[s:snapshot()] = 0}
  for g = 1, max_gen do
    s:step()
    local snap = s:snapshot()
    if seen[snap] then return g - seen[snap], seen[snap] end
    seen[snap] = g
  end
  return nil
end
print("blinker period:", period(parse("OOO"), 10))
print("pulsar period:", period(pulsar, 10))
print("block period:", period(parse("OO\nOO"), 10))
print("glider period (none, it moves):", period(glider, 10))

-- pulsar rendered after one step on a dense board
local dp = Dense.new(17, 17)
for _, rc in ipairs(pulsar) do dp:set(rc[1] + 2, rc[2] + 2) end
dp:step()
print("pulsar gen 1 (pop " .. dp:population() .. "):\n" .. dp:render())

-- R-pentomino on the unbounded plane
local rp = Sparse.new()
for _, rc in ipairs(rpent) do rp:set(rc[1], rc[2]) end
local pops = {}
for g = 1, 200 do
  rp:step()
  if g % 25 == 0 then pops[#pops + 1] = g .. ":" .. rp:population() end
end
print("r-pentomino populations:", table.concat(pops, " "))
print("r-pentomino bbox at 200:", rp:bbox())
local snap = rp:snapshot()
local h = 0
for i = 1, #snap do h = (h * 31 + snap:byte(i)) % 1000000007 end
print("r-pentomino snapshot hash:", h, #snap)

-- die-out: a lone pair dies immediately
local lone = Sparse.new()
lone:set(0, 0); lone:set(0, 1)
lone:step()
print("lone pair:", lone:population(), lone:bbox())
