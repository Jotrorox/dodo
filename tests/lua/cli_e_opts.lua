--! opts: -e "x = 1" -e "print('e1', x)" -ex=x+10 -e "print('e3', x, arg and #arg)" -e ""
-- Several -e options run in order before the script, sharing globals;
-- the argument may be attached ("-ex=x+10"); an empty -e is fine. The
-- script then sees the globals and the arg table (already set while the
-- -e chunks ran).

print("script sees x =", x)
print("arg[0] =", arg[0])
local opts = {}
for i = -1, -10, -1 do
  if arg[i - 1] == nil then break end   -- stop before the interpreter name
  opts[#opts + 1] = string.format("[%d]=%q", i, arg[i])
end
print(table.concat(opts, " "))
print("#arg =", #arg, "varargs =", select("#", ...))
