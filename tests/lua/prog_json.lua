-- Program test: a self-contained JSON encoder/decoder. Exercises string
-- scanning with string.byte/sub/find, \u escapes (including surrogate pairs
-- encoded to UTF-8 via utf8.char), integer vs float number handling via
-- math.type and math.tointeger, recursion over nested tables, sorted object
-- keys on output, deep structural equality, and error reporting for
-- malformed input through error()/pcall with position information.

local json = {}

local escapes = {
  ['"'] = '\\"', ['\\'] = '\\\\', ['\b'] = '\\b', ['\f'] = '\\f',
  ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t',
}

local function encode_string(s)
  return '"' .. s:gsub('[%c"\\]', function(c)
    return escapes[c] or string.format("\\u%04x", c:byte())
  end) .. '"'
end

local function is_array(t)
  local n = 0
  for k in pairs(t) do
    if math.type(k) ~= "integer" or k < 1 then return false end
    n = n + 1
  end
  for i = 1, n do
    if t[i] == nil then return false end
  end
  return true, n
end

local encode_value

local function encode_table(t, out, stack)
  if stack[t] then error("cycle detected", 0) end
  stack[t] = true
  local arr, n = is_array(t)
  if arr and (n > 0 or getmetatable(t) ~= json.object_mt) then
    out[#out + 1] = "["
    for i = 1, n do
      if i > 1 then out[#out + 1] = "," end
      encode_value(t[i], out, stack)
    end
    out[#out + 1] = "]"
  else
    local keys = {}
    for k in pairs(t) do
      if type(k) ~= "string" then error("non-string key: " .. tostring(k), 0) end
      keys[#keys + 1] = k
    end
    table.sort(keys)
    out[#out + 1] = "{"
    for i, k in ipairs(keys) do
      if i > 1 then out[#out + 1] = "," end
      out[#out + 1] = encode_string(k)
      out[#out + 1] = ":"
      encode_value(t[k], out, stack)
    end
    out[#out + 1] = "}"
  end
  stack[t] = nil
end

function encode_value(v, out, stack)
  local tv = type(v)
  if v == json.null then
    out[#out + 1] = "null"
  elseif tv == "nil" then
    out[#out + 1] = "null"
  elseif tv == "boolean" then
    out[#out + 1] = tostring(v)
  elseif tv == "number" then
    if v ~= v then error("cannot encode NaN", 0) end
    if v == math.huge or v == -math.huge then
      error("cannot encode " .. (v > 0 and "inf" or "-inf"), 0)
    end
    if math.type(v) == "integer" then
      out[#out + 1] = string.format("%d", v)
    else
      local s = string.format("%.14g", v)
      if not s:find("[%.eE]") then s = s .. ".0" end
      out[#out + 1] = s
    end
  elseif tv == "string" then
    out[#out + 1] = encode_string(v)
  elseif tv == "table" then
    encode_table(v, out, stack)
  else
    error("cannot encode type " .. tv, 0)
  end
end

json.null = setmetatable({}, {__tostring = function() return "null" end})
json.object_mt = {}

function json.encode(v)
  local out = {}
  encode_value(v, out, {})
  return table.concat(out)
end

-- decoder ---------------------------------------------------------------

local Decoder = {}
Decoder.__index = Decoder

function Decoder:fail(msg)
  error(string.format("json: %s at position %d", msg, self.pos), 0)
end

function Decoder:skip()
  self.pos = self.src:find("[^ \t\r\n]", self.pos) or #self.src + 1
end

function Decoder:peek()
  return self.src:sub(self.pos, self.pos)
end

function Decoder:literal(word, value)
  if self.src:sub(self.pos, self.pos + #word - 1) ~= word then
    self:fail("invalid literal")
  end
  self.pos = self.pos + #word
  return value
end

function Decoder:number()
  local s, e = self.src:find("^-?%d+%.?%d*[eE]?[-+]?%d*", self.pos)
  if not s then self:fail("invalid number") end
  local text = self.src:sub(s, e)
  local n = math.tointeger(tonumber(text))
  if not text:find("[%.eE]") and n then
    self.pos = e + 1
    return n
  end
  local f = tonumber(text)
  if not f then self:fail("invalid number '" .. text .. "'") end
  self.pos = e + 1
  return f + 0.0
end

function Decoder:hex4()
  local h = self.src:sub(self.pos, self.pos + 3)
  if not h:find("^%x%x%x%x$") then self:fail("bad \\u escape") end
  self.pos = self.pos + 4
  return tonumber(h, 16)
end

local simple = { b = "\b", f = "\f", n = "\n", r = "\r", t = "\t",
                 ['"'] = '"', ['\\'] = '\\', ['/'] = '/' }

function Decoder:string()
  self.pos = self.pos + 1 -- opening quote
  local parts = {}
  while true do
    local s, e = self.src:find('^[^"\\%c]+', self.pos)
    if s then
      parts[#parts + 1] = self.src:sub(s, e)
      self.pos = e + 1
    end
    local c = self:peek()
    if c == '"' then
      self.pos = self.pos + 1
      return table.concat(parts)
    elseif c == "\\" then
      local esc = self.src:sub(self.pos + 1, self.pos + 1)
      self.pos = self.pos + 2
      if esc == "u" then
        local cp = self:hex4()
        if cp >= 0xD800 and cp <= 0xDBFF then
          if self.src:sub(self.pos, self.pos + 1) ~= "\\u" then
            self:fail("lone high surrogate")
          end
          self.pos = self.pos + 2
          local lo = self:hex4()
          if lo < 0xDC00 or lo > 0xDFFF then self:fail("bad low surrogate") end
          cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
        end
        parts[#parts + 1] = utf8.char(cp)
      elseif simple[esc] then
        parts[#parts + 1] = simple[esc]
      else
        self.pos = self.pos - 1
        self:fail("invalid escape '\\" .. esc .. "'")
      end
    elseif c == "" then
      self:fail("unterminated string")
    else
      self:fail("control character in string")
    end
  end
end

function Decoder:value()
  self:skip()
  local c = self:peek()
  if c == "{" then
    self.pos = self.pos + 1
    local obj = setmetatable({}, json.object_mt)
    self:skip()
    if self:peek() == "}" then self.pos = self.pos + 1 return obj end
    while true do
      self:skip()
      if self:peek() ~= '"' then self:fail("expected string key") end
      local k = self:string()
      self:skip()
      if self:peek() ~= ":" then self:fail("expected ':'") end
      self.pos = self.pos + 1
      obj[k] = self:value()
      self:skip()
      local d = self:peek()
      self.pos = self.pos + 1
      if d == "}" then return obj end
      if d ~= "," then self.pos = self.pos - 1 self:fail("expected ',' or '}'") end
    end
  elseif c == "[" then
    self.pos = self.pos + 1
    local arr = {}
    self:skip()
    if self:peek() == "]" then self.pos = self.pos + 1 return arr end
    while true do
      arr[#arr + 1] = self:value()
      self:skip()
      local d = self:peek()
      self.pos = self.pos + 1
      if d == "]" then return arr end
      if d ~= "," then self.pos = self.pos - 1 self:fail("expected ',' or ']'") end
    end
  elseif c == '"' then
    return self:string()
  elseif c == "t" then return self:literal("true", true)
  elseif c == "f" then return self:literal("false", false)
  elseif c == "n" then return self:literal("null", json.null)
  elseif c == "-" or c:find("%d") then
    return self:number()
  elseif c == "" then
    self:fail("unexpected end of input")
  end
  self:fail("unexpected character '" .. c .. "'")
end

function json.decode(src)
  local d = setmetatable({src = src, pos = 1}, Decoder)
  local v = d:value()
  d:skip()
  if d.pos <= #src then d:fail("trailing garbage") end
  return v
end

-- helpers ---------------------------------------------------------------

local function deep_equal(a, b)
  if a == b then return true end
  if type(a) ~= "table" or type(b) ~= "table" then return false end
  if math.type(a) ~= math.type(b) then return false end
  for k, v in pairs(a) do
    if not deep_equal(v, b[k]) then return false end
  end
  for k in pairs(b) do
    if a[k] == nil then return false end
  end
  return true
end

-- tests -----------------------------------------------------------------

local data = {
  name = "Widget \"Pro\"",
  tags = {"a", "b\tc", "line\nbreak", "slash\\"},
  price = 12.5,
  count = 42,
  negative = -7,
  big = 9007199254740993,
  tiny = 1.5e-10,
  whole_float = 3.0,
  active = true,
  disabled = false,
  nothing = json.null,
  nested = { level1 = { level2 = { level3 = {1, 2, {3, 4, {5}}} } } },
  empty_arr = {},
  empty_obj = setmetatable({}, json.object_mt),
  ctrl = "bell\7 nul\0 end",
}

local text = json.encode(data)
print(text)
local back = json.decode(text)
print("round trip equal:", deep_equal(data, back))
print("re-encode equal:", json.encode(back) == text)
print("types:", math.type(back.count), math.type(back.price),
      math.type(back.whole_float), math.type(back.big))
print("big:", back.big, "tiny:", back.tiny)
print("ctrl bytes:", #back.ctrl, back.ctrl:byte(5), back.ctrl:byte(10))

-- unicode escapes
local u = json.decode('"caf\\u00e9 \\u20ac \\ud83d\\ude00 \\u0041\\/"')
print("unicode:", u, #u, utf8.len(u))
for _, cp in utf8.codes(u) do io.write(string.format("%X ", cp)) end
print()

-- whitespace and formatting tolerance
local spaced = json.decode(' \n\t{ "a" : [ 1 , 2.0 , -3e2 , 0.25 ] ,\r\n "b" : { } } ')
print("spaced:", json.encode(spaced))

-- numbers
for _, s in ipairs({"0", "-0", "123456789012", "1e3", "1.0", "-2.5E-3", "0.1"}) do
  local v = json.decode(s)
  print("number", s, math.type(v), json.encode(v))
end

-- malformed input
local bad = {
  '{"a":1,}', '[1 2]', '{"a" 1}', '"unterminated', '[1,2', 'tru',
  '{"x":"\\q"}', '"\\u12G4"', '01x', '{1:2}', '"\\ud800x"', '',
  '[1] extra', '"tab\there"', '@',
}
for _, s in ipairs(bad) do
  local ok, err = pcall(json.decode, s)
  print(string.format("%-14s %s %s", s:gsub("[%c]", "?"), tostring(ok), err))
end

-- encoding errors
local cyc = {}
cyc.self = cyc
print("cycle:", pcall(json.encode, cyc))
print("nan:", pcall(json.encode, {0/0}))
print("inf:", pcall(json.encode, {-math.huge}))
print("func:", pcall(json.encode, {f = print}))
print("mixed:", pcall(json.encode, {1, 2, x = 3}))

-- a larger generated structure round trip
local records = {}
for i = 1, 50 do
  records[i] = { id = i, label = "item" .. i, score = i / 8,
                 flags = { i % 2 == 0, i % 3 == 0 }, parent = i > 1 and i - 1 or json.null }
end
local big_text = json.encode({records = records, total = #records})
local big_back = json.decode(big_text)
print("records:", #big_text, deep_equal(big_back.records, records), big_back.total)
print("record 17:", json.encode(big_back.records[17]))
