-- Program test: a Brainfuck interpreter in three flavours. (1) a direct
-- interpreter with a precomputed bracket jump table, (2) an optimizing
-- interpreter that folds runs into instructions, (3) a compiler that emits
-- Lua source and runs it through load(). Each runs embedded programs that
-- print strings (classic hello world, a generated program for an arbitrary
-- message, input reversal, nested-loop arithmetic) and the outputs and
-- instruction counts are compared. Also checks error reporting for
-- unbalanced brackets.

local function jump_table(code)
  local stack, jumps = {}, {}
  for i = 1, #code do
    local c = code:sub(i, i)
    if c == "[" then
      stack[#stack + 1] = i
    elseif c == "]" then
      local j = table.remove(stack)
      if not j then error("unmatched ']' at " .. i, 0) end
      jumps[i], jumps[j] = j, i
    end
  end
  if #stack > 0 then error("unmatched '[' at " .. stack[#stack], 0) end
  return jumps
end

-- (1) straightforward interpreter
local function run_simple(code, input)
  local jumps = jump_table(code)
  local tape, ptr, pc, inpos, steps = {}, 1, 1, 1, 0
  local out = {}
  local n = #code
  while pc <= n do
    local c = code:byte(pc)
    steps = steps + 1
    if c == 43 then tape[ptr] = ((tape[ptr] or 0) + 1) & 255        -- +
    elseif c == 45 then tape[ptr] = ((tape[ptr] or 0) - 1) & 255    -- -
    elseif c == 62 then ptr = ptr + 1                               -- >
    elseif c == 60 then ptr = ptr - 1                               -- <
    elseif c == 46 then out[#out + 1] = string.char(tape[ptr] or 0) -- .
    elseif c == 44 then                                             -- ,
      tape[ptr] = input and input:byte(inpos) or 0
      inpos = inpos + 1
    elseif c == 91 then if (tape[ptr] or 0) == 0 then pc = jumps[pc] end
    elseif c == 93 then if (tape[ptr] or 0) ~= 0 then pc = jumps[pc] end
    else steps = steps - 1
    end
    pc = pc + 1
  end
  return table.concat(out), steps
end

-- (2) optimizing interpreter: parse into {op, arg} with folded runs
local function parse(code)
  code = code:gsub("[^%+%-<>%.,%[%]]", "")
  local ops, stack = {}, {}
  local i = 1
  while i <= #code do
    local c = code:sub(i, i)
    if c == "+" or c == "-" or c == ">" or c == "<" then
      local j = i
      while code:sub(j + 1, j + 1) == c do j = j + 1 end
      local count = j - i + 1
      if c == "+" then ops[#ops + 1] = {"add", count}
      elseif c == "-" then ops[#ops + 1] = {"add", -count}
      elseif c == ">" then ops[#ops + 1] = {"move", count}
      else ops[#ops + 1] = {"move", -count} end
      i = j
    elseif code:sub(i, i + 2) == "[-]" then
      ops[#ops + 1] = {"clear", 0}
      i = i + 2
    elseif c == "[" then
      ops[#ops + 1] = {"jz", 0}
      stack[#stack + 1] = #ops
    elseif c == "]" then
      local open = table.remove(stack)
      if not open then error("unmatched ']'", 0) end
      ops[#ops + 1] = {"jnz", open}
      ops[open][2] = #ops
    elseif c == "." then ops[#ops + 1] = {"out", 0}
    elseif c == "," then ops[#ops + 1] = {"in", 0}
    end
    i = i + 1
  end
  if #stack > 0 then error("unmatched '['", 0) end
  return ops
end

local function run_fast(code, input)
  local ops = parse(code)
  local tape = setmetatable({}, {__index = function() return 0 end})
  local ptr, pc, inpos, executed = 0, 1, 1, 0
  local out = {}
  while pc <= #ops do
    local op = ops[pc]
    local kind, arg = op[1], op[2]
    executed = executed + 1
    if kind == "add" then tape[ptr] = (tape[ptr] + arg) % 256
    elseif kind == "move" then ptr = ptr + arg
    elseif kind == "clear" then tape[ptr] = 0
    elseif kind == "jz" then if tape[ptr] == 0 then pc = arg end
    elseif kind == "jnz" then if tape[ptr] ~= 0 then pc = arg end
    elseif kind == "out" then out[#out + 1] = string.char(tape[ptr])
    elseif kind == "in" then
      tape[ptr] = input and input:byte(inpos) or 0
      inpos = inpos + 1
    end
    pc = pc + 1
  end
  return table.concat(out), executed, #ops
end

-- (3) compile to Lua source
local function compile(code)
  local ops = parse(code)
  local src = { "local input = ...", "local t = setmetatable({}, {__index = function() return 0 end})",
                "local p, ip, out = 0, 1, {}" }
  local indent = ""
  for _, op in ipairs(ops) do
    local kind, arg = op[1], op[2]
    local line
    if kind == "add" then line = string.format("t[p] = (t[p] + %d) %% 256", arg)
    elseif kind == "move" then line = string.format("p = p + %d", arg)
    elseif kind == "clear" then line = "t[p] = 0"
    elseif kind == "out" then line = "out[#out + 1] = string.char(t[p])"
    elseif kind == "in" then line = "t[p] = input and input:byte(ip) or 0; ip = ip + 1"
    elseif kind == "jz" then line = "while t[p] ~= 0 do"
    elseif kind == "jnz" then indent = indent:sub(3); line = "end" end
    src[#src + 1] = indent .. line
    if kind == "jz" then indent = indent .. "  " end
  end
  src[#src + 1] = "return table.concat(out)"
  local text = table.concat(src, "\n")
  local fn, err = load(text, "=bf", "t")
  if not fn then error(err) end
  return fn, #src
end

-- generate a BF program that prints a given message (cell 0 is a counter)
local function generate(msg)
  local parts, cur = {}, 0
  for i = 1, #msg do
    local target = msg:byte(i)
    local diff = target - cur
    local sign = diff >= 0 and "+" or "-"
    local mag = math.abs(diff)
    if mag > 12 then
      local f = math.floor(math.sqrt(mag))
      local q, r = mag // f, mag % f
      parts[#parts + 1] = "<" .. string.rep("+", f) .. "[>" .. string.rep(sign, q) .. "<-]>" .. string.rep(sign, r)
    else
      parts[#parts + 1] = string.rep(sign, mag)
    end
    parts[#parts + 1] = "."
    cur = target
  end
  return ">" .. table.concat(parts)
end

local programs = {
  { name = "hello",
    code = "++++++++[>++++[>++>+++>+++>+<<<<-]>+>+>->>+[<]<-]>>.>---.+++++++..+++.>>.<-.<.+++.------.--------.>>+.>++." },
  { name = "generated", code = generate("Lua 5.4 runs Brainfuck: OK!\n") },
  { name = "reverse", code = ">,[>,]<[.<]", input = "!desrever saw txet sihT" },
  -- prints 'A' * 3 via nested multiplication (5*13 = 65), then a newline
  { name = "nested", code = "+++++[>+++++++++++++<-]>...[-]++++++++++." },
  { name = "comments", code = "this is a comment +++ [ > ++ < - ] > ." .. string.rep("+", 59) .. ". done" },
  -- decrementing a zero cell wraps to 255; then clear and print '0'
  { name = "wrap", code = "-.+[-]" .. string.rep("+", 48) .. "." },
}

for _, prog in ipairs(programs) do
  local out1, steps = run_simple(prog.code, prog.input)
  local out2, executed, nops = run_fast(prog.code, prog.input)
  local fn, lines = compile(prog.code)
  local out3 = fn(prog.input)
  local shown = out1:gsub("[%c\128-\255]", function(c) return string.format("\\%d", c:byte()) end)
  print(string.format("%-10s %q", prog.name, shown))
  print(string.format("           chars=%d steps=%d folded_ops=%d executed=%d lua_lines=%d same=%s",
    #out1, steps, nops, executed, lines, tostring(out1 == out2 and out2 == out3)))
end

print("generated length:", #generate("Lua 5.4 runs Brainfuck: OK!\n"))

for _, bad in ipairs({"[[]", "[]]", "+[>+<-", "]"}) do
  local ok1, e1 = pcall(run_simple, bad)
  local ok2, e2 = pcall(run_fast, bad)
  print("bad", bad, ok1, e1, ok2, e2)
end

-- a compiled program's chunkname shows up in runtime errors
local fn = load("local t = nil\nreturn t.x", "=bf")
local ok, err = pcall(fn)
print("chunkname error:", ok, err)
