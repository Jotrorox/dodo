-- Program test: a small class system built on metatables. Covers inheritance
-- through __index chains, super calls, mixins copied into classes,
-- __tostring, class-level metamethods (__eq, __lt, __le, __add, __sub,
-- __unm, __mul, __concat, __len, __call, __index fallbacks, __newindex
-- guards), class-call constructors via a metaclass __call, isinstance
-- checks walking the chain, and metamethod inheritance (metamethods are not
-- looked up through __index, so the class system copies them down).

local METAMETHODS = {
  "__tostring", "__eq", "__lt", "__le", "__add", "__sub", "__mul", "__div",
  "__unm", "__concat", "__len", "__call", "__mod", "__idiv",
}

local Object = {}
Object.__index = Object
Object.__name = "Object"

local function new_instance(cls, ...)
  local obj = setmetatable({}, cls)
  if cls.init then cls.init(obj, ...) end
  return obj
end

setmetatable(Object, {__call = new_instance, __tostring = function(c) return "class " .. c.__name end})

function Object:extend(name)
  local cls = {}
  for _, mm in ipairs(METAMETHODS) do
    if rawget(self, mm) then cls[mm] = self[mm] end
  end
  cls.__index = cls
  cls.__name = name
  cls.super = self
  return setmetatable(cls, {
    __index = self,
    __call = new_instance,
    __tostring = function(c) return "class " .. c.__name end,
  })
end

function Object:include(mixin)
  for k, v in pairs(mixin) do
    if self[k] == nil or k:sub(1, 2) == "__" then self[k] = v end
  end
  return self
end

function Object:is(cls)
  local mt = getmetatable(self)
  while mt do
    if mt == cls then return true end
    mt = mt.super
  end
  return false
end

function Object:class_name() return getmetatable(self).__name end

-- Shapes: classic inheritance with super calls ---------------------------

local Shape = Object:extend("Shape")
function Shape:init(name) self.name = name end
function Shape:area() return 0 end
function Shape:describe()
  return string.format("%s %s with area %.2f", self:class_name(), self.name, self:area())
end
Shape.__tostring = function(s) return "<" .. s:describe() .. ">" end
Shape.__lt = function(a, b) return a:area() < b:area() end
Shape.__le = function(a, b) return a:area() <= b:area() end

local Rect = Shape:extend("Rect")
function Rect:init(name, w, h)
  Rect.super.init(self, name)
  self.w, self.h = w, h
end
function Rect:area() return self.w * self.h end

local Square = Rect:extend("Square")
function Square:init(name, side)
  Square.super.init(self, name, side, side)
end
function Square:describe()
  return Square.super.describe(self) .. " (side " .. self.w .. ")"
end

local Circle = Shape:extend("Circle")
function Circle:init(name, r)
  self.super.init(self, name)
  self.r = r
end
function Circle:area() return 3 * self.r * self.r end -- integer "pi" keeps output exact

local shapes = { Rect("r1", 3, 5), Square("s1", 3), Circle("c1", 2), Rect("r2", 1, 10), Square("s2", 1) }
for _, s in ipairs(shapes) do print(tostring(s)) end
table.sort(shapes)
local names = {}
for i, s in ipairs(shapes) do names[i] = s.name end
print("sorted by area:", table.concat(names, " "))
print(shapes[2].name .. " < " .. shapes[3].name, shapes[2] < shapes[3],
      shapes[3].name .. " < " .. shapes[2].name, shapes[3] < shapes[2],
      "self <=", shapes[4] <= shapes[4], "self <", shapes[4] < shapes[4])
print("Square is Rect/Shape/Circle:", Square("x", 1):is(Rect), Square("x", 1):is(Shape), Square("x", 1):is(Circle))
print(tostring(Square), tostring(Object), Square.super == Rect)

-- Mixins ----------------------------------------------------------------

local Comparable = {
  __eq = function(a, b) return a:key() == b:key() end,
  __lt = function(a, b) return a:key() < b:key() end,
  __le = function(a, b) return a:key() <= b:key() end,
}
local Serializable = {
  serialize = function(self)
    local keys = {}
    for k in pairs(self) do keys[#keys + 1] = k end
    table.sort(keys)
    local parts = {}
    for _, k in ipairs(keys) do parts[#parts + 1] = k .. "=" .. tostring(self[k]) end
    return self:class_name() .. "{" .. table.concat(parts, ",") .. "}"
  end,
}

local Version = Object:extend("Version"):include(Comparable):include(Serializable)
function Version:init(s)
  self.major, self.minor, self.patch = s:match("^(%d+)%.(%d+)%.(%d+)$")
  self.major, self.minor, self.patch = tonumber(self.major), tonumber(self.minor), tonumber(self.patch)
end
function Version:key() return self.major * 1000000 + self.minor * 1000 + self.patch end
Version.__tostring = function(v) return v.major .. "." .. v.minor .. "." .. v.patch end

local vs = {}
for _, s in ipairs({"1.10.0", "1.2.3", "0.9.12", "1.2.10", "2.0.0", "1.2.3"}) do vs[#vs + 1] = Version(s) end
table.sort(vs)
local out = {}
for i, v in ipairs(vs) do out[i] = tostring(v) end
print("versions:", table.concat(out, " < "))
print("eq distinct objects:", vs[2] == vs[3], rawequal(vs[2], vs[3]), vs[1] == vs[2])
print(vs[1]:serialize())

-- Arithmetic metamethods -------------------------------------------------

local Vec = Object:extend("Vec")
function Vec:init(x, y) self.x, self.y = x, y end
Vec.__add = function(a, b) return Vec(a.x + b.x, a.y + b.y) end
Vec.__sub = function(a, b) return Vec(a.x - b.x, a.y - b.y) end
Vec.__mul = function(a, b)
  if type(a) == "number" then return Vec(a * b.x, a * b.y) end
  if type(b) == "number" then return Vec(a.x * b, a.y * b) end
  return a.x * b.x + a.y * b.y
end
Vec.__unm = function(a) return Vec(-a.x, -a.y) end
Vec.__eq = function(a, b) return a.x == b.x and a.y == b.y end
Vec.__len = function(a) return math.abs(a.x) + math.abs(a.y) end
Vec.__tostring = function(a) return "(" .. a.x .. "," .. a.y .. ")" end
Vec.__concat = function(a, b) return tostring(a) .. tostring(b) end
Vec.__call = function(self, k) return self[k] end

local Vec3 = Vec:extend("Vec3") -- inherits copied metamethods
function Vec3:init(x, y, z) Vec3.super.init(self, x, y); self.z = z or 0 end

local a, b = Vec(1, 2), Vec(3, -4)
print("a+b", tostring(a + b), "a-b", tostring(a - b), "-a", tostring(-a))
print("a*b", a * b, "2*a", tostring(2 * a), "a*3", tostring(a * 3))
print("#b", #b, "a..b", a .. b, "a..'!'", a .. "!", "'<'..b", "<" .. b)
print("a==Vec(1,2)", a == Vec(1, 2), "a~=b", a ~= b, "call", a("x"), a("y"))
local c = Vec3(1, 2, 3)
print("Vec3 inherits:", tostring(c + a), c:is(Vec), c:class_name(), c("z"))

-- __index function fallback and __newindex guard -------------------------

local Record = Object:extend("Record")
function Record:init(fields)
  rawset(self, "_fields", fields)
  rawset(self, "_log", {})
end
Record.__index = function(self, k)
  local v = Record[k]
  if v ~= nil then return v end
  local f = rawget(self, "_fields")
  if f[k] ~= nil then return f[k] end
  return "<missing " .. k .. ">"
end
Record.__newindex = function(self, k, v)
  local f = rawget(self, "_fields")
  if f[k] == nil then error("unknown field '" .. k .. "'", 2) end
  local log = rawget(self, "_log")
  log[#log + 1] = k .. ":" .. tostring(f[k]) .. "->" .. tostring(v)
  f[k] = v
end

local r = Record({name = "ada", age = 36})
print(r.name, r.age, r.email, r:class_name())
r.age = 37
r.name = "Ada"
local ok, err = pcall(function() r.email = "x" end)
print("guard:", ok, (err:gsub("^.-:%d+: ", "")))
print("log:", table.concat(rawget(r, "_log"), "; "))

-- Counter class with class-level state and __call constructors ------------

local Counter = Object:extend("Counter")
Counter.instances = 0
function Counter:init(start)
  Counter.instances = Counter.instances + 1
  self.n = start or 0
end
function Counter:inc(by) self.n = self.n + (by or 1); return self end
local c1, c2 = Counter(), Counter(10)
c1:inc():inc():inc(5)
c2:inc(-3)
print("counters:", c1.n, c2.n, Counter.instances, rawget(c1, "instances"))

-- deep chain lookup
local L = Object
for i = 1, 20 do
  L = L:extend("L" .. i)
  local depth = i
  L["m" .. i] = function() return depth end
end
local deep = L()
print("deep chain:", deep.m1(), deep.m10(), deep.m20(), deep:class_name(), deep:is(Object))
