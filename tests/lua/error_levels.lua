-- error() levels: level 1 (default) prefixes the position of the error
-- call, level 2 the position of the caller, level 0 no position, levels
-- beyond the stack give no position; positions are only added to string
-- messages. Tail calls remove frames, which changes what level 2 means.

local function lvl(level, msg)
  error(msg or "msg", level)
end

local function caller(level)
  lvl(level)          -- line 11
end

print(pcall(lvl))            -- default level 1
print(pcall(lvl, 1))
print(pcall(lvl, 2))         -- caller is pcall (a C function): no position
print(pcall(caller, 1))
print(pcall(caller, 2))      -- points at line 11
print(pcall(caller, 3))      -- caller's caller is pcall: no position
print(pcall(caller, 0))
print(pcall(caller, 50))     -- beyond stack

-- level pointing to the caller line through several Lua frames
local function check(x)
  if type(x) ~= "number" then
    error("expected number, got " .. type(x), 2)
  end
  return x
end
local function user()
  return check("str")        -- tail call: user's frame is gone
end
print(pcall(user))           -- level 2 is pcall: no position
local function deeper() return (user()) end
print(pcall(deeper))

-- non-string messages never get a position
print(pcall(error, 42))
print(pcall(error, 42, 1))
local ok, v = pcall(lvl, 1, 3.5)
print(ok, v, math.type(v))
ok, v = pcall(lvl, 1, true)
print(ok, v)
local t = {}
ok, v = pcall(lvl, 1, t)
print(ok, v == t)

-- error with no arguments: error value is nil
print(pcall(error))
print(select("#", pcall(error)))
print(select("#", pcall(error, nil)))

-- level given as a float with integer value / as a string number
print(pcall(lvl, 1.0))
print(pcall(lvl, "1"))
print(pcall(error, "x", "bad"))
print(pcall(error, "x", 1.5))

-- error inside a method and inside a field function
local obj = {}
function obj:m() error("in method") end
function obj.f() error("in field") end
print(pcall(obj.m, obj))
print(pcall(obj.f))

-- error position inside a multi-line expression
local function multi()
  local a = 1
  error(
    "multi"
  )
end
print(pcall(multi))

-- position in a chunk loaded with a custom name
local chunk = load("local x = 1\nerror('from chunk')", "=custom")
print(pcall(chunk))
chunk = load("error('lvl2 from chunk', 2)", "=custom")
local function callchunk() chunk() end   -- this line
print(pcall(callchunk))
