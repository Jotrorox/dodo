-- load() with string and reader-function sources: pieces concatenated
-- across reader calls (tokens split between pieces), reader ending with
-- nil, "" or no value, readers returning non-strings, errors inside the
-- reader, the env parameter and the _ENV upvalue, varargs of chunks, and
-- the absence of loadstring/setfenv in 5.4.

print("loadstring:", type(loadstring), "setfenv:", type(setfenv))

-- string source
local f = load("return 1 + 2")
print(type(f), f())
print(load("local a, b = ...; return b, a")("x", "y"))
print(select("#", load("return ...")(1, nil, nil)))
print(load("")())
print(select("#", load("")()))

-- reader function: pieces split mid-token
local function reader(pieces)
  local i = 0
  return function()
    i = i + 1
    return pieces[i]
  end
end
print(load(reader({"ret", "urn 1", "0 + ", "2", "0"}))())
print(load(reader({"return 'ab", "cd'"}))())
print(load(reader({"local x = [[long", " string]] return x"}))())

-- reader ends with "" instead of nil
local n = 0
print(load(function() n = n + 1; if n == 1 then return "return 'first'" end return "" end)())
-- reader returning nothing at all
n = 0
print(load(function() n = n + 1; if n == 1 then return "return 'none'" end end)())
-- reader returning an empty first piece = empty chunk
print(select("#", load(function() return "" end)()))

-- reader returning a number (converted to a string) / a non-string
print(load(reader({"return ", 42, nil}))())
-- (only the first line of messages is printed: in PUC Lua, errors raised
-- while loading run through the caller's message handler, so under lua.c
-- they carry an appended traceback)
local function firstline(f, m)
  if type(m) == "string" then m = m:match("^[^\n]*") end
  return tostring(f) .. " | " .. tostring(m)
end
print(firstline(load(function() return {} end)))
print(firstline(load(function() return true end)))
-- reader raising an error: load returns nil + the error message
print(firstline(load(function() error("reader failed") end)))

-- number of reader calls: called until it signals the end
local calls = 0
local parts = {"return ", "1"}
load(function() calls = calls + 1; return parts[calls] end)
print("reader calls:", calls)

-- env parameter
local env = {y = 5}
local g = load("x = 10; return y", "chunk", "t", env)
print(g(), env.x, x)
-- env nil explicitly: globals are then inaccessible
local h = load("return print", "chunk", "t", nil)
print(pcall(h))
-- env omitted (no 4th arg): uses the global table
print(load("return print", "chunk", "t")() == print)

-- _ENV is the first (only) upvalue of a loaded chunk
local c = load("return a")
local upname, upval = debug.getupvalue(c, 1)
print(upname, upval == _G)
debug.setupvalue(c, 1, {a = "from new env"})
print(c())
-- chunk declaring a local _ENV
print(load("local _ENV = {z = 'local env'}; return z")())

-- load with non-string, non-function first argument
print(pcall(load, 42))
print(pcall(load, nil))
print(pcall(load))

-- loaded chunks are vararg functions
local info = debug.getinfo(load("return 1"), "u")
print("chunk nparams/vararg/nups:", info.nparams, info.isvararg, info.nups)

-- each load creates a new function
local s = "return 1"
print(load(s) == load(s))

-- chunk with a shebang line is NOT skipped by load (only by loadfile)
print(load("#!/usr/bin/lua\nreturn 1"))
