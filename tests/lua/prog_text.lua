-- Program test: string-heavy text processing. Word frequencies over an
-- embedded text (sorted by count desc, then word), greedy line wrapping and
-- justification, a CSV parser handling quoted fields with embedded commas,
-- doubled quotes and newlines, template substitution via gsub with function
-- and table replacements, plus a few classic string utilities (title case,
-- trimming, splitting, run-length encoding, caesar cipher, palindromes).

local text = [[
It was the best of times, it was the worst of times, it was the age of
wisdom, it was the age of foolishness, it was the epoch of belief, it was
the epoch of incredulity, it was the season of Light, it was the season of
Darkness, it was the spring of hope, it was the winter of despair, we had
everything before us, we had nothing before us, we were all going direct to
Heaven, we were all going direct the other way -- in short, the period was so
far like the present period, that some of its noisiest authorities insisted
on its being received, for good or for evil, in the superlative degree of
comparison only.
]]

-- word frequencies ------------------------------------------------------

local freq, total, distinct = {}, 0, 0
for word in text:lower():gmatch("%a+") do
  total = total + 1
  if not freq[word] then distinct = distinct + 1 end
  freq[word] = (freq[word] or 0) + 1
end
local words = {}
for w in pairs(freq) do words[#words + 1] = w end
table.sort(words, function(a, b)
  if freq[a] ~= freq[b] then return freq[a] > freq[b] end
  return a < b
end)
print(string.format("total=%d distinct=%d", total, distinct))
for i = 1, 12 do
  print(string.format("%2d. %-8s %2d %s", i, words[i], freq[words[i]], string.rep("*", freq[words[i]])))
end
local longest = ""
for _, w in ipairs(words) do
  if #w > #longest or (#w == #longest and w < longest) then longest = w end
end
print("longest word:", longest, #longest)

-- line wrapping ------------------------------------------------------------

local function wrap(s, width)
  local lines, line = {}, ""
  for word in s:gmatch("%S+") do
    if line == "" then
      line = word
    elseif #line + 1 + #word <= width then
      line = line .. " " .. word
    else
      lines[#lines + 1] = line
      line = word
    end
  end
  if line ~= "" then lines[#lines + 1] = line end
  return lines
end

local function justify(line, width)
  local ws = {}
  for w in line:gmatch("%S+") do ws[#ws + 1] = w end
  if #ws == 1 then return line end
  local letters = #line:gsub("%s", "")
  local gaps = #ws - 1
  local spaces = width - letters
  local out = {}
  for i, w in ipairs(ws) do
    out[#out + 1] = w
    if i <= gaps then
      local n = spaces // gaps + ((i <= spaces % gaps) and 1 or 0)
      out[#out + 1] = string.rep(" ", n)
    end
  end
  return table.concat(out)
end

local wrapped = wrap(text, 34)
for i, l in ipairs(wrapped) do
  local shown = i < #wrapped and justify(l, 34) or l
  print(string.format("|%-34s| %2d", shown, #l))
end

-- CSV parsing ------------------------------------------------------------

local function parse_csv(src)
  local rows, row, field = {}, {}, {}
  local i, n = 1, #src
  local in_quotes = false
  while i <= n do
    local c = src:sub(i, i)
    if in_quotes then
      if c == '"' then
        if src:sub(i + 1, i + 1) == '"' then
          field[#field + 1] = '"'
          i = i + 1
        else
          in_quotes = false
        end
      else
        field[#field + 1] = c
      end
    elseif c == '"' then
      in_quotes = true
    elseif c == "," then
      row[#row + 1] = table.concat(field); field = {}
    elseif c == "\n" then
      row[#row + 1] = table.concat(field); field = {}
      rows[#rows + 1] = row; row = {}
    elseif c ~= "\r" then
      field[#field + 1] = c
    end
    i = i + 1
  end
  if in_quotes then error("unterminated quoted field", 0) end
  if #field > 0 or #row > 0 then
    row[#row + 1] = table.concat(field)
    rows[#rows + 1] = row
  end
  return rows
end

local csv = 'name,city,quote,amount\r\n' ..
  'Alice,"Portland, OR","She said ""hi""",12.50\n' ..
  'Bob,Berlin,,7\n' ..
  '"Carol ""CJ"" Jones","São Paulo","multi\nline",100\n' ..
  ',,,\n' ..
  'Dave,"",plain,-3.25'
local rows = parse_csv(csv)
print("rows:", #rows)
local sum = 0
for r, row in ipairs(rows) do
  local shown = {}
  for c, f in ipairs(row) do shown[c] = string.format("%q", f):gsub("\n", "n") end
  print(r, #row, table.concat(shown, " | "))
  if r > 1 then sum = sum + (tonumber(row[4]) or 0) end
end
print("amount sum:", sum)
print("unterminated:", pcall(parse_csv, 'a,"b\nc'))

-- template substitution ----------------------------------------------------

local vars = { name = "World", count = 3, item = "apple", price = 0.5 }
local tpl = "Hello, ${name}! You have ${count} ${item}s costing $${price} each. ${missing} ${ name }"
local result, nsubs = tpl:gsub("%${(%s*)([%w_]+)(%s*)}", function(pre, key, post)
  if pre ~= "" or post ~= "" then return "{bad:" .. key .. "}" end
  local v = vars[key]
  if v == nil then return nil end -- keep original text
  return tostring(v)
end)
print(result, nsubs)
print(("$a and $b and $c"):gsub("%$(%a)", {a = "ALPHA", b = false, c = 42}))
print(("x = 1 + 2"):gsub("%s", "%%"), ("hello"):gsub("l", "%0%0"), ("abc"):gsub("", "-"))
print(("key=value; k2=v2"):gsub("(%w+)=(%w+)", "%2=%1"))
print(("hello world"):gsub("o", "0", 1))

-- misc utilities -----------------------------------------------------------

local function title(s) return (s:gsub("(%a)([%w']*)", function(a, b) return a:upper() .. b:lower() end)) end
local function trim(s) return (s:match("^%s*(.-)%s*$")) end
local function split(s, sep)
  local out = {}
  for piece in (s .. sep):gmatch("(.-)" .. sep:gsub("%p", "%%%0")) do out[#out + 1] = piece end
  return out
end
local function rle(s)
  local out = {}
  local i = 1
  while i <= #s do
    local c = s:sub(i, i)
    local j = i
    while s:sub(j + 1, j + 1) == c do j = j + 1 end
    out[#out + 1] = (j - i + 1 > 1 and tostring(j - i + 1) or "") .. c
    i = j + 1
  end
  return table.concat(out)
end
local function caesar(s, k)
  return (s:gsub("%a", function(c)
    local base = c:match("%l") and 97 or 65
    return string.char((c:byte() - base + k) % 26 + base)
  end))
end
local function is_palindrome(s)
  s = s:lower():gsub("[^%a]", "")
  return s == s:reverse()
end

print(title("the QUICK brown fox's tale"))
print("[" .. trim("  \t padded text \n ") .. "]", "[" .. trim("   ") .. "]")
print(table.concat(split("a.b..c.", "."), "|"), #split("a.b..c.", "."))
print(table.concat(split("one, two,three", ", "), "|"))
print(rle("aaabccddddde"), rle(""), rle("x"))
print(caesar("Hello, World!", 3), caesar(caesar("Round Trip", 13), 13))
print(is_palindrome("A man, a plan, a canal: Panama"), is_palindrome("not one"))
print(("%5.1f|%-6s|%06d|%x|%X|%o|%e|%g"):format(3.14159, "ab", 42, 255, 255, 8, 12345.678, 0.0001))
print(("CamelCaseIdentifierHere"):gsub("(%l)(%u)", "%1_%2"):lower())
print(string.format("%q", "tab\there\nnew \0 nul \"q\" \\ \1 \127"))
print(("  many   spaces  here "):gsub("%s+", " "))
print(("find me"):find("me"), ("find me"):find("m.", 1, true), ("a+b"):find("+", 1, true))
print(("2024-01-15"):match("(%d+)-(%d+)-(%d+)"))
print(("f(a(b)c)d"):match("%b()"), ("THE (quick) fox"):find("%f[%a]%a+%f[%A]"))
