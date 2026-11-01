-- file:read formats on a temporary file: "n" with many numeral forms
-- (hex, exponents, leading whitespace, signs, failures that consume
-- input), "l", "L", "a" (returns "" at EOF), byte counts (0 at EOF gives
-- nil), several formats in one call stopping at the first failure, and
-- the older "*l"/"*a" spellings.

local name = os.tmpname()
local function with_content(s)
  local f = assert(io.open(name, "wb"))
  f:write(s)
  f:close()
  return assert(io.open(name, "rb"))
end
local function show(...)
  local t = table.pack(...)
  for i = 1, t.n do
    local v = t[i]
    if math.type(v) then t[i] = math.type(v) .. ":" .. tostring(v)
    elseif type(v) == "string" then t[i] = string.format("%q", v)
    else t[i] = tostring(v) end
  end
  print(table.concat(t, " ", 1, t.n))
end

-- numbers
local f = with_content("  42 -7 3.5 0x1F 1e3 -0x10 .5 5. 0x.8p1 +12 1E-2\n\t 99")
for _ = 1, 12 do show(f:read("n")) end
show(f:read("n"))  -- EOF
f:close()

-- failing numerals: read returns fail (nil); what remains afterwards?
f = with_content("abc 12")
show(f:read("n"), f:read("a"))
f:close()
f = with_content("12abc")
show(f:read("n"), f:read("a"))
f:close()
f = with_content("0x")
show(f:read("n"), f:read("a"))
f:close()
f = with_content("1e+x")
show(f:read("n"), f:read("a"))
f:close()
f = with_content("--5")
show(f:read("n"), f:read("a"))
f:close()
f = with_content("9223372036854775807 9223372036854775808 0xffffffffffffffff")
show(f:read("n", "n", "n"))
f:close()

-- lines
f = with_content("first\nsecond\r\n\nlast without newline")
show(f:read("l"))
show(f:read("L"))
show(f:read("l"))
show(f:read("L"))
show(f:read("l"))
show(f:read("L"))
f:close()

-- "a" at EOF gives "", never nil
f = with_content("all of it\nreally")
show(f:read("a"))
show(f:read("a"))
show(f:read("a", "a"))
show(f:read("l"), f:read(0), f:read(5))
f:close()

-- counts
f = with_content("abcdefghij")
show(f:read(0))
show(f:read(3), f:read(0), f:read(4))
show(f:read(100))
show(f:read(0), f:read(1))
f:close()
f = with_content("")
show(f:read(0), f:read("a"), f:read("l"), f:read("n"))
f:close()

-- multiple formats, stopping at the first failure
f = with_content("10 20\nline\n")
show(f:read("n", "n", "l", "l", "l", "n"))
f:close()
f = with_content("x\n")
show(f:read("n", "l"))
f:close()

-- default format is "l"; old-style "*" prefixes accepted
f = with_content("one\ntwo\nthree\n")
show(f:read())
show(f:read("*l"), f:read("*L"))
show(f:read("*a"))
f:close()

-- invalid formats
f = with_content("x")
show(pcall(function() return f:read("x") end))
show(pcall(function() return f:read({}) end))
show(pcall(function() return f:read("l", true) end))
f:close()

-- reading from a file opened for writing only
f = assert(io.open(name, "w"))
local r1, r2, r3 = f:read("a")
show(r1, type(r2), type(r3))
f:close()
os.remove(name)
