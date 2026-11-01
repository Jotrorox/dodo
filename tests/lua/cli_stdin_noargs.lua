--! cmd:
--! stdin: {script}
-- With no arguments at all and stdin not a terminal, lua.c runs standard
-- input as a script (like "lua -"), without printing a banner. arg[0]
-- is then the interpreter name (not printed), there are no positive
-- entries, and the main chunk receives no varargs.

print("running from stdin")
print("#arg =", #arg, "arg[1] =", arg[1], "varargs =", select("#", ...))
print("source:", debug.getinfo(1, "S").source)
local ok, msg = pcall(error, "x")
print(ok, msg)
print(select(2, pcall(function() local t; return t.x end)))
