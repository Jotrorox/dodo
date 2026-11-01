-- numeric and generic loops
local t = {}
for i = 1, 10 do t[i] = i end
for i = 10, 1, -1 do t[i] = -i end
for i = 1.0, 2.0, 0.25 do t[#t + 1] = i end
for i = #t, 1, -2 do print(t[i]) end
for i = f(), g(), h() do print(i) end
for k, v in pairs(t) do print(k, v) end
for k in next, t do print(k) end
for a, b, c, d in f() do print(a, b, c, d) end
for k, v in g, h, i, j do print(k, v) end
for _, v in ipairs(t) do
  local w = function() return v end
  print(w())
end
local i = 0
while i < 10 do i = i + 1 end
while f(i) do i = i - 1; if i == 3 then break end end
repeat i = i + 1 until i >= 20
repeat local done = i > 30; i = i + 1 until done
for i = 1, 3 do for j = i, 3 do for k = j, 3 do print(i, j, k) end end end
