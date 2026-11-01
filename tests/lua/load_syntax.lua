-- Syntax error messages returned by load (nil + message): unexpected
-- symbols, missing 'end'/')'/'=' with the "to close ... at line N" hints,
-- bad escapes and numerals, goto/label errors, <const>/<close> misuse,
-- and lexer errors. All use "=src" so positions are short.

local cases = {
  "x = = 1",
  "local function f() return 1",
  "if x then",
  "while true do",
  "for i = 1, 2 do\n\nprint(i)",
  "repeat x = 1",
  "f(1, 2",
  "t = {1, 2",
  "local x 1",
  "x",
  "x.y",
  "1 + 1",
  "return return",
  "local = 3",
  "local function 1() end",
  "for i in do end",
  "for i = 1 do end",
  "for 1 = 1, 2 do end",
  "function f(a,) end",
  "function f(... , a) end",
  "x = 'unfinished",
  "x = \"line\nbreak\"",
  "x = [==[ never closed",
  "--[[ unfinished comment",
  "x = '\\q'",
  "x = '\\300'",
  "x = '\\xZZ'",
  "x = '\\u{110000000}'",
  "x = '\\u{zz}'",
  "x = 3x",
  "x = 0x",
  "x = 1e",
  "x = 1..2",
  "x = @",
  "x = 1 +",
  "goto nowhere",
  "::a:: ::a::",
  "do goto l1 end local x ::l1:: print(x)",
  "break",
  "local x <const> = 1; x = 2",
  "local x <foo> = 1",
  "local a <close>, b <close> = 1, 2",
  "local function f() local x <const> = 1; return function() x = 3 end end",
  "return ...",
  "function f() return ... end",
  "a.b:c = 1",
  "a:b",
  "local t = {[1] 2}",
  "if x then else elseif y then end",
  "elseif",
  "end",
  "x = }",
  "(f)",
  "f() = 1",
  "x, y() = 1, 2",
  "local x = function",
  "return 1,",
  "x = not",
  "return #",
  "a = 1 b = 2 c",
  "x = 0x1p",
  "x = 08",
  "x = .5e+",
  "return " .. string.rep("(", 100) .. "1" .. string.rep(")", 100),
}

for i, src in ipairs(cases) do
  local f, err = load(src, "=src")
  print(i, f == nil and err or ("compiled ok: " .. type(f)))
end

-- syntax errors report the line where the problem is detected
print(select(2, load("local a = 1\nlocal b = 2\nlocal c = = 3", "=lines")))
print(select(2, load("function f()\n  if x then\n    y()\n  end\n", "=lines")))
print(select(2, load("f(\n1,\n2\n", "=lines")))
print(select(2, load("x = {\n1,\n2\n", "=lines")))
print(select(2, load("local s = [[\nabc\n", "=lines")))
