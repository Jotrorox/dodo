-- _ENV and globals: global names are fields of _ENV, _G is the initial
-- global table, local _ENV = {...} redirects free names, functions capture
-- _ENV as an upvalue, load with a custom env, setting _ENV to nil, and
-- __index/__newindex on environments.

print("_G is _ENV", _G == _ENV, _G._G == _G)
newglobal = 42
print("global is field", _ENV.newglobal, _G.newglobal, rawget(_G, "newglobal"))
_ENV.viaenv = "set via _ENV"
print("set via _ENV", viaenv)
newglobal, viaenv = nil, nil

-- local _ENV redirects free names inside the block
do
  local print = print
  local _ENV = {x = "env x"}
  y = "env y"
  print("local _ENV", x, y, _ENV.y)
end
print("outside", y)

-- functions capture _ENV lexically
local function make_env_fn()
  local _ENV = {value = "captured"}
  return function() return value end
end
print("captured _ENV", make_env_fn()())

-- _ENV as a parameter
local function with_env(_ENV) return a + b end
print("_ENV param", with_env({a = 1, b = 2}))

-- an environment with __index falling back to _G
do
  local env = setmetatable({}, {__index = _G})
  local _ENV = env
  local_only = "only in env"
  print("fallback", type(print), local_only, rawget(_G, "local_only"))
end
print("not leaked", local_only)

-- __newindex on the environment catches global assignments
do
  local log = {}
  local env = setmetatable({}, {__index = _G, __newindex = function(t, k, v)
    log[#log + 1] = k
    rawset(t, k, v)
  end})
  local _ENV = env
  g1 = 1
  g2 = 2
  g1 = 3
  print("newindex log", table.concat(log, ","), g1)
end

-- strict mode: error on undefined globals
do
  local strict = setmetatable({}, {__index = function(_, k) error("undefined variable '" .. k .. "'", 2) end})
  local ok, err = pcall(function()
    local _ENV = strict
    return undefined_thing
  end)
  print("strict", ok, err)
end

-- load with a custom environment
local env = {n = 5}
local chunk = load("m = n * 2; return m", "=chunk", "t", env)
print("load env", chunk(), env.m, m)
-- load without env uses the global table
print("load default", load("return _G == _ENV and type(print)")())
-- the chunk's _ENV is its first upvalue; changing env table contents
env.n = 100
print("env mutated", chunk())

-- _ENV = nil makes global access fail
local ok, err = pcall(function()
  local _ENV = nil
  return anything
end)
print("_ENV nil read", ok, err)
ok, err = pcall(function()
  local _ENV = nil
  anything = 1
end)
print("_ENV nil write", ok, err)
-- _ENV set to a non-table with __index (a string): globals are string methods
ok, err = pcall(function()
  local _ENV = "str"
  return len
end)
print("_ENV string", ok, err == string.len)

-- assigning to _ENV itself (upvalue of the main chunk)
local saved = _ENV
local function swap()
  _ENV = {print = saved.print, tostring = saved.tostring}
  print("inside swapped", tostring(type))
  _ENV = saved
end
swap()
print("restored", type(print))

-- global function declaration goes to the current _ENV
do
  local box = {}
  local _ENV = box
  function declared() return "in box" end
end
print("function decl", declared)

-- _G is just a field: replacing it does not change the environment
local realG = _G
_G = {}
print("replaced _G", type(print), realG.print == print)
_G = realG
print("_G restored", _G == _ENV)
