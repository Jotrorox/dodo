-- string.pack/unpack: integer formats, sizes, endianness and overflow checks
local pack, unpack = string.pack, string.unpack
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function p(fmt, ...)
  local ok, r = pcall(pack, fmt, ...)
  if ok then print("pack", fmt, #r, hex(r)) else print("pack", fmt, "error: " .. r) end
end
local function u(fmt, data, pos)
  local r = table.pack(pcall(unpack, fmt, data, pos))
  if not r[1] then print("unpack", fmt, "error: " .. r[2]); return end
  local out = {}
  for i = 2, r.n do out[#out + 1] = tostring(r[i]) .. "(" .. math.type(r[i]) .. ")" end
  print("unpack", fmt, table.concat(out, " "))
end
local maxi, mini = math.maxinteger, math.mininteger

p("<i4", 1)
p(">i4", 1)
p("<i4", -1)
p("<i2", -2)
p(">i2", 0x1234)
p("b", -128)
p("b", 127)
p("B", 255)
p("B", 0)
p("<h", 0x1234)
p("<H", 0xffff)
p("<i3", -1)
p(">I3", 0xabcdef)
p("<j", -1)
p("<j", maxi)
p(">J", mini)
p("<J", -1)
p("<i8", mini)
p("<i16", -1)
p(">i16", 1)
p("<I16", 255)
p("<i9", -2)
p("<i5", 0x7fffffffff)
p("<I1", 200)
p("<i", 7)
p("<I", 7)
p("<l", -3)
p("<L", 3)
p("<T", 5)
p("<i1i2i4", 1, 2, 3)
p(">!4 i3", 1)
p("<b", 3.0)
p("<b", "10")

-- sizes
for _, f in ipairs{"b", "B", "h", "H", "i", "I", "l", "L", "j", "J", "T", "i1", "i7", "i16", "f", "d", "n"} do
  io.write(f, "=", string.packsize(f), " ")
end
print()

-- overflow checks
p("b", 128)
p("b", -129)
p("B", 256)
p("B", -1)
p("<i2", 32768)
p("<i2", -32769)
p("<I2", 65536)
p("<i3", 0x800000)
p("<i3", -0x800000)
p("<I3", -1)
p("<I8", -1)
p("<i8", maxi)
p("<b", 1.5)
p("<b", "x")
p("<b")
p("i17", 1)
p("i0", 1)
p("I99", 1)

-- unpack
u("<i4", "\xff\xff\xff\xff")
u("<I4", "\xff\xff\xff\xff")
u(">i2", "\x80\x00")
u("<i2", "\x80\x00")
u("b", "\x80")
u("B", "\x80")
u("<i3", "\xff\xff\x7f")
u("<i3", "\x00\x00\x80")
u("<I8", ("\xff"):rep(8))
u("<i16", ("\xff"):rep(16))
u("<i16", "\x01" .. ("\x00"):rep(15))
u("<i16", ("\x00"):rep(8) .. "\x01" .. ("\x00"):rep(7))
u("<I16", ("\xff"):rep(8) .. ("\x00"):rep(8))
u("<i16", ("\xff"):rep(8) .. ("\x00"):rep(8))
u("<i9", ("\xff"):rep(9))
u("<i9", "\x01" .. ("\x00"):rep(8))
u("<I9", ("\xff"):rep(8) .. "\x00")
u("<I9", ("\xff"):rep(9))
u("<i9", ("\xff"):rep(8) .. "\x7f")
u("<i9", ("\x00"):rep(8) .. "\xff")
u(">i9", "\x00\x80" .. ("\x00"):rep(7))
u("<i4", "\1\0\0")
u("<i2 i2", "\1\0\2\0")
u("<i2", "\1\0\2\0", 3)
u("<i2", "\1\0\2\0", -2)
u("<i2", "\1\0\2\0", 5)
u("<i2", "\1\0\2\0", 6)
u("<i2", "\1\0\2\0", 0)
u("<i2", "\1\0\2\0", -10)

-- round trips
for _, v in ipairs{0, 1, -1, 127, -128, 32767, mini, maxi, 0x123456789} do
  for _, f in ipairs{"<j", ">j", "<i8", ">i16", "=j"} do
    assert(unpack(f, pack(f, v)) == v, f .. " " .. v)
  end
end
print("round trips ok", pack("=i4", 1) == pack("<i4", 1) or pack("=i4", 1) == pack(">i4", 1))
