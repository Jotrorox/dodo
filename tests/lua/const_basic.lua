-- <const> local variables: readable like normal locals, captured by
-- closures, constant-folded in expressions, compile errors on assignment
-- (also from nested functions and compound situations), shadowing by a new
-- local is allowed, and const with non-constant initializers.

local N <const> = 10
local S <const> = "str"
local F <const> = 2.5
local B <const> = true
local NIL <const> = nil
print("values", N, S, F, B, NIL)
print("expressions", N * 2, S .. "!", F + N, not B, N // 3, -N, N << 2, #S)
local function uses() return N + 1, S:upper() end
print("captured", uses())
-- non-constant initializers are fine
local T <const> = {x = 1}
T.x = 2               -- the table is mutable; only the variable is const
print("table fields mutable", T.x)
local R <const> = math.max(3, 7)
print("runtime init", R)
local A, B2 <const>, C = 1, 2, 3
print("mixed list", A, B2, C)
-- shadowing with a new local is allowed
local V <const> = "first"
do
  local V = "shadow"
  V = "reassigned shadow"
  print("shadow", V)
end
print("original", V)
-- const in loops
for i = 1, 3 do
  local SQ <const> = i * i
  io.write(SQ, " ")
end
print()
-- const used as a table key and in a constructor
local KEY <const> = "k"
local tab = {[KEY] = N, KEY}
print("as key", tab.k, tab[1])
-- integer/float constant folding keeps subtypes
local I <const> = 3
local FL <const> = 3.0
print("folding types", math.type(I + 1), math.type(FL + 1), I == FL, I / 1)
local BIG <const> = math.maxinteger
print("wraparound", BIG + 1 == math.mininteger)
local Z <const> = 0
print("division by const zero", pcall(function() return 1 // Z end))
print("float division by const zero", 1 / Z, -1 / Z)

-- compile errors
local function compile(label, src)
  local f, err = load(src, "=src")
  print(label, f and "compiled" or err)
end
compile("assign", "local x <const> = 1; x = 2")
compile("assign in nested fn", "local x <const> = 1; local function f() x = 2 end")
compile("assign deeply nested", "local x <const> = 1; return function() return function() x = 3 end end")
compile("multiple assign", "local a <const> = 1; local b = 2; b, a = 3, 4")
compile("compound in loop", "local c <const> = 0; for i = 1, 2 do c = c + i end")
compile("assign nil const", "local n <const> = nil; n = nil")
compile("const without init", "local x <const>; return x")
compile("const with no value assign", "local x <const>; x = 1")
compile("shadow then assign ok", "local x <const> = 1; do local x = 2; x = 3 end")
compile("function decl to const", "local f <const> = nil; function f() end")
compile("global with same name ok", "local x <const> = 1; _ENV.x = 2")
compile("const in for body", "for i = 1, 2 do local k <const> = i; k = 1 end")
compile("const attrib spacing", "local x < const > = 1; return x")
-- loop control variables are ordinary assignable locals in 5.4
compile("assign numeric for var", "for i = 1, 2 do i = 5 end")
compile("assign generic for var", "for k, v in pairs({}) do k = 1 end")
-- run a compiled chunk that reads const values
print("chunk result", load("local q <const> = 6; local r <const> = q * 7; return r")())
