--! cmd: - first "second arg"
--! stdin: {script}
-- The script is read from standard input because the script name is "-":
-- arg[0] is "-", the remaining words are arguments, the chunk name is
-- "=stdin" (so positions read "stdin:N:"), and io.read sees nothing more.

print("arg[0] =", arg[0], "#arg =", #arg, arg[1], arg[2])
print("varargs:", ...)
local info = debug.getinfo(1, "S")
print("source:", info.source, "short_src:", info.short_src)
print(pcall(error, "positioned"))
print(select(2, pcall(function() error("here") end)))
print("stdin rest:", io.read("a"))
