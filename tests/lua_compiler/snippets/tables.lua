-- table constructors and indexing
local x, y = ...
local t1 = {}
local t2 = {1, 2, 3}
local t3 = {a = 1, b = 2, ["c"] = 3, [4] = 4, [x] = y}
local t4 = {1, a = 2, 3, b = 4; 5, [6] = 6, 7,}
local t5 = {f()}
local t6 = {f(), g()}
local t7 = {(f())}
local t8 = {...}
local t9 = {{1}, {2, {3}}, {a = {b = {}}}}
local t10 = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
  21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
  41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, f()}
local v = t2[1] + t2[255] + t2[256] + t2[0] + t2[-1] + t2[1.0] + t2[x] + t2.a + t2["b"]
t2[1] = 1; t2[256] = 2; t2[-1] = 3; t2.k = 4; t2["long key that is longer than forty characters!"] = 5
t2[x] = y; t2[x] = 1; t2[x] = "s"; t2[x] = nil; t2[x] = true; t2[1.5] = 2; t2[x + 1] = t2[x - 1]
local w = t2["long key that is longer than forty characters!"]
local m = t9:method_with_a_rather_long_name_exceeding_forty_bytes_of_name()
t3.a.b.c.d = t3.e.f.g
return v, w, m
