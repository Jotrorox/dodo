-- Benchmark: method calls through metatables. A three-level class hierarchy
-- with methods resolved through chained __index tables, overridden methods
-- calling super, polymorphic dispatch over a mixed array, and setter-style
-- methods that write fields. Stresses OP_SELF, __index chain lookups and
-- table field access.

local N = 6000000

local Base = {}
Base.__index = Base
function Base.new(x) return setmetatable({x = x, hits = 0}, Base) end
function Base:value() return self.x end
function Base:bump() self.hits = self.hits + 1; return self end
function Base:area() return 0 end

local Middle = setmetatable({}, {__index = Base})
Middle.__index = Middle
function Middle.new(x, y)
  local o = Base.new(x)
  o.y = y
  return setmetatable(o, Middle)
end
function Middle:value() return Base.value(self) + self.y end
function Middle:area() return self.x * self.y end

local Leaf = setmetatable({}, {__index = Middle})
Leaf.__index = Leaf
function Leaf.new(x, y, z)
  local o = Middle.new(x, y)
  o.z = z
  return setmetatable(o, Leaf)
end
function Leaf:value() return Middle.value(self) * self.z end
function Leaf:volume() return self:area() * self.z end -- area found two levels up

local objs = {}
for i = 1, 300 do
  local k = i % 3
  if k == 0 then objs[i] = Base.new(i)
  elseif k == 1 then objs[i] = Middle.new(i, 2)
  else objs[i] = Leaf.new(i, 2, 3) end
end

local sum = 0
local leaf = Leaf.new(1, 2, 3)
for i = 1, N do
  local o = objs[i % 300 + 1]
  sum = sum + o:value() + o:area()
  o:bump()
  sum = sum + leaf:volume() + leaf:bump().hits % 3
end

local hits = 0
for i = 1, #objs do hits = hits + objs[i].hits end
print("sum", sum % 1000000007, "hits", hits, "leaf", leaf.hits)
