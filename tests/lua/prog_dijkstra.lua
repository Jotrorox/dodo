-- Program test: a binary-heap priority queue and graph algorithms on top of
-- it. Dijkstra shortest paths with path reconstruction on a small named
-- graph, a heap-sort cross-check against table.sort on LCG data, A* on a
-- grid maze parsed from a string, BFS, and topological sort with cycle
-- detection. Ties are broken deterministically by an insertion counter so
-- the output never depends on heap implementation details.

local Heap = {}
Heap.__index = Heap

function Heap.new(less)
  return setmetatable({ items = {}, n = 0, less = less or function(a, b) return a < b end }, Heap)
end

function Heap:push(v)
  self.n = self.n + 1
  local items, i = self.items, self.n
  items[i] = v
  while i > 1 do
    local parent = i // 2
    if self.less(items[i], items[parent]) then
      items[i], items[parent] = items[parent], items[i]
      i = parent
    else
      break
    end
  end
end

function Heap:pop()
  if self.n == 0 then return nil end
  local items = self.items
  local top = items[1]
  items[1] = items[self.n]
  items[self.n] = nil
  self.n = self.n - 1
  local i = 1
  while true do
    local l, r, smallest = 2 * i, 2 * i + 1, i
    if l <= self.n and self.less(items[l], items[smallest]) then smallest = l end
    if r <= self.n and self.less(items[r], items[smallest]) then smallest = r end
    if smallest == i then break end
    items[i], items[smallest] = items[smallest], items[i]
    i = smallest
  end
  return top
end

function Heap:__len() return self.n end

-- heap sort cross-check --------------------------------------------------

local seed = 2024
local function rand(n)
  seed = (seed * 6364136223846793005 + 1442695040888963407) -- wraps around (64-bit)
  return ((seed >> 33) % n) + 1
end

local data, h = {}, Heap.new()
for i = 1, 500 do
  data[i] = rand(1000)
  h:push(data[i])
end
local sorted = {}
while #h > 0 do sorted[#sorted + 1] = h:pop() end
table.sort(data)
local same = #sorted == #data
for i = 1, #data do if sorted[i] ~= data[i] then same = false end end
print("heapsort matches table.sort:", same, #sorted, sorted[1], sorted[250], sorted[500])

local maxh = Heap.new(function(a, b) return a > b end)
for _, v in ipairs({5, 1, 9, 3, 7, 9, 2}) do maxh:push(v) end
local order = {}
while #maxh > 0 do order[#order + 1] = maxh:pop() end
print("max-heap order:", table.concat(order, " "))

-- Dijkstra -----------------------------------------------------------------

local edges = {
  {"A", "B", 7}, {"A", "C", 9}, {"A", "F", 14}, {"B", "C", 10}, {"B", "D", 15},
  {"C", "D", 11}, {"C", "F", 2}, {"D", "E", 6}, {"E", "F", 9}, {"G", "H", 1},
  {"E", "I", 3}, {"I", "J", 2}, {"D", "J", 4},
}
local graph = {}
local function add_edge(a, b, w)
  graph[a] = graph[a] or {}
  graph[b] = graph[b] or {}
  table.insert(graph[a], {to = b, w = w})
  table.insert(graph[b], {to = a, w = w})
end
for _, e in ipairs(edges) do add_edge(e[1], e[2], e[3]) end

local function dijkstra(src)
  local dist, prev, counter = {[src] = 0}, {}, 0
  local pq = Heap.new(function(a, b)
    if a[1] ~= b[1] then return a[1] < b[1] end
    return a[3] < b[3]
  end)
  pq:push({0, src, 0})
  local visited, order = {}, {}
  while #pq > 0 do
    local d, u = table.unpack(pq:pop())
    if not visited[u] then
      visited[u] = true
      order[#order + 1] = u
      for _, e in ipairs(graph[u]) do
        local nd = d + e.w
        if dist[e.to] == nil or nd < dist[e.to] then
          dist[e.to], prev[e.to] = nd, u
          counter = counter + 1
          pq:push({nd, e.to, counter})
        end
      end
    end
  end
  return dist, prev, order
end

local function path_to(prev, target)
  local p = {}
  while target do
    table.insert(p, 1, target)
    target = prev[target]
  end
  return table.concat(p, "->")
end

local dist, prev, visit_order = dijkstra("A")
print("visit order:", table.concat(visit_order, " "))
local nodes = {}
for k in pairs(graph) do nodes[#nodes + 1] = k end
table.sort(nodes)
for _, n in ipairs(nodes) do
  if dist[n] then
    print(string.format("  %s dist=%-3d path=%s", n, dist[n], path_to(prev, n)))
  else
    print(string.format("  %s unreachable", n))
  end
end

-- A* on a grid maze --------------------------------------------------------

local maze_text = [[
S...#......
.##.#.####.
.#..#....#.
.#.###.#.#.
.#.....#...
.#####.###.
...#...#.#E
.#...#.....]]

local maze, start, goal = {}, nil, nil
local r = 0
for line in maze_text:gmatch("[^\n]+") do
  r = r + 1
  maze[r] = {}
  for c = 1, #line do
    local ch = line:sub(c, c)
    maze[r][c] = ch
    if ch == "S" then start = {r, c} elseif ch == "E" then goal = {r, c} end
  end
end
local rows, cols = #maze, #maze[1]

local function astar()
  local function key(rr, cc) return rr * 100 + cc end
  local function hdist(rr, cc) return math.abs(rr - goal[1]) + math.abs(cc - goal[2]) end
  local g, came = {[key(start[1], start[2])] = 0}, {}
  local counter = 0
  local open = Heap.new(function(a, b)
    if a.f ~= b.f then return a.f < b.f end
    return a.id < b.id
  end)
  open:push({f = hdist(start[1], start[2]), r = start[1], c = start[2], id = 0})
  local expanded = 0
  local closed = {}
  while #open > 0 do
    local cur = open:pop()
    local ck = key(cur.r, cur.c)
    if not closed[ck] then
      closed[ck] = true
      expanded = expanded + 1
      if cur.r == goal[1] and cur.c == goal[2] then return g[ck], came, expanded end
      for _, d in ipairs({{-1, 0}, {1, 0}, {0, -1}, {0, 1}}) do
        local nr, nc = cur.r + d[1], cur.c + d[2]
        if nr >= 1 and nr <= rows and nc >= 1 and nc <= cols and maze[nr][nc] ~= "#" then
          local nk = key(nr, nc)
          local ng = g[ck] + 1
          if g[nk] == nil or ng < g[nk] then
            g[nk], came[nk] = ng, ck
            counter = counter + 1
            open:push({f = ng + hdist(nr, nc), r = nr, c = nc, id = counter})
          end
        end
      end
    end
  end
  return nil, came, expanded
end

local cost, came = astar()
print("A* cost:", cost)
local k = goal[1] * 100 + goal[2]
while came[k] do
  local rr, cc = k // 100, k % 100
  if maze[rr][cc] == "." then maze[rr][cc] = "*" end
  k = came[k]
end
for i = 1, rows do print("  " .. table.concat(maze[i])) end

-- BFS hop counts --------------------------------------------------------------

local function bfs(src)
  local hops, queue, head = {[src] = 0}, {src}, 1
  while head <= #queue do
    local u = queue[head]; head = head + 1
    for _, e in ipairs(graph[u]) do
      if hops[e.to] == nil then
        hops[e.to] = hops[u] + 1
        queue[#queue + 1] = e.to
      end
    end
  end
  return hops, queue
end
local hops, q = bfs("A")
print("bfs order:", table.concat(q, " "))
local hs = {}
for _, n in ipairs(nodes) do hs[#hs + 1] = n .. "=" .. tostring(hops[n]) end
print("hops:", table.concat(hs, " "))

-- topological sort (Kahn) ---------------------------------------------------------

local function toposort(deps)
  local indeg, out, names = {}, {}, {}
  for n, ds in pairs(deps) do
    indeg[n] = indeg[n] or 0
    names[#names + 1] = n
    for _, d in ipairs(ds) do
      indeg[d] = indeg[d] or 0
      out[d] = out[d] or {}
      table.insert(out[d], n)
      indeg[n] = indeg[n] + 1
    end
  end
  local ready = Heap.new()
  for n, c in pairs(indeg) do if c == 0 then ready:push(n) end end
  local result = {}
  while #ready > 0 do
    local n = ready:pop()
    result[#result + 1] = n
    for _, m in ipairs(out[n] or {}) do
      indeg[m] = indeg[m] - 1
      if indeg[m] == 0 then ready:push(m) end
    end
  end
  local count = 0
  for _ in pairs(indeg) do count = count + 1 end
  if #result ~= count then return nil, "cycle detected among " .. (count - #result) .. " nodes" end
  return result
end

print("build order:", table.concat(toposort({
  app = {"lib", "ui", "net"}, ui = {"lib", "gfx"}, net = {"lib"}, gfx = {"math"},
  lib = {"math"}, tests = {"app"}, math = {},
}), " "))
print("cyclic:", toposort({a = {"b"}, b = {"c"}, c = {"a"}, d = {}}))
