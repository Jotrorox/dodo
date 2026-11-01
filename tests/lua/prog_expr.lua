-- Program test: a tokenizer, recursive-descent parser and evaluator for a
-- small expression language: integer/float literals, variables, unary
-- minus, + - * / // % ^ (right-assoc), comparisons, 'and'/'or'/'not',
-- function calls with builtins, 'let x = e in e', and 'if c then a else b'.
-- The parser builds an AST that is pretty-printed back with minimal
-- parentheses, evaluated, and constant-folded. Syntax and runtime errors are
-- reported with column positions through error tables caught by pcall.

local Lexer = {}
Lexer.__index = Lexer

local KEYWORDS = { ["let"] = true, ["in"] = true, ["if"] = true, ["then"] = true,
                   ["else"] = true, ["and"] = true, ["or"] = true, ["not"] = true }

local function syntax_error(pos, msg)
  error({kind = "syntax", pos = pos, msg = msg})
end

function Lexer.tokenize(src)
  local toks, i = {}, 1
  while true do
    local s = src:find("%S", i)
    if not s then break end
    i = s
    local num = src:match("^%d+%.%d+", i) or src:match("^%d+", i)
    local ident = src:match("^[%a_][%w_]*", i)
    local op = src:match("^//", i) or src:match("^[<>=~]=", i) or src:match("^[-+*/%%^()<>,=]", i)
    if num then
      local kind = num:find("%.") and "float" or "int"
      toks[#toks + 1] = {t = "num", v = kind == "float" and tonumber(num) or math.tointeger(tonumber(num)), pos = i}
      i = i + #num
    elseif ident then
      toks[#toks + 1] = {t = KEYWORDS[ident] and ident or "name", v = ident, pos = i}
      i = i + #ident
    elseif op then
      toks[#toks + 1] = {t = op, pos = i}
      i = i + #op
    else
      syntax_error(i, "unexpected character '" .. src:sub(i, i) .. "'")
    end
  end
  toks[#toks + 1] = {t = "eof", pos = #src + 1}
  return toks
end

-- parser (precedence climbing for binary operators) -------------------------

local BINARY = {
  ["or"] = {1, "left"}, ["and"] = {2, "left"},
  ["<"] = {3, "left"}, [">"] = {3, "left"}, ["<="] = {3, "left"}, [">="] = {3, "left"},
  ["=="] = {3, "left"}, ["~="] = {3, "left"},
  ["+"] = {4, "left"}, ["-"] = {4, "left"},
  ["*"] = {5, "left"}, ["/"] = {5, "left"}, ["//"] = {5, "left"}, ["%"] = {5, "left"},
  ["^"] = {7, "right"},
}
local UNARY_PREC = 6

local Parser = {}
Parser.__index = Parser

function Parser.new(src)
  return setmetatable({toks = Lexer.tokenize(src), i = 1}, Parser)
end
function Parser:peek() return self.toks[self.i] end
function Parser:next() local t = self.toks[self.i]; self.i = self.i + 1; return t end
function Parser:expect(t)
  local tok = self:next()
  if tok.t ~= t then
    syntax_error(tok.pos, string.format("expected '%s' but found '%s'", t, tok.v or tok.t))
  end
  return tok
end

function Parser:primary()
  local tok = self:next()
  if tok.t == "num" then return {k = "num", v = tok.v}
  elseif tok.t == "name" then
    if self:peek().t == "(" then
      self:next()
      local args = {}
      if self:peek().t ~= ")" then
        repeat args[#args + 1] = self:expr(0) until self:peek().t ~= "," or not self:next()
      end
      self:expect(")")
      return {k = "call", name = tok.v, args = args, pos = tok.pos}
    end
    return {k = "var", name = tok.v, pos = tok.pos}
  elseif tok.t == "(" then
    local e = self:expr(0)
    self:expect(")")
    return e
  elseif tok.t == "-" or tok.t == "not" then
    return {k = "unop", op = tok.t, e = self:expr(UNARY_PREC)}
  elseif tok.t == "let" then
    local name = self:expect("name").v
    self:expect("=")
    local val = self:expr(0)
    self:expect("in")
    return {k = "let", name = name, val = val, body = self:expr(0)}
  elseif tok.t == "if" then
    local c = self:expr(0)
    self:expect("then")
    local a = self:expr(0)
    self:expect("else")
    return {k = "if", c = c, a = a, b = self:expr(0)}
  end
  syntax_error(tok.pos, "unexpected " .. (tok.t == "eof" and "end of input" or "'" .. (tok.v or tok.t) .. "'"))
end

function Parser:expr(min_prec)
  local lhs = self:primary()
  while true do
    local tok = self:peek()
    local info = BINARY[tok.t]
    if not info or info[1] < min_prec then break end
    self:next()
    -- left-assoc operators require the right operand to bind tighter
    local rhs = self:expr(info[2] == "left" and info[1] + 1 or info[1])
    lhs = {k = "bin", op = tok.t, l = lhs, r = rhs, pos = tok.pos}
  end
  return lhs
end

function Parser.parse(src)
  local p = Parser.new(src)
  local e = p:expr(0)
  local tok = p:peek()
  if tok.t ~= "eof" then syntax_error(tok.pos, "unexpected '" .. (tok.v or tok.t) .. "' after expression") end
  return e
end

-- pretty printer with minimal parentheses -------------------------------------

local function prec_of(node)
  if node.k == "bin" then return BINARY[node.op][1] end
  if node.k == "unop" then return UNARY_PREC end
  if node.k == "let" or node.k == "if" then return 0 end
  return 100
end

local function show(node)
  local k = node.k
  if k == "num" then return math.type(node.v) == "float" and string.format("%.14g", node.v):gsub("^(%-?%d+)$", "%1.0") or tostring(node.v)
  elseif k == "var" then return node.name
  elseif k == "call" then
    local args = {}
    for i, a in ipairs(node.args) do args[i] = show(a) end
    return node.name .. "(" .. table.concat(args, ", ") .. ")"
  elseif k == "unop" then
    local inner = show(node.e)
    if prec_of(node.e) < UNARY_PREC then inner = "(" .. inner .. ")" end
    return (node.op == "not" and "not " or "-") .. inner
  elseif k == "let" then return "let " .. node.name .. " = " .. show(node.val) .. " in " .. show(node.body)
  elseif k == "if" then return "if " .. show(node.c) .. " then " .. show(node.a) .. " else " .. show(node.b)
  end
  local p, assoc = BINARY[node.op][1], BINARY[node.op][2]
  local l, r = show(node.l), show(node.r)
  local lp, rp = prec_of(node.l), prec_of(node.r)
  if lp < p or (lp == p and assoc == "right") then l = "(" .. l .. ")" end
  if rp < p or (rp == p and assoc == "left") then r = "(" .. r .. ")" end
  return l .. " " .. node.op .. " " .. r
end

-- evaluator ---------------------------------------------------------------------

local BUILTINS = {
  max = math.max, min = math.min, abs = math.abs,
  sqrt = function(x) return math.sqrt(x) end,
  floor = function(x) return math.floor(x) end,
  fact = function(n)
    local r = 1
    for i = 2, n do r = r * i end
    return r
  end,
}

local function runtime_error(pos, msg) error({kind = "runtime", pos = pos, msg = msg}) end

local function eval(node, env)
  local k = node.k
  if k == "num" then return node.v
  elseif k == "var" then
    local v = env[node.name]
    if v == nil then runtime_error(node.pos, "undefined variable '" .. node.name .. "'") end
    return v
  elseif k == "unop" then
    local v = eval(node.e, env)
    if node.op == "-" then return -v end
    return not v
  elseif k == "let" then
    local child = setmetatable({[node.name] = eval(node.val, env)}, {__index = env})
    return eval(node.body, child)
  elseif k == "if" then
    if eval(node.c, env) then return eval(node.a, env) end
    return eval(node.b, env)
  elseif k == "call" then
    local f = BUILTINS[node.name]
    if not f then runtime_error(node.pos, "unknown function '" .. node.name .. "'") end
    local args = {}
    for i, a in ipairs(node.args) do args[i] = eval(a, env) end
    local ok, res = pcall(f, table.unpack(args))
    if not ok then runtime_error(node.pos, node.name .. ": " .. tostring(res):gsub("^.-:%d+: ", "")) end
    return res
  end
  local op = node.op
  if op == "and" then return eval(node.l, env) and eval(node.r, env) end
  if op == "or" then return eval(node.l, env) or eval(node.r, env) end
  local a, b = eval(node.l, env), eval(node.r, env)
  if op == "+" then return a + b elseif op == "-" then return a - b
  elseif op == "*" then return a * b elseif op == "/" then return a / b
  elseif op == "//" or op == "%" then
    if math.type(a) == "integer" and math.type(b) == "integer" and b == 0 then
      runtime_error(node.pos, "integer division by zero")
    end
    if op == "//" then return a // b end
    return a % b
  elseif op == "^" then return a ^ b
  elseif op == "<" then return a < b elseif op == ">" then return a > b
  elseif op == "<=" then return a <= b elseif op == ">=" then return a >= b
  elseif op == "==" then return a == b elseif op == "~=" then return a ~= b
  end
end

-- constant folding: replaces closed arithmetic subtrees with literals
local function fold(node)
  if node.k == "bin" then
    node.l, node.r = fold(node.l), fold(node.r)
    if node.l.k == "num" and node.r.k == "num" and BINARY[node.op][1] >= 4 then
      local ok, v = pcall(eval, node, {})
      if ok and type(v) == "number" then return {k = "num", v = v} end
    end
  elseif node.k == "unop" then
    node.e = fold(node.e)
    if node.e.k == "num" and node.op == "-" then return {k = "num", v = -node.e.v} end
  end
  return node
end

local function fmt(v)
  if math.type(v) == "float" then
    local s = string.format("%.10g", v)
    if s:find("^%-?%d+$") then s = s .. ".0" end
    return s
  end
  return tostring(v)
end

local cases = {
  "1 + 2 * 3", "(1 + 2) * 3", "2 ^ 3 ^ 2", "(2 ^ 3) ^ 2", "-2 ^ 2", "10 - 4 - 3",
  "10 - (4 - 3)", "7 // 2 + 7 % 3", "-7 // 2", "7 / 2", "1.5 * 4", "2 ^ 0.5",
  "let x = 5 in x * x + 1", "let x = 2 in let y = x + 1 in x * y", "let x = 1 in (let x = 10 in x) + x",
  "max(3, 9, 4) - min(8, 2)", "fact(20)", "sqrt(16) + abs(-3)", "floor(7.9) + 1",
  "if 3 > 2 then 100 else 200", "if 1 == 2 or 2 <= 2 and not (3 ~= 3) then 1 else 0",
  "a * b + c", "a - -b", "not not true_ish", "1 < 2 == (3 > 1)",
}
local env = {a = 6, b = 7, c = 8, true_ish = 1}
for _, src in ipairs(cases) do
  local ok, res = pcall(function()
    local ast = Parser.parse(src)
    local printed = show(ast)
    local reparsed = show(Parser.parse(printed))
    local value = eval(ast, env)
    local folded = show(fold(Parser.parse(src)))
    return string.format("%-50s => %-8s | %s%s | fold: %s", src, fmt(value), printed,
                         reparsed == printed and "" or " (REPARSE MISMATCH " .. reparsed .. ")", folded)
  end)
  print(ok and res or ("ERROR " .. tostring(res)))
end

local errors = {
  "1 +", "(1 + 2", "1 2", "let = 3 in 4", "let x = 1 x", "if 1 then 2", "3 $ 4", "f(1,",
  "unknown_var + 1", "nope(3)", "5 // 0", "5 % (2 - 2)", "fact(\"x\")", ")", "max()", "3 < nope_var",
}
for _, src in ipairs(errors) do
  local ok, err = pcall(function() return eval(Parser.parse(src), env) end)
  if ok then
    print(string.format("%-16s no error: %s", src, fmt(err)))
  elseif type(err) == "table" then
    print(string.format("%-16s %s error at column %d: %s", src, err.kind, err.pos, err.msg))
    print("                 " .. src)
    print("                 " .. string.rep(" ", err.pos - 1) .. "^")
  else
    print(src, "lua error:", err)
  end
end
