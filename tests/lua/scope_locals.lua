-- Lexical scoping: local shadowing in nested blocks, do ... end blocks,
-- a local's scope starting after its declaration statement, repeat-until
-- conditions seeing body locals, 'local function' vs 'local f = function'
-- for recursion, if/while/for bodies as blocks, and globals vs locals.

local x = "outer"
do
  local x = "block"
  print("in block", x)
  do
    local x = "nested"
    print("nested", x)
  end
  print("back in block", x)
end
print("outer", x)

-- shadowing in the same scope: the new local hides the old one; closures
-- created before keep the old one
local y = 1
local get_old = function() return y end
local y = 2
print("same scope shadow", y, get_old())

-- the scope of a local begins after the declaration statement
local z = "global-ish"
do
  local z = z .. "+local"
  print("init uses outer", z)
end

-- repeat-until: the condition sees locals declared in the body
local i = 0
repeat
  local done = i >= 2
  i = i + 1
until done
print("repeat until local", i)

-- local function allows recursion; local f = function does not see itself
local function fact(n) if n <= 1 then return 1 end return n * fact(n - 1) end
print("local function", fact(5))
local ok, err = pcall(function()
  local f = function(n) if n <= 1 then return 1 end return n * f(n - 1) end
  return f(3)
end)
print("local f = function", ok, err)
-- with a global of the same name, the inner reference goes to the global
function g_fact(n) return "global called " .. n end
local g_fact = function(n) if n <= 1 then return 1 end return g_fact(n - 1) end
print("refers to global", g_fact(3))
g_fact = nil
_G.g_fact = nil

-- if/elseif/else bodies are separate blocks
local v = "outer"
if true then local v = "then" end
if false then else local v = "else" end
print("if blocks", v)
-- loop bodies
for _ = 1, 2 do local v = "loop" end
while true do local v = "while"; break end
print("loop blocks", v)

-- function parameters shadow outer locals
local p = "outer p"
local function params(p) return p end
print("param shadow", params("arg"), p)
-- parameters can be reassigned without affecting the caller
local orig = {val = 1}
local function reassign(t) t = {val = 2}; return t.val end
print("param reassign", reassign(orig), orig.val)

-- globals vs locals
gvar = "global value"
local function read_global() return gvar end
local gvar = "local value"
print("global vs local", read_global(), gvar, _G.gvar)
_G.gvar = nil

-- many locals in one function (up to the 200 limit is fine)
local src = {"local a1 = 1"}
for k = 2, 199 do src[#src + 1] = "local a" .. k .. " = a" .. (k - 1) .. " + 1" end
src[#src + 1] = "return a199"
print("199 locals", load(table.concat(src, "\n"))())
-- too many locals is a compile error
src = {}
for k = 1, 201 do src[#src + 1] = "local b" .. k end
local f, e = load(table.concat(src, "\n"), "=many")
print("201 locals", f, e)

-- local declared inside a block is not visible after it
do local hidden = 1 end
print("hidden after block", hidden)

-- nested functions see enclosing locals at any depth
local depth = "d0"
local function l1() return function() return function() return depth end end end
print("deep visibility", l1()()())

-- a local function name is local; a plain function statement is global
local function lf() end
function gf() end
print("local fn not global", _G.lf, type(_G.gf))
gf = nil
-- method definitions on local tables
local obj = {}
function obj.m(a) return a end
function obj:n(a) return self == obj, a end
print("methods", obj.m(1), obj:n(2))
