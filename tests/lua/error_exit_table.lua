-- Uncaught error whose value is a table without __tostring: lua.c reports
-- "(error object is a table value)" and exits with status 1.
-- A __name field in the metatable does not change this message.

print("start")
local t = setmetatable({}, {__name = "MyType"})
local ok, e = pcall(error, t)
print("caught same table:", ok, e == t)
error({1, 2, 3})
