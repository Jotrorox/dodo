-- Lexer: escape sequences in short strings
local function bytes(s)
  local t = {}
  for i = 1, #s do t[#t + 1] = string.format("%02X", s:byte(i)) end
  return #s .. ":" .. table.concat(t, " ")
end
local function show(label, s) print(label, bytes(s)) end

show("C escapes", "\a\b\f\n\r\t\v")
show("backslash quotes", "\\\"\'")
show("single quoted", '\'"\"')
show("decimal", "\65\066\0671")
show("decimal max", "\255\0\00\000")
show("decimal 3 digits only", "\1234")
show("decimal zero then digit", "\0009")
show("hex", "\x41\x6a\x6A\xff\x00")
show("utf8 1", "\u{41}")
show("utf8 0", "\u{0}")
show("utf8 leading zeros", "\u{0000000041}")
show("utf8 7FF", "\u{7FF}")
show("utf8 800", "\u{800}")
show("utf8 FFFF", "\u{FFFF}")
show("utf8 surrogate", "\u{D800}")
show("utf8 10000", "\u{10000}")
show("utf8 10FFFF", "\u{10FFFF}")
show("utf8 110000", "\u{110000}")
show("utf8 1FFFFF", "\u{1FFFFF}")
show("utf8 200000", "\u{200000}")
show("utf8 3FFFFFF", "\u{3FFFFFF}")
show("utf8 4000000", "\u{4000000}")
show("utf8 7FFFFFFF", "\u{7FFFFFFF}")
show("utf8 lowercase hex", "\u{e9}")
show("z skips spaces", "a\z     b")
show("z skips newlines", "a\z

      b")
show("z at end", "a\z   ")
show("z no space", "a\zb")
show("continuation", "a\
b")
show("embedded zero", "a\0b")
print("#embedded", #"a\0b\0", #"\0\0\0")

-- line continuations and \z with carriage returns via load
local srcs = {
  "return 'a\\\r\nb'",
  "return 'a\\\n\rb'",
  "return 'a\\\rb'",
  "return 'a\\\n\nb'",
  "return 'a\\z \r\n \t\v\f b'",
}
for i, src in ipairs(srcs) do
  local f, err = load(src, "=s")
  if f then show("load" .. i, f()) else print("load" .. i, (err:gsub("\n", "\\n"))) end
end

-- escapes inside long strings are not processed
show("long raw", [[\n\x41]])
-- equality of different spellings
print("\65" == "A", "\x41" == "A", "\u{41}" == "A", "\z
  A" == "A")
print("\u{E9}" == "\xC3\xA9", "\u{20AC}" == "\226\130\172")
