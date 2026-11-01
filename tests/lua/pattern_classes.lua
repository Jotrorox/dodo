-- Patterns: character classes, their complements, and bracket sets
local bytes = {}
for i = 0, 255 do bytes[#bytes + 1] = string.char(i) end
local all = table.concat(bytes)
local function vis(s)
  return (s:gsub("[^\33-\126]", function(c) return string.format("<%d>", c:byte()) end))
end
local function members(pat)
  local out = {}
  for c in all:gmatch(pat) do out[#out + 1] = c end
  return table.concat(out)
end

-- each class, its count over all 256 bytes, and the matching bytes (C locale)
for _, cl in ipairs{"a", "c", "d", "g", "l", "p", "s", "u", "w", "x"} do
  local m = members("%" .. cl)
  local M = members("%" .. cl:upper())
  print("%" .. cl, #m, #M, #m + #M == 256, vis(m))
end
print(". matches all", #members("."))

-- bracket sets
local function set(pat) print(string.format("%-12s", vis(pat)), vis(members(pat))) end
set("[a-f]")
set("[a-cx-z]")
set("[%d_]")
set("[%a%d]")
set("[%]]")
set("[]]")
set("[^]]")
set("[a-]")
set("[-a]")
set("[%a-z]")
set("[z-a]")
set("[%-]")
set("[%%]")
set("[.]")
set("[%.]")
set("[+*?]")
set("[%^]")
set("[a^]")
set("[0-9A-F]")
set("[%s%p]")
set("[^%w%s%p]")
set("[\1-\31]")
set("[%z]")
set("[%x-]")
set("[^%a]")
set("[%W]")
set("[^%W]")
set("[a%-z]")
set("[%[]")
set("[[]")
set("[*-/]")

-- classes inside other constructs
print(("abc123def"):match("^%a+"), ("abc123def"):match("%d+"), ("  x"):match("^%s*()"))
print(("hello_world 42"):gsub("%w", "."))
print(("Hello World"):gsub("%u", "_"))
print(("a,b;c.d"):gsub("%p", ""))
print(("tab\there"):gsub("%c", "?"))
print(("0x1F zz"):match("0x(%x+)"))
print("%z is a deprecated alias for \\0:", ("no-z-here zz\0"):find("%z"))
print(("A\0B"):find("[%z]"))
-- %g = printable except space
print(("a b\tc"):gsub("%g", "#"))
-- unknown letter class escapes match the letter itself
print(("qQ"):find("%q"), ("eE"):find("%E"))
