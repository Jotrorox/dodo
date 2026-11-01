-- string.pack/unpack: float formats, length-prefixed, fixed and zero-terminated strings
local pack, unpack, packsize = string.pack, string.unpack, string.packsize
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function show(v)
  if type(v) == "number" then
    if v ~= v then return "nan" end
    return tostring(v) .. "(" .. math.type(v) .. ")"
  elseif type(v) == "string" then
    return "'" .. v:gsub("%z", "\\0") .. "'"
  end
  return tostring(v)
end
local function p(fmt, ...)
  local ok, r = pcall(pack, fmt, ...)
  if ok then print("pack", fmt, #r, hex(r)) else print("pack", fmt, "error: " .. r) end
end
local function u(fmt, data, pos)
  local r = table.pack(pcall(unpack, fmt, data, pos))
  if not r[1] then print("unpack", fmt, "error: " .. r[2]); return end
  local out = {}
  for i = 2, r.n do out[#out + 1] = show(r[i]) end
  print("unpack", fmt, table.concat(out, " "))
end

-- floats
p("<f", 1.5)
p(">f", 1.5)
p("<d", 1.5)
p(">d", -2.0)
p("<n", 0.1)
p("<d", -0.0)
p("<d", math.huge)
p("<f", -math.huge)
p("<f", 0.1)
p("<d", 3)
p("<f", "2.5")
p("<d", math.maxinteger)
p("<f", 1e300)
p("<d", "x")
u("<f", pack("<f", 0.1))
u("<d", pack("<d", 0.1))
u("<d", pack("<d", 3))
u("<f", pack("<f", 16777217))
u(">d", pack(">d", -0.0))
u("<d", ("\xff"):rep(8))
u("<f", "\0\0\128\127")
u("<f", "\0\0")

-- length-prefixed strings
p("<s1", "abc")
p("<s2", "abc")
p(">s2", "abc")
p("<s4", "")
p("<s", "hi")
p("<s1", ("x"):rep(255):sub(1, 3))
print("s1 255 ok", #pack("s1", ("x"):rep(255)))
p("<s1", ("x"):rep(256))
p("<s1", 12)
p("<s1", {})
u("<s1", "\3abcdef")
u("<s2", "\3\0abc")
u("<s1", "\5ab")
u("<s1", "")
u("<s1 s1", "\1a\2bc")
u("<s", pack("<s", "hello"))

-- fixed-size strings
p("c5", "ab")
p("c2", "ab")
p("c2", "abc")
p("c0", "")
p("c", "a")
p("c3c3", "ab", "cd")
u("c3", "abcdef")
u("c3", "ab")
u("c0", "abc")
u("c2c2", "abcd")

-- zero-terminated strings
p("z", "hello")
p("z", "")
p("z", "a\0b")
p("zz", "a", "b")
u("z", "hello\0world\0")
u("zz", "hello\0world\0")
u("z", "no terminator")
u("z", "\0")
u("z", "abc\0", 2)

-- mixed records, sizes and the returned position
local rec = pack("<i4 s1 d z B", 7, "name", 2.5, "tail", 255)
print("record", #rec, hex(rec))
u("<i4 s1 d z B", rec)
u("<i4", rec, 1)
u("<s1", rec, 5)
print("packsize", packsize("<i4 d B"), packsize("c10"), packsize(""), packsize("i1 i2 i4 i8"))
print("packsize s", pcall(packsize, "s"))
print("packsize z", pcall(packsize, "z"))
print("packsize s1", pcall(packsize, "i4s1"))
print("packsize too large", pcall(packsize, ("c1000000000"):rep(3)))
print("pack no args", pcall(pack))
print("pack extra args", hex(pack("b", 1, 2, 3)))
print("unpack empty fmt", unpack("", "abc"))
print("unpack empty fmt pos", unpack("", "abc", 4))
