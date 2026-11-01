-- Benchmark: method calls with variable arguments. Methods taking `...`,
-- forwarding them to other varargs functions, select('#', ...) and
-- select(n, ...), packing into tables and table.unpack. Stresses VARARGPREP,
-- VARARG, OP_SELF and calls with a variable number of results.

local N = 2000000

local Acc = {}
Acc.__index = Acc
function Acc.new() return setmetatable({total = 0, calls = 0}, Acc) end
function Acc:add(...)
  local n = select('#', ...)
  local s = 0
  for i = 1, n do s = s + (select(i, ...)) end
  self.total = self.total + s
  self.calls = self.calls + 1
  return self
end
function Acc:forward(...) return self:add(...) end
function Acc:pack(...)
  local t = {...}
  self.total = self.total + #t
  return ...
end
local function sum3(a, b, c) return a + b + c end
local function first(...) return (...) end

local acc = Acc.new()
local check = 0
for i = 1, N do
  acc:add(i, 1)
  acc:forward(1, 2, 3)
  local a, b, c = acc:pack(i, i + 1, i + 2)
  check = check + sum3(a, b, c) + first(c, b, a)
  if i % 4 == 0 then check = check + sum3(table.unpack({1, 2, 3})) end
end
print(acc.total, acc.calls, check)
