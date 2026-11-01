--! args: x
-- Values returned by the main chunk are ignored by lua.c: the exit
-- status stays 0 and nothing is printed for them, even for false/nil or
-- an error-looking string.

print("returning values")
return false, "error-looking string", 1
