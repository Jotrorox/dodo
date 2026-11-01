-- multiple assignment, evaluation order and conflicts
local a, b, c = 1, 2, 3
local t = {}
a, b = b, a
a, b, c = c
a, b = f()
a, b, c = 1, f()
a = 1, 2, 3
t[a], a = 1, 2
a, t[a] = 1, 2
t.x, t = 1, 2
t, t.x = 1, 2
t[a], t[b], a, b = a, b, 3, 4
local i = 1
i, t[i] = i + 1, 20
t[i], i = 20, i + 1
local u = {}
local function up()
  u, u.x = 1, 2
  u.x, u = 1, 2
  u[a], a = 1, 2
end
g1, g2 = g2, g1
g1.x, g1 = 1, 2
a, b, c = ...
t[1], t[2], t[3] = ...
