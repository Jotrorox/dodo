-- loadfile and dofile with fixture files: return values and varargs,
-- chunk names derived from the path ("@fixtures/x.lua"), first-line '#'
-- and UTF-8 BOM skipping, syntax and runtime errors with file positions,
-- missing files ("cannot open ..."), mode and env parameters of loadfile,
-- and binary chunks written to a temporary file.

-- loadfile returns a function; the chunk receives the call arguments
local f = loadfile("fixtures/chunk.lua")
print(type(f))
print(f("a", "b"))
print(f())
print(debug.getinfo(f, "S").source, debug.getinfo(f, "S").short_src)

-- dofile returns all the chunk's values and passes no arguments
print(dofile("fixtures/chunk.lua"))
print(select("#", dofile("fixtures/chunk.lua")))

-- first line starting with '#' is skipped (line numbers are preserved)
print(dofile("fixtures/shebang.lua"))
print(dofile("fixtures/hashline.lua"))
print(dofile("fixtures/bom.lua"))

-- syntax errors: loadfile returns nil + message, dofile raises
print(loadfile("fixtures/syntax_error.lua"))
print(pcall(dofile, "fixtures/syntax_error.lua"))

-- runtime errors propagate out of dofile with the file position
print(pcall(dofile, "fixtures/runtime_error.lua"))
local rf = loadfile("fixtures/runtime_error.lua")
print(pcall(rf))

-- missing files
print(loadfile("fixtures/does_not_exist.lua"))
print(pcall(dofile, "fixtures/does_not_exist.lua"))
print(loadfile("no_such_dir/x.lua"))

-- loadfile mode parameter
print(loadfile("fixtures/chunk.lua", "b"))
print(type(loadfile("fixtures/chunk.lua", "t")))
print(type(loadfile("fixtures/chunk.lua", "bt")))

-- loadfile env parameter
local env = {x_from_env = "X", y_from_env = "Y"}
print(loadfile("fixtures/envchunk.lua", "t", env)())
print(loadfile("fixtures/envchunk.lua", "t")())
x_from_env = "global x"
print(loadfile("fixtures/envchunk.lua")())
x_from_env = nil

-- binary chunk saved to a temporary file and loaded back
local name = os.tmpname()
local out = assert(io.open(name, "wb"))
out:write(string.dump(function(...) return "from binary file", select("#", ...) end))
out:close()
local bf = loadfile(name)
print(bf(1, 2, 3))
print(dofile(name))
local bt, berr = loadfile(name, "t")
print(bt, (berr:gsub("^.*: ", "")))
-- a binary chunk preceded by a '#' line is also accepted
out = assert(io.open(name, "wb"))
out:write("#!shebang line\n", string.dump(function() return "binary after #" end))
out:close()
print(dofile(name))
os.remove(name)

-- dofile with no argument reads stdin (empty here), returning nothing
print(select("#", dofile()))
-- loadfile(nil) also reads stdin; its chunkname is "=stdin"
local sf = loadfile(nil)
print(type(sf), debug.getinfo(sf, "S").source)

-- argument errors
print(pcall(dofile, {}))
print(pcall(loadfile, {}))
