-- String library: sub, byte, char, rep, reverse, upper, lower, len
local function esc(s) return (s:gsub("[^\32-\126]", function(c) return string.format("\\x%02X", c:byte()) end)) end
local function try(label, f, ...)
  local r = table.pack(pcall(f, ...))
  if not r[1] then print(label, "error", r[2]); return end
  local out = {}
  for i = 2, r.n do out[#out + 1] = type(r[i]) == "string" and "'" .. esc(r[i]) .. "'" or tostring(r[i]) end
  print(label, "n=" .. (r.n - 1), table.concat(out, " "))
end
local mini, maxi = math.mininteger, math.maxinteger

-- sub
local s = "hello"
for _, a in ipairs{{2}, {-3}, {2, 4}, {0}, {-100, 2}, {4, 2}, {2, 100}, {-1, -1}, {6}, {5},
                   {0, 0}, {mini, maxi}, {3, -3}, {-5, -4}, {maxi}, {mini}, {1, mini}, {-2, maxi}} do
  try("sub(" .. table.concat(a, ",") .. ")", string.sub, s, table.unpack(a))
end
try("sub empty", string.sub, "", 1, 1)
try("sub number", string.sub, 12345, 2, 3)
try("sub no args", string.sub)
try("sub bad i", string.sub, "x", "a")
try("method sub bad", function() return s:sub({}) end)

-- byte
try("byte()", string.byte, s)
try("byte(1,-1)", string.byte, s, 1, -1)
try("byte(-1)", string.byte, s, -1)
try("byte(10)", string.byte, s, 10)
try("byte(0)", string.byte, s, 0)
try("byte(3,2)", string.byte, s, 3, 2)
try("byte(-100,2)", string.byte, s, -100, 2)
try("byte(4,100)", string.byte, s, 4, 100)
try("byte empty", string.byte, "")
try("byte high", string.byte, "\0\127\128\255", 1, -1)
try("byte 1.5", string.byte, s, 1.5)

-- char
try("char", string.char, 72, 105, 33)
try("char()", string.char)
try("char 0 255", string.char, 0, 255)
try("char 256", string.char, 256)
try("char -1", string.char, -1)
try("char 2nd bad", string.char, 65, 300)
try("char '65'", string.char, "65")
try("char 65.0", string.char, 65.0)
try("char 65.5", string.char, 65.5)
try("char 'x'", string.char, "x")
try("char nil", string.char, nil)
local all = {}
for i = 0, 255 do all[#all + 1] = i end
local bytes = string.char(table.unpack(all))
print("all bytes", #bytes, bytes:byte(1), bytes:byte(256), select("#", bytes:byte(1, -1)))

-- rep
try("rep 3", string.rep, "ab", 3)
try("rep sep", string.rep, "ab", 3, ",")
try("rep 0", string.rep, "ab", 0)
try("rep -1", string.rep, "ab", -1)
try("rep 1 sep", string.rep, "ab", 1, ",")
try("rep empty", string.rep, "", 5)
try("rep 0 sep", string.rep, "x", 0, ",")
try("rep empty sep", string.rep, "", 3, "-")
try("rep minint", string.rep, "x", mini)
try("rep huge", string.rep, "x", maxi)
try("rep huge sep", string.rep, "x", 2^40 // 1, "yy")
try("rep zero char", string.rep, "\0", 3, "\1")
try("rep no n", string.rep, "x")
try("rep number", string.rep, 7, 3, 0)

-- reverse, upper, lower, len
try("reverse", string.reverse, "abc")
try("reverse empty", string.reverse, "")
try("reverse zeros", string.reverse, "a\0b\255")
try("upper", string.upper, "Hello, World! 123 \xe9\0z")
try("lower", string.lower, "Hello, World! 123 \xC9\0Z")
try("upper num", string.upper, 12.5)
try("len", string.len, "a\0b")
try("len num", string.len, -1.5)
try("len table", string.len, {})
try("len nil", string.len)

-- method syntax and the string metatable
print(("x"):rep(3), ("abc"):len(), s:upper():lower():reverse())
print(getmetatable("").__index == string, ("abc").len == string.len)
print(#string.format("%s", s), s.len == string.len, (s.nonexistent))
try("index number with method", function() return (5):rep(2) end)
