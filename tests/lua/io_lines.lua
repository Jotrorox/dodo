-- io.lines and file:lines: iterating lines (with and without trailing
-- newline, empty lines, CRLF kept), formats passed to lines ("L", "n",
-- counts, several formats per step), io.lines closing the file at EOF
-- and returning 4 values for generic-for closing, file:lines leaving the
-- file open, iterating after close, and missing-file errors.

local name = os.tmpname()
local function write(s)
  local f = assert(io.open(name, "wb"))
  f:write(s)
  f:close()
end
local function q(v) return type(v) == "string" and string.format("%q", v) or tostring(v) end

write("alpha\nbeta\n\ngamma\r\ndelta")
for l in io.lines(name) do io.write(q(l), " ") end
print()
for l in io.lines(name, "L") do io.write(q(l), " ") end
print()
for a, b in io.lines(name, 1, 2) do io.write(q(a), "+", q(b), " ") if a == "\n" then break end end
print()

-- io.lines returns iterator plus three more values (nil, nil, file)
local it, s, c, closing = io.lines(name)
print(type(it), s, c, io.type(closing))
print(q(it()), q(it()), q(it()), q(it()), q(it()))
print("values at EOF:", select("#", it()))
print(io.type(closing))
-- calling the iterator after EOF closed the file
print(pcall(it))

-- numbers
write("1 2 3.5\n0x10 -4\n")
local nums = {}
for n in io.lines(name, "n") do nums[#nums + 1] = math.type(n) .. ":" .. n end
print(table.concat(nums, " "))
for a, b in io.lines(name, "n", "n") do print(a, b) end

-- file:lines keeps the file open after iteration
write("x\ny\n")
local f = assert(io.open(name))
local count = 0
for l in f:lines() do count = count + 1 end
print(count, io.type(f), f:read("a"))
f:seek("set")
for l in f:lines("L") do io.write(q(l), " ") end
print()
f:close()
-- file:lines iterator used after closing the file
f = assert(io.open(name))
local fit = f:lines()
print(fit())
f:close()
print(pcall(fit))

-- breaking out of a generic for over io.lines closes the file (4th value)
local captured
do
  local iter, a, b, file = io.lines(name)
  captured = file
  for l in iter, a, b, file do break end
end
print("closed after break:", io.type(captured))

-- empty file
write("")
local n = 0
for l in io.lines(name) do n = n + 1 end
print("empty file lines:", n)
for l in io.lines(name, "a") do print("a format on empty file:", q(l)) break end

-- only newlines
write("\n\n\n")
n = 0
for l in io.lines(name) do n = n + 1; assert(l == "") end
print("blank lines:", n)

-- errors
print(pcall(io.lines, "no_such_file_for_lines.txt"))
-- formats are only checked when the iterator runs
write("data\n")
local ok1, it1 = pcall(io.lines, name, "x")
print(ok1, pcall(it1))
local ok2, it2 = pcall(io.lines, name, {})
print(ok2, pcall(it2))
-- io.lines with no file name iterates the default input (stdin is empty)
n = 0
for l in io.lines() do n = n + 1 end
print("stdin lines:", n)

os.remove(name)
