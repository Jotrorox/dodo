--! cmd: -e "print('option ran')" -- {script} -e "not an option" --
-- "--" ends option processing: the next word is the script, and every
-- word after it (even "-e" and "--") is a script argument.

print("#arg =", #arg)
for i = 1, #arg do print(i, arg[i]) end
print("arg[0] =", arg[0], "arg[-1] =", arg[-1], "arg[-2] =", arg[-2], "arg[-3] =", arg[-3])
print("varargs:", select("#", ...), ...)
