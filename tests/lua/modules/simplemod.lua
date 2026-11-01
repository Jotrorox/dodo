-- fixture module: records the arguments require passes to the chunk
local name, data = ...
local M = {}
M.name = name
M.data = data
M.nargs = select("#", ...)
function M.greet(who) return "hello, " .. who end
return M
