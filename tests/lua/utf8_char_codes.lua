-- utf8: char, charpattern, codes and codepoint (strict and lax)
local function hex(s) return (s:gsub(".", function(c) return string.format("%02X ", c:byte()) end)) end
local function try(label, f, ...)
  local r = table.pack(pcall(f, ...))
  if not r[1] then print(label, "error: " .. r[2]); return end
  local out = {}
  for i = 2, r.n do out[#out + 1] = type(r[i]) == "string" and "[" .. hex(r[i]) .. "]" or tostring(r[i]) end
  print(label, table.concat(out, " "))
end

-- utf8.char
for _, cp in ipairs{0, 0x41, 0x7F, 0x80, 0x7FF, 0x800, 0xD7FF, 0xD800, 0xDFFF, 0xFFFF, 0x10000,
                    0x10FFFF, 0x110000, 0x1FFFFF, 0x200000, 0x3FFFFFF, 0x4000000, 0x7FFFFFFF} do
  try(string.format("char U+%X", cp), utf8.char, cp)
end
try("char multi", utf8.char, 72, 0xE9, 0x20AC, 0x1F600)
try("char none", utf8.char)
try("char -1", utf8.char, -1)
try("char 0x80000000", utf8.char, 0x80000000)
try("char 65.0", utf8.char, 65.0)
try("char '66'", utf8.char, "66")
try("char 1.5", utf8.char, 1.5)
try("char 2nd bad", utf8.char, 65, -5)
try("char string", utf8.char, "x")

-- charpattern
print("charpattern", (utf8.charpattern:gsub("[^\32-\126]", function(c) return string.format("\\x%02X", c:byte()) end)))
local n = 0
for c in ("h\u{E9}\u{20AC}\u{1F600}"):gmatch(utf8.charpattern) do n = n + 1; io.write("[", hex(c), "]") end
print(" count", n)

-- utf8.codes
local s = "h\u{E9}llo\u{20AC}\u{1F600}"
for p, c in utf8.codes(s) do io.write(p, ":", string.format("%X", c), " ") end
print()
for p, c in utf8.codes("") do print("never", p, c) end
local function codes(str, lax)
  local out = {}
  local ok, err = pcall(function()
    for p, c in utf8.codes(str, lax) do out[#out + 1] = p .. ":" .. string.format("%X", c) end
  end)
  print("codes [" .. hex(str) .. "]", lax and "lax" or "strict", table.concat(out, " "), ok and "ok" or "error: " .. err)
end
codes("ab\xffcd")
codes("a\x80")
codes("\xC0\x80")
codes("\xE0\x80\x80")
codes("\xC3")
codes("a\xC3")
codes("\xC3\xA9\xA9")
codes("\u{D800}")
codes("\u{D800}", true)
codes("\u{110000}")
codes("\u{110000}", true)
codes("\u{7FFFFFFF}", true)
codes("\xF4\x90\x80\x80", true)
codes("\xF8\x88\x80\x80\x80", true)
codes("\xFE")
codes("\0a\0")

-- utf8.codepoint
local t = "a\u{E9}\u{20AC}\u{1F600}z"   -- byte offsets: a=1 é=2 €=4 😀=7 z=11
try("codepoint()", utf8.codepoint, t)
try("codepoint(2)", utf8.codepoint, t, 2)
try("codepoint(1,-1)", utf8.codepoint, t, 1, -1)
try("codepoint(4,7)", utf8.codepoint, t, 4, 7)
try("codepoint(-1)", utf8.codepoint, t, -1)
try("codepoint(3)", utf8.codepoint, t, 3)
try("codepoint(5,4)", utf8.codepoint, t, 5, 4)
try("codepoint(0)", utf8.codepoint, t, 0)
try("codepoint(12)", utf8.codepoint, t, 12)
try("codepoint(1,12)", utf8.codepoint, t, 1, 12)
try("codepoint(-11)", utf8.codepoint, t, -11)
try("codepoint(-12)", utf8.codepoint, t, -12)
try("codepoint(11,11)", utf8.codepoint, t, 11, 11)
try("codepoint empty", utf8.codepoint, "")
try("codepoint empty 1,0", utf8.codepoint, "", 1, 0)
try("codepoint surrogate", utf8.codepoint, "\u{DFFF}")
try("codepoint surrogate lax", utf8.codepoint, "\u{DFFF}", 1, 1, true)
try("codepoint big lax", utf8.codepoint, "\u{7FFFFFFF}", 1, 1, true)
try("codepoint big strict", utf8.codepoint, "\u{7FFFFFFF}")
try("codepoint 10FFFF", utf8.codepoint, "\u{10FFFF}")
try("codepoint overlong", utf8.codepoint, "\xC1\x81", 1, 1, true)
try("codepoint truncated", utf8.codepoint, "\xE2\x82")
try("codepoint number", utf8.codepoint, 12, 1, 2)
