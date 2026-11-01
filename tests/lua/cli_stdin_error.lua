--! cmd: -
--! stdin: {script}
-- A runtime error in a script read from stdin ("lua -"): positions use
-- the chunk name "stdin", so stderr reads "lua: stdin:8: ..."; exit 1.

print("running from stdin, about to fail")
local t = nil
local v = t.missing
