-- debug.getlocal/setlocal (active locals by level, block scoping,
-- varargs via negative indices, parameters of non-active functions),
-- debug.getupvalue/setupvalue (names, _ENV as first upvalue of the main
-- chunk, C function upvalues), upvalueid and upvaluejoin.

local function locals(level)
  local out = {}
  local i = 1
  while true do
    local name, value = debug.getlocal(level + 1, i)
    if not name then break end
    if name:sub(1, 1) ~= "(" then out[#out + 1] = name .. "=" .. tostring(value) end
    i = i + 1
  end
  return table.concat(out, " ")
end

local function f(a, b)
  local c = a + b
  do
    local d = "inner"
    print("in block:", locals(1))
  end
  local e = true
  print("after block:", locals(1))
  return c
end
f(1, 2)

-- varargs: negative indices
local function va(x, ...)
  local names = {}
  for i = -1, -10, -1 do
    local n, v = debug.getlocal(1, i)
    if not n then break end
    names[#names + 1] = n .. "=" .. tostring(v)
  end
  return table.concat(names, " ")
end
print("varargs:", va("fixed", "v1", nil, "v3"))
print("no varargs:", va("fixed"))

-- setlocal
local function modify()
  local target = "old"
  local idx
  for i = 1, 10 do
    local n = debug.getlocal(1, i)
    if n == "target" then idx = i break end
  end
  print("setlocal returns:", debug.setlocal(1, idx, "new"))
  print("setlocal out of range:", debug.setlocal(1, 100, "x"))
  return target
end
print("modified:", modify())
local function setva(...)
  debug.setlocal(1, -2, "changed")
  return ...
end
print("setlocal vararg:", setva("a", "b", "c"))

-- parameters of a non-active function
local function params(alpha, beta, ...) local gamma end
for i = 1, 4 do io.write(tostring(debug.getlocal(params, i)), " ") end
print()
print(debug.getlocal(print, 1))

-- getlocal at a caller level
local function inner() return debug.getlocal(2, 1) end
local function outerf(param) local r1, r2 = inner(); return r1, r2 end
print("caller level:", outerf("P"))

-- errors
print(pcall(debug.getlocal, 100, 1))
print(pcall(debug.setlocal, 100, 1, 0))
print(pcall(debug.getlocal, {}, 1))

-- upvalues
local up_a, up_b = 10, 20
local function useboth() return up_a + up_b end
print(debug.getupvalue(useboth, 1))
print(debug.getupvalue(useboth, 2))
print(debug.getupvalue(useboth, 3))
print(debug.getupvalue(useboth, 0))
print(debug.setupvalue(useboth, 2, 5), useboth(), up_b)
print(debug.setupvalue(useboth, 9, 5))

-- the main chunk's single upvalue is _ENV
local main = debug.getinfo(1, "f").func
local n1, v1 = debug.getupvalue(main, 1)
print("main upvalue:", n1, v1 == _ENV, debug.getupvalue(main, 2))

-- C functions: upvalue names are empty strings
local cf = string.gmatch("a b", "%a+")
local cn = debug.getupvalue(cf, 1)
print("C upvalue name:", string.format("%q", tostring(cn)))
print("print upvalues:", debug.getupvalue(print, 1))

-- upvalueid / upvaluejoin
local x, y = "x", "y"
local function gx() return x end
local function gx2() return x end
local function gy() return y end
print(debug.upvalueid(gx, 1) == debug.upvalueid(gx2, 1), debug.upvalueid(gx, 1) == debug.upvalueid(gy, 1))
print(type(debug.upvalueid(gx, 1)))
debug.upvaluejoin(gx, 1, gy, 1)
print(gx(), gx2(), debug.upvalueid(gx, 1) == debug.upvalueid(gy, 1))
y = "y changed"
print(gx(), gy())
print(pcall(debug.upvalueid, gx, 5))
print(pcall(debug.upvaluejoin, gx, 1, print, 1))
print(pcall(debug.upvaluejoin, gx, 3, gy, 1))
