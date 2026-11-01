-- Program test: a backtracking sudoku solver. Uses bitmasks (integer
-- bitwise ops) for row/column/box candidates, picks the most constrained
-- cell first, counts search nodes, validates the solved grid independently,
-- and checks uniqueness by searching for a second solution. Also exercises
-- string parsing of puzzles (gsub/gmatch), error handling for invalid
-- puzzles, and formatted grid printing.

local function parse(text)
  local cells = {}
  for ch in text:gmatch("[%d%.]") do
    cells[#cells + 1] = ch == "." and 0 or tonumber(ch)
  end
  if #cells ~= 81 then error("puzzle needs 81 cells, got " .. #cells, 0) end
  return cells
end

local function box_of(i)
  local r, c = (i - 1) // 9, (i - 1) % 9
  return (r // 3) * 3 + c // 3
end

local Solver = {}
Solver.__index = Solver

function Solver.new(cells)
  local s = setmetatable({ grid = {}, rows = {}, cols = {}, boxes = {}, nodes = 0 }, Solver)
  for k = 0, 8 do s.rows[k], s.cols[k], s.boxes[k] = 0, 0, 0 end
  for i = 1, 81 do
    local v = cells[i]
    s.grid[i] = 0
    if v ~= 0 then
      if not s:can_place(i, v) then
        error(string.format("conflict at row %d col %d (value %d)", (i - 1) // 9 + 1, (i - 1) % 9 + 1, v), 0)
      end
      s:place(i, v)
    end
  end
  return s
end

function Solver:can_place(i, v)
  local bit = 1 << v
  local r, c = (i - 1) // 9, (i - 1) % 9
  return (self.rows[r] | self.cols[c] | self.boxes[box_of(i)]) & bit == 0
end

function Solver:place(i, v)
  local bit = 1 << v
  local r, c = (i - 1) // 9, (i - 1) % 9
  self.grid[i] = v
  self.rows[r] = self.rows[r] | bit
  self.cols[c] = self.cols[c] | bit
  local b = box_of(i)
  self.boxes[b] = self.boxes[b] | bit
end

function Solver:unplace(i, v)
  local mask = ~(1 << v)
  local r, c = (i - 1) // 9, (i - 1) % 9
  self.grid[i] = 0
  self.rows[r] = self.rows[r] & mask
  self.cols[c] = self.cols[c] & mask
  local b = box_of(i)
  self.boxes[b] = self.boxes[b] & mask
end

local function popcount(x)
  local n = 0
  while x ~= 0 do
    x = x & (x - 1)
    n = n + 1
  end
  return n
end

-- returns the empty cell with fewest candidates and its candidate mask
function Solver:choose()
  local best, best_mask, best_count = nil, 0, 10
  for i = 1, 81 do
    if self.grid[i] == 0 then
      local r, c = (i - 1) // 9, (i - 1) % 9
      local used = self.rows[r] | self.cols[c] | self.boxes[box_of(i)]
      local free = ~used & 0x3FE
      local n = popcount(free)
      if n < best_count then
        best, best_mask, best_count = i, free, n
        if n <= 1 then break end
      end
    end
  end
  return best, best_mask
end

-- collects up to `limit` solutions
function Solver:search(solutions, limit)
  self.nodes = self.nodes + 1
  local i, mask = self:choose()
  if not i then
    local copy = {}
    for k = 1, 81 do copy[k] = self.grid[k] end
    solutions[#solutions + 1] = copy
    return #solutions >= limit
  end
  for v = 1, 9 do
    if mask & (1 << v) ~= 0 then
      self:place(i, v)
      if self:search(solutions, limit) then
        self:unplace(i, v)
        return true
      end
      self:unplace(i, v)
    end
  end
  return false
end

local function format_grid(g)
  local lines = {}
  for r = 0, 8 do
    if r > 0 and r % 3 == 0 then lines[#lines + 1] = "------+-------+------" end
    local row = {}
    for c = 0, 8 do
      if c > 0 and c % 3 == 0 then row[#row + 1] = "|" end
      local v = g[r * 9 + c + 1]
      row[#row + 1] = v == 0 and "." or tostring(v)
    end
    lines[#lines + 1] = table.concat(row, " ")
  end
  return table.concat(lines, "\n")
end

local function validate(g, original)
  for i = 1, 81 do
    if original[i] ~= 0 and original[i] ~= g[i] then return false, "clue changed at " .. i end
  end
  local function check(group, label)
    local seen = 0
    for _, idx in ipairs(group) do
      local v = g[idx]
      if v < 1 or v > 9 or seen & (1 << v) ~= 0 then return false end
      seen = seen | (1 << v)
    end
    return seen == 0x3FE
  end
  for k = 0, 8 do
    local row, col, box = {}, {}, {}
    for j = 0, 8 do
      row[#row + 1] = k * 9 + j + 1
      col[#col + 1] = j * 9 + k + 1
      local br, bc = (k // 3) * 3 + j // 3, (k % 3) * 3 + j % 3
      box[#box + 1] = br * 9 + bc + 1
    end
    if not check(row) then return false, "row " .. k end
    if not check(col) then return false, "col " .. k end
    if not check(box) then return false, "box " .. k end
  end
  return true
end

local puzzles = {
  { name = "easy", text = [[
    53..7.... 6..195... .98....6.
    8...6...3 4..8.3..1 7...2...6
    .6....28. ...419..5 ....8..79 ]] },
  { name = "hard", text = [[
    8........ ..36..... .7..9.2..
    .5...7... ....457.. ...1...3.
    ..1....68 ..85...1. .9....4.. ]] },
  { name = "multi", text = [[
    ......... ......... .........
    ...123... ...456... ...789...
    ......... ......... ......... ]] },
}

for _, p in ipairs(puzzles) do
  local cells = parse(p.text)
  local clues = 0
  for i = 1, 81 do if cells[i] ~= 0 then clues = clues + 1 end end
  print(string.format("== %s (%d clues)", p.name, clues))
  local s = Solver.new(cells)
  local solutions = {}
  s:search(solutions, 2)
  local sol = solutions[1]
  print(format_grid(sol))
  local ok, why = validate(sol, cells)
  print("valid:", ok, why or "", "unique:", #solutions == 1, "nodes:", s.nodes)
  if p.name ~= "multi" then
    local digits = {}
    for i = 1, 9 do digits[i] = sol[i] end
    print("first row:", table.concat(digits), "diag sum:", sol[1] + sol[11] + sol[21] + sol[31] + sol[41] + sol[51] + sol[61] + sol[71] + sol[81])
  end
end

-- invalid puzzles
local bad_conflict = "11" .. string.rep(".", 79)
print(pcall(function() return Solver.new(parse(bad_conflict)) end))
print(pcall(parse, "123"))
-- unsolvable but consistent clues: row 1 has 1-8 and the column of the last
-- cell already holds a 9
local unsolvable = "12345678." .. "........9" .. string.rep(".", 63)
local s = Solver.new(parse(unsolvable))
local sols = {}
s:search(sols, 1)
print("unsolvable solutions:", #sols)
