-- Loading damaged binary chunks: the error messages (or successful loads)
-- follow the reference loader, which reads before it validates.
local function try(label, chunk)
  local f, err = load(chunk, "=bin", "b")
  print(label, type(f), err)
  return f
end

local d = string.dump(load("local up = ... return function() return up, 'abc', 42 end"))

-- every truncation of the chunk
local seen = {}
for n = 1, #d - 1 do
  local _, err = load(d:sub(1, n), "=bin", "b")
  if not seen[err] then seen[err] = n print("prefix", n, err) end
end

-- damaged header fields
try("signature", "\27Lux" .. d:sub(5))
try("version", d:sub(1, 4) .. "\x53" .. d:sub(6))
try("format", d:sub(1, 5) .. "\1" .. d:sub(7))
try("luac_data", d:sub(1, 6) .. "\x19\x93\r\n\x1a\r" .. d:sub(13))
try("short data", d:sub(1, 9))
try("instr size", d:sub(1, 12) .. "\8" .. d:sub(14))
try("int format", d:sub(1, 15) .. "\0" .. d:sub(17))

-- the main function's upvalue count in the header is not checked
local f = try("header nups", d:sub(1, 31) .. "\3" .. d:sub(33))
print("runs", f and select("#", f()))

-- an unknown constant tag leaves the constant nil
local inner = string.dump(load("gk = true"))
local pos = inner:find("\4\131gk\17", 1, true)
local g = try("bad tag", inner:sub(1, pos + 3) .. "\9" .. inner:sub(pos + 5))
print("bad tag runs", pcall(g))
print("gk", gk)

-- a debug-name count other than the upvalue count still reads one name
-- per upvalue
local s = string.dump(load("local a, b return function() return a, b end"))
print("names", select(2, load(s, "=x", "b")) == nil)

-- forged string sizes: the string is allocated before it is read
local function vlq(n)
  local t = {}
  repeat t[#t + 1] = n & 0x7f; n = n >> 7 until n == 0
  local out = {}
  for i = #t, 1, -1 do out[#out + 1] = string.char(t[i] | (i == 1 and 0x80 or 0)) end
  return table.concat(out)
end
local hdr = d:sub(1, 32)
for _, size in ipairs({1, 41, 1000, math.maxinteger}) do
  try("size " .. size, hdr .. vlq(size) .. "xyz")
end
try("size overflow", hdr .. string.rep("\127", 10) .. "\255")
