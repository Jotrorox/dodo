-- Benchmark: binary-trees. Allocates and walks many short-lived complete
-- binary trees plus one long-lived tree. Stresses table allocation, the
-- garbage collector and recursive traversal.

local MAX_DEPTH = 16

local function make(depth)
  if depth == 0 then return {} end
  depth = depth - 1
  return { make(depth), make(depth) }
end

local function check(tree)
  local left = tree[1]
  if left then return 1 + check(left) + check(tree[2]) end
  return 1
end

local min_depth = 4
local max_depth = math.max(min_depth + 2, MAX_DEPTH)

local stretch = max_depth + 1
print(string.format("stretch tree of depth %d\t check: %d", stretch, check(make(stretch))))

local long_lived = make(max_depth)
local total = 0
for depth = min_depth, max_depth, 2 do
  local iterations = 1 << (max_depth - depth + min_depth)
  local sum = 0
  for _ = 1, iterations do sum = sum + check(make(depth)) end
  total = total + sum
end
print(string.format("long lived tree of depth %d\t check: %d", max_depth, check(long_lived)))
print(string.format("total checks: %d", total))
