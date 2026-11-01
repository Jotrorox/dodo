-- Bitwise metamethods __band __bor __bxor __shl __shr __bnot: dispatch from
-- either side (numbers on the left), strings that convert vs ones that do
-- not, floats without integer representation, and error messages when no
-- metamethod exists.

local trace = {}
local function d(v) return type(v) == "table" and (v.n or "tbl") or (type(v) == "string" and ("'" .. v .. "'") or tostring(v)) end
local mt = {}
for _, ev in ipairs({"__band", "__bor", "__bxor", "__shl", "__shr", "__bnot"}) do
  mt[ev] = function(a, b)
    trace[#trace + 1] = ev .. "(" .. d(a) .. "," .. d(b) .. ")"
    return ev
  end
end
local X = setmetatable({n = "X"}, mt)
local r = {X & 1, 1 & X, X | 2, 2 | X, X ~ 3, 3 ~ X, X << 4, 4 << X, X >> 5, 5 >> X, ~X}
print("results", table.concat(r, " "))
print("trace", table.concat(trace, " "))
trace = {}
local _ = X & "7"; _ = "7" | X; _ = 1.5 ~ X; _ = X >> 2.5; _ = X & X
print("mixed operands", table.concat(trace, " "))

-- plain integer semantics for reference
print("ints", 0xF0 & 0x3C, 0xF0 | 0x0F, 0xFF ~ 0x0F, ~0, 1 << 4, 256 >> 4)
print("shift edge", 1 << 63, 1 << 64, 1 << -1, -1 >> 1, -1 >> 63, -1 >> 64, 2 >> -1, 1 << 0)
print("negative shift", 8 >> -2, 8 << -2)
print("floats exact", 3.0 & 1, 2.0 ^ 2 | 0, math.type(3.0 & 1))
local function try(label, f) print(label, pcall(f)) end
-- unlike arithmetic, the string metatable has no bitwise metamethods in
-- 5.4, so even numeric strings are rejected
try("string conv", function() return "3" & 1 end)
try("string conv right", function() local s = "8"; return 1 >> s end)
try("string bnot", function() return ~"0" end)

-- errors: no metamethod
try("table &", function() local t = {}; return t & 1 end)
try("1 | table", function() local t = {}; return 1 | t end)
try("~table", function() local t = {}; return ~t end)
try("bool", function() return true << 1 end)
try("nil", function() local n; return n >> 1 end)
try("float no int", function() local f = 1.5; return f & 1 end)
try("float right", function() local f = 2.5; return 1 | f end)
try("huge float", function() return 2^63 | 0 end)
try("inf", function() return math.huge ~ 0 end)
try("nan", function() return (0/0) & 1 end)
try("string bad", function() return "abc" & 1 end)
try("string float", function() return "1.5" | 0 end)
try("string bad right", function() return 1 ~ "xyz" end)
try("~string bad", function() return ~"hi" end)
-- a table with only arithmetic metamethods does not provide bitwise ones
local arith = setmetatable({}, {__add = function() return 0 end})
try("only __add", function() return arith & 1 end)
-- metamethod present only on the right operand
local right = setmetatable({}, {__shl = function(a, b) return "shl:" .. d(a) end})
try("right side", function() return 5 << right end)
-- string metamethods give way to the table's metamethod
try("string with table", function() return "abc" | X end)
