--! opts: -W
--! args: one "two words" "" 3 -e --
-- The arg table: script arguments at positive indices (strings, even
-- numeric-looking ones, including empty strings and option-like words
-- after the script), the script name at 0, interpreter options at
-- negative indices; the main chunk's ... equals the script arguments.

print("#arg =", #arg)
for i = 1, #arg do print(i, string.format("%q", arg[i]), type(arg[i])) end
print("arg[0] =", arg[0], "arg[-1] =", arg[-1])
print("arg[-2] is the interpreter:", type(arg[-2]), "arg[-3] =", arg[-3])
local va = table.pack(...)
print("varargs n =", va.n)
local same = va.n == #arg
for i = 1, va.n do same = same and va[i] == arg[i] end
print("varargs equal arg:", same)
-- arg is an ordinary global table that can be modified
arg[1] = "changed"
print(arg[1], select(1, ...))
