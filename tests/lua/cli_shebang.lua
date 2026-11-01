#!/usr/bin/env lua
--! args: from-shebang-test
-- A script whose first line starts with "#" (a shebang): lua.c's file
-- loader skips that line but keeps line numbering, so errors below report
-- the real line numbers. (The harness also skips '#' lines when reading
-- directives.)

print("shebang skipped, arg:", arg[1])
print(select(2, pcall(error, "line check")))
print(select(2, pcall(function() error("at line 10") end)))
print(debug.getinfo(1, "l").currentline)
