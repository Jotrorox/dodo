-- __concat (table on either side, numbers and strings as the other
-- operand, right associativity of chains, results of any type) and __len
-- (called by #, receives the operand, result not adjusted, absent for raw
-- tables), plus the errors without metamethods.

local trace = {}
local function d(v) return type(v) == "table" and (v.n or "tbl") or (type(v) == "string" and ("'" .. v .. "'") or tostring(v)) end
local C = {__concat = function(a, b)
  trace[#trace + 1] = d(a) .. ".." .. d(b)
  local function s(v) return type(v) == "table" and ("<" .. v.n .. ">") or tostring(v) end
  return s(a) .. s(b)
end}
local x = setmetatable({n = "x"}, C)
local y = setmetatable({n = "y"}, C)
print("table..str", x .. "s")
print("str..table", "s" .. x)
print("table..num", x .. 1, 1 .. x, x .. 2.5)
print("table..table", x .. y)
print("trace", table.concat(trace, " "))
trace = {}

-- chains are right associative: "a" .. "b" .. x concatenates "b" .. x first
local r = "a" .. "b" .. x
print("chain1", r, table.concat(trace, " "))
trace = {}
r = x .. "a" .. "b"
print("chain2", r, table.concat(trace, " "))
trace = {}
r = "a" .. x .. "b" .. y .. "c"
print("chain3", r, table.concat(trace, " "))
trace = {}

-- result may be any type; with a non-string result the chain continues
-- through metamethods again
local T = setmetatable({n = "T"}, {__concat = function(a, b) return 42 end})
print("non-string result", T .. "x", math.type(T .. "x"))
local Z = setmetatable({n = "Z"}, {__concat = function(a, b) return {n = "fresh"} end})
print("table result", type(Z .. "x"))
print("chain with number result", "p" .. (T .. "q"))
-- only the right operand has __concat
local plain = {n = "plain"}
print("right only", plain .. x)
print("trace", table.concat(trace, " "))
trace = {}
-- left operand's metamethod is preferred
local L = setmetatable({n = "L"}, {__concat = function() return "left" end})
local R = setmetatable({n = "R"}, {__concat = function() return "right" end})
print("left wins", L .. R, R .. L)

-- numbers concatenate directly (no metamethod)
print("numbers", 1 .. 2, 1.5 .. "", -0.0 .. "", 2^63 .. "", math.mininteger .. "")
print("int float", 10 // 1 .. "|" .. 10 / 1)

-- errors without __concat
local function try(label, f) print(label, pcall(f)) end
try("table..str", function() local t = {}; return t .. "x" end)
try("str..table", function() local t = {}; return "x" .. t end)
try("nil..str", function() local n; return n .. "x" end)
try("bool", function() return "x" .. true end)
try("field", function() local t = {f = {}}; return "a" .. t.f end)
try("global", function() return "a" .. undefined_global end)
try("chain error", function() local t = {}; return "a" .. "b" .. t .. "c" end)
try("concat error in mm", function() return setmetatable({}, {__concat = function() error("cc", 0) end}) .. "" end)

-- __len
local len_args
local Lmt = {__len = function(self, other) len_args = {self, other}; return 99, "ignored" end}
local lt = setmetatable({1, 2, 3}, Lmt)
print("__len", #lt, rawlen(lt))
print("__len args", len_args[1] == lt, len_args[2] == lt)
-- result not adjusted to integer
local weird = setmetatable({}, {__len = function() return "long" end})
print("string length", #weird)
local fl = setmetatable({}, {__len = function() return 2.5 end})
print("float length", #fl, math.type(#fl))
local none = setmetatable({}, {__len = function() end})
print("nil length", #none)
-- __len on strings is not consulted (strings have built-in length)
print("string #", #"abc")
-- rawlen
print("rawlen", rawlen({1, 2}), rawlen("abcd"), pcall(rawlen, 5))
-- __len error propagates
local blen = setmetatable({}, {__len = function() error("no len", 0) end})
print("len error", pcall(function() return #blen end))
-- # on a table with a metatable but no __len is the raw border
print("no __len", #setmetatable({1, 2, 3}, {}))
