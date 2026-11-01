-- boolean logic in value and jump contexts
local a, b, c, d = ...
local v1 = a and b
local v2 = a or b
local v3 = a and b or c
local v4 = (a or b) and (c or d)
local v5 = not a
local v6 = not not a
local v7 = a == b
local v8 = a < b and c > d
local v9 = a and (b < c)
local v10 = (a < b) or c
local v11 = a == nil or b ~= nil
local v12 = nil and a
local v13 = false or a
local v14 = 1 and a
local v15 = a or 2
if a then print(1) end
if not a then print(2) end
if a and b then print(3) end
if a or b then print(4) end
if a and not b or c then print(5) else print(6) end
if (a == 1) == (b == 2) then print(7) end
if a < b then elseif b < c then elseif c then else end
while a and b do a = nil end
while not (a or b) do b = 1 end
if nil then print(8) end
if true then print(9) end
if 1 then print(10) end
if not nil then print(11) end
if a == "x" or a == "y" or a == 3 then print(12) end
local w = a and f() or g()
local x = {a and b, a or b}
return v1, v2, v3, v4, v5, v6, v7, v8, v9, v10, v11, v12, v13, v14, v15, w, x
