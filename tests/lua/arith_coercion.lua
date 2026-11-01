-- Arithmetic: string->number coercion in arithmetic and error messages
local function show(label, f)
  local ok, v = pcall(f)
  if ok then print(label, math.type(v) or type(v), v ~= v and "nan" or v)
  else print(label, "error", v) end
end

show('"10"+1', function() return "10" + 1 end)
show('"10.0"+1', function() return "10.0" + 1 end)
show('"0x10"*1', function() return "0x10" * 1 end)
show('"1e1"+0', function() return "1e1" + 0 end)
show('" 5 "+0', function() return " 5 " + 0 end)
show('"\\t5\\n"+0', function() return "\t5\n" + 0 end)
show('"5"*"2"', function() return "5" * "2" end)
show('-"2"', function() return -"2" end)
show('-"2.0"', function() return -"2.0" end)
show('-"-0"', function() return -"-0" end)
show('"10"//"3"', function() return "10" // "3" end)
show('"7"%"2"', function() return "7" % "2" end)
show('"2"^"3"', function() return "2" ^ "3" end)
show('"1"/"2"', function() return "1" / "2" end)
show('"0x1p4"+0', function() return "0x1p4" + 0 end)
show('"9223372036854775807"+0', function() return "9223372036854775807" + 0 end)
show('"9223372036854775808"+0', function() return "9223372036854775808" + 0 end)
show('"0xffffffffffffffff"+0', function() return "0xffffffffffffffff" + 0 end)
show('"1"+1.5', function() return "1" + 1.5 end)
show('"-7"//2', function() return "-7" // 2 end)
show('"10"//"0"', function() return "10" // "0" end)
show('"10"%"0"', function() return "10" % "0" end)
show('"1.5"+"1.5"', function() return "1.5" + "1.5" end)
show('".5"+0', function() return ".5" + 0 end)
show('"5."+0', function() return "5." + 0 end)

-- strings that are not numerals
show('"abc"+1', function() return "abc" + 1 end)
show('1+"abc"', function() return 1 + "abc" end)
show('""+0', function() return "" + 0 end)
show('"0x"+0', function() return "0x" + 0 end)
show('"1e"+0', function() return "1e" + 0 end)
show('"inf"+0', function() return "inf" + 0 end)
show('"nan"+0', function() return "nan" + 0 end)
show('"1 2"+0', function() return "1 2" + 0 end)
show('"10\\0"+0', function() return "10\0" + 0 end)
show('-"x"', function() return -"x" end)
local s = "abc"
show("local s+1", function() return s + 1 end)
local t = {x = "abc", y = {}}
show("field x*2", function() return t.x * 2 end)
show("field y+1", function() return t.y + 1 end)
gstr = "zz"
show("global gstr-1", function() return gstr - 1 end)
show("upvalue s^2", function() return 2 ^ s end)
show("nil+1", function() return t.missing + 1 end)
show("undefined global", function() return undefined_global + 1 end)
show("true+1", function() return true + 1 end)
show("{}+1", function() return {} + 1 end)
show("function * 2", function() return print * 2 end)
show("1 + nil const", function() local n; return 1 + n end)
show("method result", function() return s:upper() + 1 end)
show("call result", function() return tostring(1) .. "" + {} end)

-- arithmetic metamethods take precedence over coercion for tables only
local mt = {__add = function(a, b) return "added" end, __unm = function() return "neg" end,
            __idiv = function() return "idiv" end, __mod = function() return "mod" end}
local obj = setmetatable({}, mt)
show("obj+1", function() return obj + 1 end)
show("'1'+obj", function() return "1" + obj end)
show("-obj", function() return -obj end)
show("obj//0", function() return obj // 0 end)
show("obj%0", function() return obj % 0 end)
show("obj*1", function() return obj * 1 end)

-- string results of arithmetic print without quotes and as numbers
print(("10" + 0) .. "", ("10" * 1.0) .. "", (10 .. "") + 1, 2 .. 3 + 1)
