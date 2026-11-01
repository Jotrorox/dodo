-- A runtime error in the script: earlier output is kept, stderr gets
-- "lua: cli_runtime_error.lua:N: <message>" followed by a traceback
-- (only the message is compared), and the exit status is 1.

local function inner(t)
  return t.field.subfield
end
print("before the error")
io.write("partial ")
inner({})
