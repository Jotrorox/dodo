-- Lexer: operators, comments, identifiers, keywords and line counting
-- operators written without whitespace
local a, b = 7, 2
print(a+b, a-b, a*b, a/b, a%b, a^b, a//b)
print(a&b, a|b, a~b, a<<b, a>>b, ~a)
print(a==b, a~=b, a<b, a<=b, a>b, a>=b)
print(a..b, #"abc", -a, not a)
print(a-  -b, a- -b, - -a)
print(7//2*2, 2^3^2, -2^2, (2)^-1)
print(1 .. 2 == "12", "a".."b".."c")
local t = {x = {y = {z = 42}}}
print(t.x.y.z, t["x"]["y"].z, t . x . y . z)
local function v(...) return select("#", ...), ... end
print(v(1, 2, 3))
print(#{...}, select("#", ...))

-- labels and goto
do
  local i = 1
  ::top::
  if i < 3 then i = i + 1; goto top end
  print("goto result", i)
end

-- semicolons and empty statements
;;; local s = 1; ; print("semicolons", s);

-- identifiers
local _ = 1
local _x1, X_Y, __ = 2, 3, 4
local abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789 = 5
print(_, _x1, X_Y, __, abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789)
print(type(_ENV), _ENV == _G)
-- keywords cannot be identifiers but can be table keys via strings
local kw = {["end"] = 1, ["and"] = 2}
print(kw["end"], kw["and"])
for _, src in ipairs{"local and = 1", "local x = {end = 1}", "goto = 1",
                     "local function return() end", "local nil = 1", "x.and = 1"} do
  print(src, select(2, load(src, "=kw")))
end
print("And/END are identifiers:", load("local And, END = 1, 2 return And + END")())

-- comments at end of input without a trailing newline
print(load("return 5 -- comment")())
print(load("return 6 --[[ c ]]")())
print(load("return 7 --")())
print(load("return 8\n--")())

-- line numbering: \n, \r, \r\n and \n\r each end one line
local function line_of(src)
  local f = assert(load(src, "=c"))
  local ok, err = pcall(f)
  return err
end
print(line_of("\n\nerror('x')"))
print(line_of("\r\rerror('x')"))
print(line_of("\r\n\r\nerror('x')"))
print(line_of("\n\r\n\rerror('x')"))
print(line_of("\n\n\rerror('x')"))
print(line_of("\r\n\nerror('x')"))
print(line_of("--[[\n\n]]error('x')"))
print(line_of("local s = [[\r\n\r\n]] error('x')"))

-- whitespace characters accepted between tokens
print(load("return\t1\v+\f2\r+3")())

-- a method call on a string literal needs parentheses
print(("%d"):format(5), #"x" + 1)
print(select(2, load("return 'x':upper()", "=m")))
