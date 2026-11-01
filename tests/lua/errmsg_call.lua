-- Error messages for calling non-callable values, with variable
-- descriptions (global, local, field, method, upvalue), calls through
-- __call with non-function values, and argument errors for Lua-visible
-- library functions (bad argument #n to 'name', calling 'x' on bad self).

local function try(f, ...)
  print(select(2, pcall(f, ...)))
end

try(function() undefined_function() end)
try(function() local f; f() end)
try(function() local f = 1; f() end)
try(function() local f = 1.5; f(1, 2) end)
try(function() local t = {}; t() end)
try(function() local s = "str"; s() end)
try(function() local b = true; b() end)
try(function() local t = {}; t.method() end)
try(function() local t = {}; t:method() end)
try(function() local t = {sub = {}}; t.sub.fn() end)
local upv
try(function() upv() end)
try(function() local t = {}; t[1]() end)
try(function() local t = {}; local k = "dyn"; t[k]() end)
try(function() return (nil)() end)
try(function() local function g() end; g()() end)
try(function() math.nonexistent(1) end)
try(function() string.nothing("x") end)

-- __call that is not a function
local weird = setmetatable({}, {__call = 42})
try(function() weird() end)
-- __call chain: a callable table used as __call is itself called
local inner = setmetatable({}, {__call = function() return "inner" end})
local outer = setmetatable({}, {__call = inner})
try(function() return outer() end)

-- calling via pcall directly
print(pcall(nil))
print(pcall(undefined_function))
print(pcall(setmetatable({}, {})))

-- calls in generic for
try(function() for k in nil do end end)
try(function() for k in 5 do end end)
try(function() local t = {}; for k, v in t do end end)

-- bad arguments to library functions
try(function() table.insert(nil, 1) end)
try(function() table.insert({}, 1, 2, 3) end)
try(function() table.insert({}, 5, 2) end)
try(function() table.concat({1, {}, 3}) end)
try(function() string.rep() end)
try(function() string.rep("x", "y") end)
try(function() string.sub() end)
try(function() ("x"):rep({}) end)
try(function() math.floor("a") end)
try(function() math.floor({}) end)
try(function() setmetatable(1, {}) end)
try(function() setmetatable({}, 1) end)
try(function() ipairs() end)
try(function() select(0) end)
try(function() select(-5, 1) end)
try(function() tostring() end)
try(function() rawlen(5) end)
try(function() rawset({}, nil, 1) end)
try(function() string.format("%d", 1.5) end)
try(function() string.format("%d", "x") end)
try(function() string.char(256) end)
try(function() local s = string.rep; s() end)

-- method style: "calling 'xyz' on bad self"
try(function() local f = io.stdout; f.write(nil, "x") end)
try(function() io.stdout.write(42) end)
try(function() io.stdout:seek("bogus") end)
local s = "abc"
try(function() s:rep() end)
try(function() s:byte({}) end)
local fake = {write = io.stdout.write, rep = string.rep, close = io.close}
try(function() fake:write("x") end)
try(function() fake:rep(2) end)
try(function() fake:close() end)
try(function() local n = 5; n:foo() end)

-- argument errors in functions stored under other names
local myins = table.insert
try(function() myins(nil, 1) end)
local holder = {fn = string.rep}
try(function() holder.fn() end)
