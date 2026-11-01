-- fixture for load_files.lua: returns its varargs count and a global
local n = select("#", ...)
loaded_counter = (loaded_counter or 0) + 1
return "chunk result", n, loaded_counter, ...
