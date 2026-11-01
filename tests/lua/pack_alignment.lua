-- string.pack/unpack: alignment (! X x), endianness switches, format option errors
local pack, unpack, packsize = string.pack, string.unpack, string.packsize
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function p(fmt, ...)
  local ok, r = pcall(pack, fmt, ...)
  if ok then print("pack", fmt, #r, hex(r)) else print("pack", fmt, "error: " .. r) end
end
local function sz(fmt)
  local ok, r = pcall(packsize, fmt)
  print("size", fmt, ok and r or "error: " .. r)
end

-- padding bytes
p("<b x b", 1, 2)
p("<xxx")
p("<i2 x i2", 1, 2)

-- default is no alignment
p("<b i4", 1, 2)
p("<b d", 1, 1.0)
-- "!" sets maximum alignment (default native) and aligns each item to min(size, max)
p("<! b i4", 1, 2)
p("<!4 b d", 1, 1.0)
p("<!8 b d", 1, 1.0)
p("<!2 b i8", 1, 2)
p("<!1 b i8", 1, 2)
p("<!16 b i16", 1, 2)
p("<!8 b i3", 1, 2)
p("<!8 b s1", 1, "x")
p("<!8 b z b", 1, "ab", 2)
p("<!8 b c3 i2", 1, "abc", 2)
-- "X" aligns to the size of the next option without consuming data
p("<!8 b Xi4 b", 1, 2)
p("<!8 b Xd", 1)
p("<!2 b Xi8 b", 1, 2)
p("<!8 Xi4", nil)
p("<! b X", 1)
p("<! b Xz", 1)
p("<! b Xs4", 1)
p("<! b Xc3", 1)
p("<!4 bXbXhXi", 1, 2)
-- alignment must be a power of 2
p("<!3 b i3", 1, 2)
p("<!8 b i6", 1, 2)
p("<!8 b Xi6", 1)

-- sizes with alignment
sz("<!8 b d")
sz("<!4 b d")
sz("! b i4 b")
sz("!8 i4 Xd")
sz("!8 b Xi16")
sz("<i3 !4 i3")
sz("x")
sz("Xi4")

-- endianness can change within a format
p("<i2 >i2 =i2", 0x102, 0x102, 0x102)
p(">i4 <i4", 1, 1)
print("native matches one", pack("=i4", 1) == pack("<i4", 1) or pack("=i4", 1) == pack(">i4", 1))

-- unpack honours alignment too
local data = pack("<!8 b d i2", 7, 2.5, -3)
print("aligned record", #data, unpack("<!8 b d i2", data))
print("X in unpack", unpack("<!4 b Xi4 i4", pack("<!4 b Xi4 i4", 1, 2)))

-- spaces are ignored; everything else unknown is an error
p("< i2   i2 ", 1, 2)
p("y", 1)
p("<i2 q", 1)
p("!17", 1)
p("!0 b", 1)
sz("i2 #")
print(pcall(unpack, "w", "abc"))
print(pcall(unpack, "i4", nil))
print(pcall(unpack, "i4", "abcd", 1.5))
