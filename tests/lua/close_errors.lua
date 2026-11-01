-- <close> error cases: non-closable values ("variable 'x' got a
-- non-closable value"), errors inside __close replacing the original error
-- (and being passed to the remaining handlers), errors in __close on normal
-- exit, __close removed before closing, and compile errors for attributes
-- ("multiple to-be-closed variables in local list", "unknown attribute").

local log = {}
local function res(name, fail)
  return setmetatable({}, {__close = function(_, err)
    log[#log + 1] = name .. "(" .. tostring(err) .. ")"
    if fail then error(fail, 0) end
  end})
end
local function flush(label)
  print(label, table.concat(log, " "))
  log = {}
end

-- non-closable values
print("table", pcall(function() local x <close> = {} end))
print("number", pcall(function() local num <close> = 1 end))
print("string", pcall(function() local s <close> = "str" end))
print("true", pcall(function() local b <close> = true end))
print("function", pcall(function() local f <close> = print end))
-- __close must be present at declaration
print("no __close", pcall(function() local y <close> = setmetatable({}, {__index = {}}) end))
-- values declared before the failing one are closed
print("earlier closed", pcall(function()
  local ok1 <close> = res("ok1")
  local bad <close> = 42
end))
flush("  log")

-- error inside __close on normal exit propagates
print("close error", pcall(function()
  local a <close> = res("a")
  local b <close> = res("b", "b failed")
end))
flush("  log")

-- error in __close replaces the original error; later handlers get the
-- new error object
print("replace error", pcall(function()
  local a <close> = res("a")
  local b <close> = res("b", "b failed")
  local c <close> = res("c")
  error("original", 0)
end))
flush("  log")
-- several failing handlers: the last error wins
print("chained errors", pcall(function()
  local a <close> = res("a", "a failed")
  local b <close> = res("b", "b failed")
  error("original", 0)
end))
flush("  log")
-- error in __close during return discards the return values
print("error on return", pcall(function()
  local a <close> = res("a", "close failed")
  return "value"
end))
flush("  log")
-- error in __close during break
print("error on break", pcall(function()
  for i = 1, 3 do
    local a <close> = res("loop" .. i, i == 2 and "loop failed" or nil)
    if i == 2 then break end
  end
  return "not reached"
end))
flush("  log")

-- metamethod removed after declaration: closing raises an error
local mt = {__close = function() end}
print("removed __close", pcall(function()
  local v <close> = setmetatable({}, mt)
  mt.__close = nil
end))
print("non-callable __close", pcall(function()
  local v <close> = setmetatable({}, {__close = function() end})
  getmetatable(v).__close = "nope"
end))

-- compile-time errors
local function compile(label, src)
  local f, err = load(src, "=src")
  print(label, f and "compiled" or err)
end
compile("multiple tbc", "local a <close>, b <close> = nil, nil")
compile("one tbc among many ok", "local a, b <close>, c = 1, nil, 3")
compile("unknown attribute", "local x <foo> = 1")
compile("attribute typo", "local x <Close> = nil")
compile("empty attribute", "local x <> = 1")
compile("assign to close var", "local x <close> = nil; x = 1")
compile("close without value ok", "local x <close>")
compile("close in for header", "for i <close> = 1, 2 do end")
compile("attrib on function", "local function f <close> () end")
compile("missing >", "local x <close = nil")
-- 'close' and 'const' remain ordinary identifiers
compile("names ok", "local close, const = 1, 2; return close + const")
