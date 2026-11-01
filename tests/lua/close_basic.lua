-- <close> variables: __close called at scope exit in reverse declaration
-- order, on normal exit, break, return (after the return values are
-- computed), goto out of the block, and error (the error object is the
-- second argument; nil on normal exit). nil and false values are allowed
-- and ignored. Closing happens for each loop iteration.

local log = {}
local function res(name)
  return setmetatable({name = name}, {__close = function(self, err)
    local e = err
    if type(e) == "table" then e = "tbl:" .. tostring(e.msg) end
    log[#log + 1] = self.name .. "(" .. tostring(e) .. ")"
  end})
end
local function flush(label)
  print(label, table.concat(log, " "))
  log = {}
end

do
  local a <close> = res("a")
  local b <close> = res("b")
  local c <close> = res("c")
  log[#log + 1] = "body"
end
flush("reverse order")

-- nil and false are ignored
do
  local n <close> = nil
  local f <close> = false
  local r <close> = res("r")
end
flush("nil/false")

-- break
for i = 1, 3 do
  local x <close> = res("it" .. i)
  if i == 2 then break end
end
flush("loop+break")

-- while loop iterations each close
local w = 0
while w < 2 do
  w = w + 1
  local x <close> = res("w" .. w)
end
flush("while")

-- return: values are computed before closing
local function ret()
  local r <close> = res("ret")
  local t = {v = "before"}
  log[#log + 1] = "returning"
  return t.v, #log
end
print("ret values", ret())
flush("return")

-- return of a tail-call-looking expression is not a tail call with <close>
local function inner() log[#log + 1] = "inner runs"; return "inner" end
local function tailish()
  local r <close> = res("tail")
  return inner()
end
print("tailish", tailish())
flush("return call")

-- goto out of a block
do
  do
    local g <close> = res("g")
    goto out
  end
  ::out::
  log[#log + 1] = "after goto"
end
flush("goto")

-- goto backwards out of the variable's scope closes each time
do
  local n = 0
  ::again::
  do
    local gb <close> = res("gb" .. n)
    n = n + 1
    if n < 3 then goto again end
  end
end
flush("goto loop")

-- error: the error object is passed; error continues propagating
local ok, err = pcall(function()
  local e1 <close> = res("e1")
  local e2 <close> = res("e2")
  error("oops", 0)
end)
print("error", ok, err)
flush("error")
ok, err = pcall(function()
  local e <close> = res("etab")
  error({msg = "table error"})
end)
print("error table", ok, type(err), err.msg)
flush("error table")
-- error(nil): the handler receives nil as the error object
ok, err = pcall(function()
  local e <close> = res("enil")
  error(nil)
end)
print("error nil", ok, err)
flush("error nil")

-- variable declared after the error point is not closed
ok = pcall(function()
  local first <close> = res("first")
  error("early", 0)
  local second <close> = res("second")
end)
flush("partial")

-- <close> value captured by closures is still usable before close
local function make()
  local h <close> = res("h")
  return function() return h.name end
end
local getname = make()
print("captured", getname())
flush("captured")

-- a callable __close object (table with __call) works
local calls = 0
local callable = setmetatable({}, {__call = function() calls = calls + 1 end})
do local c <close> = setmetatable({}, {__close = callable}) end
print("callable __close", calls)

-- __close must exist at declaration, but it is looked up again at
-- closing time, so a replaced metamethod is the one called
local mt = {__close = function() log[#log + 1] = "old" end}
do
  local v <close> = setmetatable({}, mt)
  mt.__close = function() log[#log + 1] = "new" end
end
flush("changed mm")
-- the value of a <close> variable is the object itself
do
  local v <close> = res("val")
  print("value", v.name, getmetatable(v).__close ~= nil)
end
flush("value")
