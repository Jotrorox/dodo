-- io.popen: reading a command's output, writing to a command's stdin,
-- close() results (true/nil, "exit", status) for success and failure,
-- io.type of popen handles, lines over a pipe, invalid modes, and
-- os.execute results (shell availability, exit codes).

local p = assert(io.popen("echo hi"))
print(io.type(p))
print(p:read("a"))
print(p:close())

p = assert(io.popen("printf 'a\\nb\\nc\\n'", "r"))
local got = {}
for l in p:lines() do got[#got + 1] = l end
print(#got, table.concat(got, ","))
print(p:close())

-- exit statuses
p = io.popen("exit 3")
print(p:read("a"))
print(p:close())
p = io.popen("echo out; exit 1")
print(p:read("l"), p:close())
-- reading numbers from a pipe
p = io.popen("echo 10 20.5")
print(p:read("n", "n"))
p:close()

-- write mode: the command's output goes to our stdout (flush first)
io.stdout:flush()
p = assert(io.popen("cat", "w"))
print(io.type(p), p:write("written to cat\n") == p)
io.stdout:flush()
print(p:close())
io.stdout:flush()
p = assert(io.popen("exit 5", "w"))
print(p:close())

-- invalid modes
print(pcall(io.popen, "echo x", "rw"))
print(pcall(io.popen, "echo x", "x"))
print(pcall(io.popen, "echo x", "r+"))

-- closed pipe
p = io.popen("echo closed")
p:close()
print(io.type(p), pcall(p.read, p))

-- os.execute
print(os.execute())
print(os.execute("true"))
print(os.execute("exit 2"))
print(os.execute("false"))
-- command output goes to stdout directly
io.stdout:flush()
print(os.execute("echo from shell"))
-- killed by a signal: "signal", signo
print(os.execute("kill -TERM $$"))
