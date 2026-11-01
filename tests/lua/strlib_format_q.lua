-- string.format: %q quoting of strings, integers, floats and other values
local function esc(s) return (s:gsub("[\128-\255]", function(c) return string.format("<%02X>", c:byte()) end)) end
local function q(label, v)
  local ok, r = pcall(string.format, "%q", v)
  if ok then print(label, esc(r)) else print(label, "error: " .. r) end
end

q("plain", "hello")
q("empty", "")
q("dquote", 'say "hi"')
q("squote", "it's")
q("backslash", "a\\b")
q("newline", "line1\nline2")
q("cr", "a\rb")
q("tab", "a\tb")
q("nul", "a\0b")
q("nul digit", "\0001")
q("ctrl digit", "\0019")
q("ctrl letter", "\1a")
q("bell", "\a")
q("del", "\127")
q("high bytes", "\128\200\255")
q("all ctrl", "\1\2\3\4\5\6\7\8\9\10\11\12\13\14\15\16\17\18\19\20\21\22\23\24\25\26\27\28\29\30\31")
q("utf8", "caf\u{E9}")
q("trailing nl", "x\n")
q("crlf", "\r\n")

-- non-string values
q("int 0", 0)
q("int 42", 42)
q("int -7", -7)
q("maxint", math.maxinteger)
q("minint", math.mininteger)
q("float 1.0", 1.0)
q("float 0.5", 0.5)
q("float 3.0", 3.0)
q("float -2.5", -2.5)
q("float 0.1", 0.1)
q("float 0.0", 0.0)
q("float -0.0", -0.0)
q("float 1e100", 1e100)
q("float 2^63", 2^63)
q("float 2^53", 2^53)
q("float huge", math.huge)
q("float -huge", -math.huge)
q("float nan", 0 / 0)
q("true", true)
q("false", false)
q("nil", nil)
q("table", {})
q("function", print)
print(pcall(string.format, "%q"))
print(pcall(string.format, "%10q", "x"))
print(pcall(string.format, "%-q", "x"))
print(pcall(string.format, "%.3q", "x"))

-- every %q output reads back to the original value
local function roundtrip(v)
  local src = "return " .. string.format("%q", v)
  local back = load(src)()
  return back == v or (v ~= v and back ~= back), math.type(back) == math.type(v)
end
local allbytes = {}
for i = 0, 255 do allbytes[#allbytes + 1] = string.char(i) end
local s = table.concat(allbytes)
print("all bytes", roundtrip(s), roundtrip(s:reverse()), roundtrip(s .. "0123"))
for _, v in ipairs{0, -1, math.maxinteger, math.mininteger, 1.0, -0.5, 0.1, 1 / 3, 2^63, -2^63,
                   1e308, 2^-1022, math.huge, -math.huge, math.pi} do
  print("roundtrip", v, roundtrip(v))
end
print("roundtrip nan", roundtrip(0 / 0))
-- %q mixed with other conversions
print(string.format("x=%q, y=%d, z=%q", "a\nb", 5, 1.5))
