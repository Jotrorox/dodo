-- Yielding from unusual places: across pcall/xpcall inside a coroutine,
-- inside metamethods (__index, __newindex, __add, __lt, __le, __eq,
-- __concat, __len, __call), inside generic-for iterators, and the
-- "attempt to yield across a C-call boundary" error from a table.sort
-- comparator and from string.gsub callbacks.

local function run(name, f, ...)
  local co = coroutine.create(f)
  local res = {coroutine.resume(co, ...)}
  local log = {}
  while coroutine.status(co) == "suspended" do
    log[#log + 1] = tostring(res[2])
    res = {coroutine.resume(co, "r" .. #log)}
  end
  print(name, "yields=[" .. table.concat(log, ",") .. "]",
        "final:", table.unpack(res, 1, 3))
end

-- across pcall
run("pcall", function()
  local ok, v = pcall(function()
    local r = coroutine.yield("in pcall")
    return r .. "!"
  end)
  return ok, v
end)
run("pcall-error-after-yield", function()
  return pcall(function()
    coroutine.yield("before error")
    error("after", 0)
  end)
end)
run("xpcall", function()
  return xpcall(function()
    coroutine.yield("in xpcall")
    error("x", 0)
  end, function(m)
    return "handled " .. m
  end)
end)

-- metamethods
local mt = {}
mt.__index = function(t, k) return coroutine.yield("index " .. k) end
mt.__newindex = function(t, k, v) rawset(t, k, coroutine.yield("newindex " .. k) .. v) end
mt.__add = function(a, b) return coroutine.yield("add") end
mt.__lt = function(a, b) return coroutine.yield("lt") == "r1" end
mt.__le = function(a, b) return coroutine.yield("le") end
mt.__eq = function(a, b) return coroutine.yield("eq") end
mt.__concat = function(a, b) return coroutine.yield("concat") end
mt.__len = function(a) return coroutine.yield("len") end
mt.__call = function(self, x) return coroutine.yield("call " .. x) end
mt.__unm = function(a) return coroutine.yield("unm") end
local A = setmetatable({}, mt)
local B = setmetatable({}, mt)

run("__index", function() return A.foo end)
run("__newindex", function() A.bar = "=v"; return rawget(A, "bar") end)
run("__add", function() return A + 1 end)
run("__lt true", function() return A < B end)
run("__lt false", function() coroutine.yield("pre"); return A < B end)
run("__le", function() return A <= B, A >= B end)
run("__eq", function() return A == B, A ~= B end)
run("__concat", function() return "x" .. A .. "y" end)
run("__len", function() return #A end)
run("__call", function() return A("arg") end)
run("__unm", function() return -A end)

-- inside a generic-for iterator function
run("for-iter", function()
  local sum = 0
  local function iter(_, i)
    if i >= 3 then return nil end
    coroutine.yield("iter " .. i)
    return i + 1
  end
  for i in iter, nil, 0 do sum = sum + i end
  return sum
end)

-- across a C function that does not support yields
run("sort", function()
  local t = {3, 1, 2}
  table.sort(t, function(a, b) coroutine.yield("cmp"); return a < b end)
  return "sorted"
end)
run("pcall(sort)", function()
  return pcall(table.sort, {2, 1}, function(a, b) coroutine.yield(); return a < b end)
end)
run("gsub", function()
  return pcall(string.gsub, "abc", "%w", function(c) coroutine.yield(c) end)
end)
-- __gc / tostring metamethod called from C (tostring is not yieldable)
run("tostring", function()
  local o = setmetatable({}, {__tostring = function() coroutine.yield("ts"); return "s" end})
  return pcall(tostring, o)
end)
-- load reader function cannot yield either
run("load reader", function()
  return load(function() coroutine.yield("reader"); return nil end)
end)
