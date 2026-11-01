-- The gmatch iterator is a C closure with three upvalues: the subject, the
-- pattern and a userdata holding the iteration state. Replacing upvalues
-- through the debug library does not change what the iterator matches.
local it = string.gmatch("k1=v1, k2=v2, k3=v3", "(%w+)=(%w+)")
print(debug.getinfo(it, "u").nups)
for i = 1, 4 do
  local name, value = debug.getupvalue(it, i)
  print(i, name, type(value), type(value) == "string" and value or "")
end
print(it())
print(debug.setupvalue(it, 1, "zz=top"))
print(debug.setupvalue(it, 2, "(.)"))
print(it())
print(it())
print(it())
print(select("#", debug.getupvalue(string.gmatch(123, "%d"), 1)), debug.getupvalue(string.gmatch(123, "%d"), 1))
print(debug.getupvalue(string.gmatch("x", 4.0), 2))
