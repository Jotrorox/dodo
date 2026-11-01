-- utf8: len (with invalid sequences) and offset with positive/negative n and i
local function try(label, f, ...)
  local r = table.pack(pcall(f, ...))
  if not r[1] then print(label, "error: " .. r[2]); return end
  local out = {}
  for i = 2, r.n do out[#out + 1] = tostring(r[i]) end
  print(label, "n=" .. (r.n - 1), table.concat(out, " "))
end
local s = "a\u{E9}\u{20AC}\u{1F600}z"   -- byte offsets: a=1 é=2 €=4 😀=7 z=11, #s=11

-- utf8.len
try("len", utf8.len, s)
try("len empty", utf8.len, "")
try("len ascii", utf8.len, "hello")
try("len i=2", utf8.len, s, 2)
try("len i=4 j=7", utf8.len, s, 4, 7)
try("len i=4 j=6", utf8.len, s, 4, 6)
try("len j=-1", utf8.len, s, 1, -1)
try("len i=-1", utf8.len, s, -1)
try("len i=-5", utf8.len, s, -5)
try("len i=12", utf8.len, s, 12)
try("len i=13", utf8.len, s, 13)
try("len i=0", utf8.len, s, 0)
try("len i=-12", utf8.len, s, -12)
try("len j=12", utf8.len, s, 1, 12)
try("len j=0", utf8.len, s, 1, 0)
try("len i>j", utf8.len, s, 5, 2)
try("len i=3 (continuation)", utf8.len, s, 3)
try("len invalid", utf8.len, "ab\xffcd")
try("len invalid first", utf8.len, "\x80")
try("len truncated", utf8.len, "abc\xE2\x82")
try("len overlong", utf8.len, "\xC0\xAF")
try("len surrogate", utf8.len, "x\u{D800}")
try("len surrogate lax", utf8.len, "x\u{D800}", 1, -1, true)
try("len big", utf8.len, "\u{7FFFFFFF}")
try("len big lax", utf8.len, "\u{7FFFFFFF}", 1, -1, true)
try("len 110000", utf8.len, "\u{110000}")
try("len zeros", utf8.len, "\0\0\0")
try("len number", utf8.len, 12345)
try("len nil", utf8.len, nil)

-- utf8.offset(s, n [, i])
for _, n in ipairs{1, 2, 3, 4, 5, 6, 7, 0, -1, -2, -5, -6} do
  try("offset n=" .. n, utf8.offset, s, n)
end
try("offset 0 at 3", utf8.offset, s, 0, 3)
try("offset 0 at 6", utf8.offset, s, 0, 6)
try("offset 0 at 7", utf8.offset, s, 0, 7)
try("offset 0 at 12", utf8.offset, s, 0, 12)
try("offset 1 at 4", utf8.offset, s, 1, 4)
try("offset 2 at 4", utf8.offset, s, 2, 4)
try("offset -1 at 4", utf8.offset, s, -1, 4)
try("offset -1 at 12", utf8.offset, s, -1, 12)
try("offset 1 at 12", utf8.offset, s, 1, 12)
try("offset 2 at 12", utf8.offset, s, 2, 12)
try("offset -1 at -1", utf8.offset, s, -1, -1)
try("offset 1 at -1", utf8.offset, s, 1, -1)
try("offset 1 at 3 (cont)", utf8.offset, s, 1, 3)
try("offset -1 at 5 (cont)", utf8.offset, s, -1, 5)
try("offset i=0", utf8.offset, s, 1, 0)
try("offset i=13", utf8.offset, s, 1, 13)
try("offset i=-12", utf8.offset, s, 1, -12)
try("offset empty 1", utf8.offset, "", 1)
try("offset empty 0", utf8.offset, "", 0)
try("offset empty -1", utf8.offset, "", -1)
try("offset ascii 3", utf8.offset, "abc", 3)
try("offset ascii 4", utf8.offset, "abc", 4)
try("offset ascii 5", utf8.offset, "abc", 5)
try("offset ascii -3", utf8.offset, "abc", -3)
try("offset ascii -4", utf8.offset, "abc", -4)
try("offset no n", utf8.offset, s)
try("offset invalid bytes", utf8.offset, "\xff\xfe", 2)

-- len and offset agree when walking a string
local walk = {}
local i = 1
while true do
  local p = utf8.offset(s, 2, i)
  if not p or p > #s then break end
  walk[#walk + 1] = p
  i = p
end
print("walk", table.concat(walk, " "), "len", utf8.len(s))
