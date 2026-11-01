-- Miscellaneous os functions: getenv (set and unset variables, argument
-- errors), clock (type only), tmpname (type only, file can be created),
-- remove and rename on temporary files including the nil+message+errno
-- results for missing files, setlocale queries, and argument checks.

print(os.getenv("LUA_TEST_VAR"))
print(os.getenv("LUA_TEST_VAR_THAT_DOES_NOT_EXIST_42"))
print(os.getenv("LUA_INIT"), os.getenv("LUA_PATH"))   -- removed by the harness
print(pcall(os.getenv))
print(pcall(os.getenv, {}))
print(os.getenv(""))

-- clock: a float, non-negative, non-decreasing
local c1 = os.clock()
local c2 = os.clock()
print(math.type(c1), c1 >= 0, c2 >= c1)

-- tmpname
local name = os.tmpname()
print(type(name), #name > 0)
local f = assert(io.open(name, "w"))
f:write("temporary")
f:close()

-- rename
local name2 = name .. ".renamed"
print(os.rename(name, name2))
print(io.open(name) == nil)
local g = assert(io.open(name2))
print(g:read("a"))
g:close()

-- remove
print(os.remove(name2))
print(io.open(name2) == nil)

-- failures return nil, "<path>: <strerror>", errno
print(os.remove("no_such_file_os_misc.txt"))
print(os.rename("no_such_file_os_misc.txt", "other_name_os_misc.txt"))
print(os.remove("no_such_dir_os_misc/file"))
local r1, r2, r3 = os.remove("no_such_file_os_misc.txt")
print(r1, type(r2), math.type(r3))

-- exactly three results on failure; note os.rename's message has no path
print(select("#", os.remove("no_such_file_os_misc.txt")))
print(select("#", os.rename("no_such_file_os_misc.txt", "x_os_misc.txt")))

-- argument checks
print(pcall(os.remove))
print(pcall(os.rename, "a"))
print(pcall(os.remove, {}))

-- setlocale: querying works; the C locale is always available
print(os.setlocale(nil), os.setlocale("C"), os.setlocale())
print(os.setlocale("C", "numeric"), os.setlocale(nil, "all"))
print(os.setlocale("this-locale-does-not-exist"))
print(pcall(os.setlocale, "C", "bogus"))

-- os.exit, os.execute exist; os.exit is tested in dedicated files
print(type(os.exit), type(os.execute), type(os.difftime))

-- list of os functions
local names = {}
for k in pairs(os) do names[#names + 1] = k end
table.sort(names)
print(table.concat(names, " "))
