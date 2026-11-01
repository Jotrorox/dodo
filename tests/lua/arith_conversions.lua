-- Arithmetic: float<->integer conversions in integer-only contexts
local function try(label, f, ...)
  local r = table.pack(pcall(f, ...))
  if r[1] then
    local out = {}
    for i = 2, r.n do out[#out + 1] = tostring(r[i]) .. "(" .. (math.type(r[i]) or type(r[i])) .. ")" end
    print(label, "ok", table.concat(out, " "))
  else
    print(label, "err", r[2])
  end
end

-- math.tointeger
for _, v in ipairs{3.0, 3.5, -0.0, 2^53, 2^63, -2^63, 1e100, math.huge, 42, "8", "8.0", "x", {}} do
  try("tointeger " .. tostring(v):gsub("table: 0x%x+", "table"), math.tointeger, v)
end
try("tointeger nan", math.tointeger, 0 / 0)
try("tointeger()", math.tointeger)

-- bitwise operators require exact integer values
try("3.0 | 0", function() return 3.0 | 0 end)
try("3.5 | 0", function() local x = 3.5; return x | 0 end)
try("2^63 | 0", function() local x = 2^63; return x | 0 end)
try("-2^63 | 0", function() local x = -2^63; return x | 0 end)
try("2^53 & -1", function() local x = 2^53; return x & -1 end)
try("inf ~ 1", function() local x = math.huge; return x ~ 1 end)
try("1 << 2.0", function() return 1 << 2.0 end)
try("1 << 2.5", function() local s = 2.5; return 1 << s end)

-- library functions taking integers
try("rep 2.0", string.rep, "x", 2.0)
try("rep 2.5", string.rep, "x", 2.5)
try("rep '3'", string.rep, "x", "3")
try("rep '3.0'", string.rep, "x", "3.0")
try("rep '3.5'", string.rep, "x", "3.5")
try("sub 2.0", string.sub, "hello", 2.0, 4.0)
try("sub 2.5", string.sub, "hello", 2.5)
try("char 65.0", string.char, 65.0)
try("char 2^63", string.char, 2^63)
try("select 2.0", select, 2.0, "a", "b")
try("select 1.5", select, 1.5, "a", "b")
try("table.concat 1.0", table.concat, {"a", "b", "c"}, ",", 1.0, 2.0)
try("tostring(10 // 3.0)", function() return tostring(10 // 3.0) end)

-- indexing with floats
local t = {"a", "b", "c"}
try("t[2.0]", function() return t[2.0] end)
try("t[2.5]", function() return t[2.5] end)
try("t['2']", function() return t["2"] end)

-- numeric for loops
local function loop(a, b, c)
  local out = {}
  for i = a, b, c or 1 do
    out[#out + 1] = tostring(i) .. ":" .. math.type(i)
    if #out > 6 then out[#out + 1] = "..."; break end
  end
  return table.concat(out, " ")
end
try("for 1,3", loop, 1, 3)
try("for 1,3.5", loop, 1, 3.5)
try("for 1.0,3", loop, 1.0, 3)
try("for 1,3,0.5", loop, 1, 3, 0.5)
try("for 3,1,-1", loop, 3, 1, -1)
try("for 1,-1.5,-1", loop, 1, -1.5, -1)
try("for maxint-2,1e100", loop, math.maxinteger - 2, 1e100)
try("for minint+2,-1e100,-1", loop, math.mininteger + 2, -1e100, -1)
try("for 1,0", loop, 1, 0)
try("for 1,-math.huge", loop, 1, -math.huge)
try("for maxint,maxint", loop, math.maxinteger, math.maxinteger)
try("for 0.1,0.35,0.1", loop, 0.1, 0.35, 0.1)
try("for step 0", loop, 1, 3, 0)
try("for step 0.0", loop, 1, 3, 0.0)
try("for limit nan", loop, 1, 0 / 0)
try("for '1'", loop, "1", 3)
try("for limit 'x'", loop, 1, "x")
try("for step {}", loop, 1, 2, {})
try("for 1,2,maxint", loop, 1, 2, math.maxinteger)
try("for -1,maxint,maxint", loop, -1, math.maxinteger, math.maxinteger)
