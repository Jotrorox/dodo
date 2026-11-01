-- Table constructors: positional items, [k]=v and name=v fields, both
-- separators, trailing separators, and multiple-result expansion of the
-- last positional expression ({f()} vs {f(), 1} vs {(f())} vs {f(), nil}).

local function dump(t, n)
  local parts = {}
  for i = 1, n do parts[#parts + 1] = tostring(t[i]) end
  return "[" .. table.concat(parts, " ") .. "]"
end

local function three() return 10, 20, 30 end
local function none() end

-- empty and trivial
local e = {}
print("empty", #e, next(e))
print("single", #{5}, ({5})[1])

-- separators, trailing separators
local a = {1, 2, 3,}
local b = {1; 2; 3;}
local c = {1, 2; 3}
print("trailing ,", #a, dump(a, 3))
print("trailing ;", #b, dump(b, 3))
print("mixed sep", #c, dump(c, 3))
local d = {x = 1, y = 2;}
print("named trailing", d.x, d.y)

-- named and bracketed keys
local k = "key"
local t = {x = 1, ["y"] = 2, [k] = 3, [1 + 1] = "two", [k .. "2"] = 4}
print("named", t.x, t.y, t.key, t[2], t.key2)

-- positional items take indices 1.. irrespective of interleaved fields
local m = {"a", x = "X", "b", [10] = "ten", "c"}
print("interleave", m[1], m[2], m[3], m.x, m[10], #m == 3 or #m == 10)

-- repeated keys: the manual leaves the assignment order undefined, so only
-- check that one of the candidates won
local o1 = {[1] = "explicit", "positional"}
print("override1", o1[1] == "positional" or o1[1] == "explicit")
local o2 = {"positional", [1] = "explicit"}
print("override2", o2[1] == "positional" or o2[1] == "explicit")
local o3 = {x = 1, x = 2}
print("override3", o3.x == 1 or o3.x == 2)

-- multret expansion: only the last positional expression expands
local x1 = {three()}
print("{f()}", #x1, dump(x1, 3))
local x2 = {three(), 1}
print("{f(),1}", #x2, dump(x2, 2))
local x3 = {(three())}
print("{(f())}", #x3, dump(x3, 1))
local x4 = {1, three()}
print("{1,f()}", #x4, dump(x4, 4))
local x5 = {three(), three()}
print("{f(),f()}", #x5, dump(x5, 4))
local x6 = {none()}
print("{none()}", #x6, next(x6))
local x7 = {none(), 1}
print("{none(),1}", x7[1], x7[2])
local x8 = {three(), x = 1}
print("{f(), x=1}", x8[1], x8[2], x8.x)
local x9 = {x = 1, three()}
print("{x=1, f()}", #x9, dump(x9, 3), x9.x)
local x10 = {three(),}
print("{f(),}", #x10, dump(x10, 3))

-- varargs in constructors
local function va(...) return {...}, select('#', ...) end
local v, n = va(1, 2, 3)
print("{...}", n, #v, dump(v, 3))
local function va2(...) return {..., "end"} end
local v2 = va2(7, 8, 9)
print("{..., x}", dump(v2, 3))

-- nested constructors
local nest = {{1, 2}, {3, {4, 5}}, n = {deep = {deeper = "yes"}}}
print("nested", nest[1][2], nest[2][2][1], nest[2][2][2], nest.n.deep.deeper)

-- expressions as values and keys
local i = 0
local function nexti() i = i + 1; return i end
local ev = {nexti(), nexti(), nexti()}
print("eval order positional", dump(ev, 3))
local tk = {[true] = "t", [false] = "f", [1.5] = "f15", [-0] = "zero"}
print("odd keys", tk[true], tk[false], tk[1.5], tk[0])

-- large constructor (crosses internal flush boundaries, e.g. 50 per batch)
local big = {1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,
  21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,
  41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60, three()}
print("big", #big, big[50], big[51], big[60], big[61], big[63])
local sum = 0
for j = 1, #big do sum = sum + big[j] end
print("big sum", sum)

-- nil positional values still occupy their slot
local h = {1, nil, 3}
print("nil slot", h[1], h[2], h[3])
local h2 = {nil, nil, 3}
print("nil slots", h2[1], h2[2], h2[3])

-- constructor values are evaluated, a function value is stored
local fn = {function() return "called" end}
print("fn value", fn[1]())
