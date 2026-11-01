-- File handles: io.write/file:write return the file (chaining) and how
-- numbers are written, seek with "set"/"cur"/"end", setvbuf, flush,
-- io.type for open/closed/non-files, operations on closed files, io.open
-- errors and invalid modes, io.tmpfile, tostring(file) prefix, and
-- switching io.output/io.input to files.

-- io.write returns the default output file
print(io.write("") == io.stdout, io.stdout:write("") == io.stdout)
io.write("chained ", 1, " ", 2.5, "\n"):write("second call\n")
-- number formatting in write: integers as %d, floats as %.14g
io.write(1.0, " ", -0.0, " ", 1e100, " ", 2^63, " ", math.mininteger, " ", 0.1, " ", 1/3, "\n")
print(pcall(io.write, {}))
print(pcall(io.write, nil))
print(pcall(io.write, true))

-- io.type
print(io.type(io.stdout), io.type(io.stdin), io.type(42), io.type(nil), io.type({}))
print(pcall(io.type))

-- tostring prefix
print(tostring(io.stdout):sub(1, 6))

local name = os.tmpname()
local f = assert(io.open(name, "w"))
print(io.type(f), f:write("0123456789") == f)
print(f:seek("cur"), f:seek("set"), f:seek("end"), f:seek("set", 3), f:seek("cur", 2), f:seek("end", -1))
print(f:seek())
f:write("X")
print(f:seek("cur"))
print(pcall(function() return f:seek("middle") end))
print(pcall(f.seek, f, "set", 1.5))
print(f:seek("set", -5))
print(f:setvbuf("no"), f:setvbuf("full", 1024), f:setvbuf("line"))
print(pcall(function() return f:setvbuf("bogus") end))
print(f:flush(), io.flush(), io.type(f))
print(f:close())
print(io.type(f), tostring(f))

-- operations on a closed file
for _, op in ipairs({"read", "write", "lines", "seek", "flush", "setvbuf", "close"}) do
  local ok, msg = pcall(f[op], f, "l")
  print(op, ok, msg)
end
print(pcall(io.close, f))

-- reading back what was written (seek + write overwrote byte 9)
f = assert(io.open(name, "r"))
print(f:read("a"))
f:close()

-- append and update modes
f = assert(io.open(name, "a"))
f:write("+app")
f:close()
f = assert(io.open(name, "r+"))
f:write("AB")
f:seek("set", 0)
print(f:read("a"))
f:close()
f = assert(io.open(name, "w+b"))
f:write("fresh")
f:seek("set", 1)
print(f:read(2))
f:close()

-- io.open errors
print(io.open("no_such_dir/nofile.txt"))
print(io.open("no_such_file_for_io_test.txt", "r"))
for _, mode in ipairs({"x", "rw", "r++", "", "wb+", "rb", "a+b", "w+x", "bw"}) do
  local ok, err = pcall(io.open, name, mode)
  if ok and io.type(err) == "file" then err:close(); err = "file" end
  print(string.format("mode %q:", mode), ok, err)
end

-- io.output / io.input switching
local old_out = io.output()
print(old_out == io.stdout)
io.output(name)
io.write("via io.write\n", 42, "\n")
print_ok = io.output() ~= io.stdout
io.output():close()
io.output(old_out)
print("switched output:", print_ok)
io.input(name)
print(io.read("l"), io.read("n"))
io.input():close()
print(pcall(io.read))
io.input(io.stdin)
print(io.input() == io.stdin)
local h = assert(io.open(name))
print(io.input(h) == h, io.read("L"))
h:close()
io.input(io.stdin)
print(pcall(io.input, "no_such_file_for_io_test.txt"))

-- io.tmpfile
local t = io.tmpfile()
print(io.type(t), t:write("tmp data"):seek("set"), t:read("a"))
t:close()

-- io.close() with no argument closes the default output (use a file)
io.output(name)
print(io.close())
print(io.type(io.output()))
io.output(io.stdout)
os.remove(name)
print("done")
