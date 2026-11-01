-- Vararg expressions and result adjustment: ... expands only as the last
-- item of an argument list, constructor, return list or assignment list;
-- elsewhere (and inside parentheses) it is truncated to one value; varargs
-- in main chunk; passing more than 200 arguments; varargs in nested
-- functions require their own '...'.

local function show(...)
  local parts = {}
  for i = 1, select('#', ...) do parts[i] = tostring((select(i, ...))) end
  return select('#', ...) .. ":[" .. table.concat(parts, ",") .. "]"
end

local function f(...) return show(...) end
local function g(...) return show(..., "end") end
local function h(...) return show("start", ...) end
local function p(...) return show((...)) end
print("passthrough", f(1, 2, 3))
print("middle truncates", g(1, 2, 3))
print("middle empty", g())
print("last expands", h(1, 2, 3))
print("last empty", h())
print("paren", p(1, 2, 3))
print("paren empty", p())

-- in return lists
local function ret1(...) return ... end
local function ret2(...) return ..., "x" end
local function ret3(...) return "x", ... end
local function ret4(...) return (...) end
print("return ...", show(ret1(1, 2)))
print("return ..., x", show(ret2(1, 2)))
print("return x, ...", show(ret3(1, 2)))
print("return (...)", show(ret4(1, 2)), show(ret4()))

-- in local declarations and assignments
local function locals(...)
  local a, b, c = ...
  local d, e = ..., "last"
  return show(a, b, c, d, e)
end
print("locals", locals(1, 2, 3, 4))
print("locals short", locals(1))
local function assigns(...)
  local x, y, z
  x, y, z = "first", ...
  return show(x, y, z)
end
print("assign", assigns(1, 2, 3))

-- in table constructors
local function ctor(...)
  local t1 = {...}
  local t2 = {..., "e"}
  local t3 = {"s", ...}
  local t4 = {(...)}
  return #t1, #t2, #t3, #t4
end
print("ctor", ctor(1, 2, 3))

-- trailing nils are preserved through calls
local function count(...) return select('#', ...) end
print("trailing nils", count(nil), count(1, nil), count(nil, nil, nil), count(f()))
print("nil passthrough", count(ret1(nil, nil)))

-- function results adjusted the same way as ...
local function three() return 1, 2, 3 end
print("call middle", show(three(), 10))
print("call last", show(10, three()))
print("call paren", show((three())))
print("call in ctor", #{three(), three()})
-- in binary expressions results are truncated
print("in expression", three() + 10, -three(), three() .. "")
-- string method call results
print("method results", show(("abc"):byte(1, -1)))

-- varargs in the main chunk: the script receives command-line args (none)
print("main chunk", select('#', ...))

-- nested functions do not see the outer ... (they need their own)
local ok, err = load("local function outer(...) return function() return ... end end")
print("nested ... compile", ok, err)

-- more than 200 arguments via table.unpack
local big = {}
for i = 1, 250 do big[i] = i end
local function sum_count(...)
  local s = 0
  for i = 1, select('#', ...) do s = s + select(i, ...) end
  return s, select('#', ...)
end
print("250 args", sum_count(table.unpack(big)))
print("250 forwarded", count(ret1(table.unpack(big))))
local packed = table.pack(table.unpack(big))
print("250 packed", packed.n, packed[250])
-- 250 values in a constructor via ...
local function into_table(...) return {...} end
print("250 ctor", #into_table(table.unpack(big)))

-- vararg function with fixed params
local function fixed(a, b, ...)
  return show(a, b), show(...)
end
print("fixed", fixed(1))
print("fixed extra", fixed(1, 2, 3, 4))
-- extra arguments to non-vararg functions are dropped
local function two(a, b) return show(a, b) end
print("dropped", two(1, 2, 3, 4), two())

-- ... used multiple times
local function twice(...) return show(...), show(...) end
print("twice", twice("a", nil))
-- select('#', ...) after modifying a local copy
local function cp(...)
  local t = table.pack(...)
  t[1] = "changed"
  return show(...), show(table.unpack(t, 1, t.n))
end
print("unaffected", cp("orig", 2))
