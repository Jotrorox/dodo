-- Variable names in error messages are C strings in the reference: they end
-- at the first NUL byte.
print(pcall(function() return "10\0" | 1 end))
print(pcall(function() local t = {} return t["a\0b"].x end))
print(pcall(function() return _ENV["g\0h"].x end))
print(pcall(function() local t = {} t["m\0n"](1) end))
print(pcall(function() local s = "x\0y" return s.foo.bar end))
print(pcall(function() return ("\0abc") + {} end))
