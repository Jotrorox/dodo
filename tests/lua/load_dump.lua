-- string.dump + load round trips: constants of every type, nested
-- functions, upvalues (fresh/nil after loading, settable via
-- debug.setupvalue), strip flag (line info removed), and load modes
-- "t", "b", "bt" with their error messages. Binary contents are never
-- printed; only behaviour of the reloaded functions.

local function roundtrip(f, strip)
  local s = string.dump(f, strip)
  assert(type(s) == "string")
  return assert(load(s, "dumped", "b"))
end

-- constants of every type
local function consts()
  return nil, true, false, 42, -7, math.maxinteger, math.mininteger,
         3.25, -0.0, 1e300, 2^-1074, "short", string.rep("long", 30), ""
end
local g = roundtrip(consts)
local r = table.pack(g())
for i = 1, r.n do
  local v = r[i]
  local shown = tostring(v)
  if math.type(v) == "integer" then shown = string.format("int(%d)", v)
  elseif math.type(v) == "float" then shown = string.format("flt(%.17g)", v) end
  if type(v) == "string" and #v > 20 then shown = "len" .. #v end
  io.write(shown, i < r.n and " " or "\n")
end
print(1 / select(9, g()))

-- dumped string starts with the signature "\27Lua"
local dumped = string.dump(consts)
print(dumped:sub(1, 4) == "\27Lua", dumped:byte(5) == 0x54)

-- nested functions and vararg
local function outer(...)
  local function inner(a, b) return a * b end
  local n = select("#", ...)
  return inner(n, 10), ...
end
print(roundtrip(outer)(1, 2, 3))

-- upvalues: after load they are fresh; load sets the FIRST upvalue to the
-- global table whatever its name, the others start as nil
local counter = 5
local function useup() counter = counter + 1; return counter end
local u = roundtrip(useup)
local n1, v1 = debug.getupvalue(u, 1)
print(n1, v1 == _G, (pcall(u)))
debug.setupvalue(u, 1, 100)
print(u(), u(), counter)
local function addup(a) local c = counter; return function() return a, c, print end end
local au = roundtrip(addup(7))
local names = {}
for i = 1, 3 do
  local nm, vv = debug.getupvalue(au, i)
  names[i] = tostring(nm) .. "=" .. (vv == _G and "_G" or tostring(vv))
end
print(table.concat(names, " "))
-- a function whose only upvalue is _ENV: the loaded copy gets the globals
local function useenv() return type(print) end
print(roundtrip(useenv)())
-- with two upvalues (neither is _ENV), only the first is set to _G
local x1, x2 = 1, 2
local function two() return x1, x2 end
local t2 = roundtrip(two)
local a1, b1 = debug.getupvalue(t2, 1)
local a2, b2 = debug.getupvalue(t2, 2)
print(a1, b1 == _G, a2, b2)

-- strip removes debug info (line numbers in error messages become ?)
local function failing()
  local x = nil
  return x.field
end
print(pcall(roundtrip(failing, false)))
print(pcall(roundtrip(failing, true)))
local info = debug.getinfo(roundtrip(failing, true), "S")
print("stripped source:", info.source, info.short_src)
info = debug.getinfo(roundtrip(failing, false), "S")
print("unstripped source:", info.source == debug.getinfo(failing, "S").source)
print(#string.dump(failing, true) < #string.dump(failing))

-- modes
local src = "return 'text chunk'"
print(load(src, "c", "t")())
print(load(src, "c", "bt")())
print(load(src, "c", "b"))
print(load(dumped, "c", "t"))
print(load(dumped, "c", "b") ~= nil, load(dumped, "c", "bt") ~= nil, load(dumped) ~= nil)
print(pcall(load, src, "c", "x"))
print(load(src, "c", ""))
-- binary chunk via a reader function
local pos = 0
local b = load(function()
  if pos >= #dumped then return nil end
  local piece = dumped:sub(pos + 1, pos + 7)
  pos = pos + 7
  return piece
end, "pieces", "b")
print(type(b), select(4, b()))

-- truncated / corrupted binary chunks
print(load(dumped:sub(1, 10), "trunc", "b"))
print(load(dumped:sub(1, 4), "short", "b"))
print(load("\27Lua\0", "badver", "b"))

-- dumping C functions is an error
print(pcall(string.dump, print))
print(pcall(string.dump, 1))
