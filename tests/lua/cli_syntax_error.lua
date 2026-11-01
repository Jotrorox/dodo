-- A script with a syntax error is never run: lua.c prints
-- "lua: cli_syntax_error.lua:9: ..." (no traceback for syntax errors)
-- and exits with status 1; nothing reaches stdout.

print("this line must not run")
local function ok()
  return 1
end
local x = = 2
