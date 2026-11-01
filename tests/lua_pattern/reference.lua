-- Prints the expected results for tests/lua_pattern cases using a reference
-- Lua 5.4 interpreter. Usage: lua reference.lua cases.txt > expected.txt
-- Output format: see tests/stdlib/lua_pattern_driver.dodo.
local function unhex(h)
  return (h:gsub("%x%x", function(d) return string.char(tonumber(d, 16)) end))
end
local function hex(s)
  return "s" .. s:gsub(".", function(c) return string.format("%02x", c:byte()) end)
end
local function val(v)
  if math.type(v) == "integer" then return tostring(v) end
  return hex(v)
end
local function list(t, i, n)
  local r = {}
  for k = i, n do r[#r + 1] = val(t[k]) end
  return table.concat(r, ",")
end
local function result(r)
  if not r[1] then return "E" .. r[2] end
  if r[2] == nil then return "N" end
  return list(r, 2, r.n)
end

for line in io.lines(arg[1]) do
  local hs, hp, init = line:match("^x(%x*) x(%x*) (%-?%d+)$")
  local s, p = unhex(hs), unhex(hp)
  init = math.tointeger(tonumber(init))
  local f = result(table.pack(pcall(string.find, s, p, init)))
  local m = result(table.pack(pcall(string.match, s, p, init)))
  local steps = {}
  local ok, msg = pcall(function()
    local it = string.gmatch(s, p, init)
    while true do
      local t = table.pack(it())
      if t[1] == nil then break end
      steps[#steps + 1] = list(t, 1, t.n)
    end
  end)
  if not ok then steps[#steps + 1] = "E" .. msg:gsub("^[^:]*:%d+: ", "") end
  local g = table.concat(steps, ";")
  local r = table.pack(pcall(string.gsub, s, p, "<%0>"))
  local sub = r[1] and (hex(r[2]) .. "," .. r[3]) or ("E" .. r[2])
  io.write("F:", f, "|M:", m, "|G:", g, "|S:", sub, "\n")
end
