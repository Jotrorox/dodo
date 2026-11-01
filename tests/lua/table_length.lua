-- The length operator # on proper sequences (no holes): constructors,
-- growth by t[#t+1], shrinking by t[#t]=nil, strings, and tables whose
-- non-sequence keys (hash part, negative, zero, float) do not count.

local t = {}
print("empty", #t)
for i = 1, 100 do t[#t + 1] = i * 2 end
print("grown", #t, t[1], t[100])
for i = 1, 40 do t[#t] = nil end
print("shrunk", #t, t[60], t[61])
while #t > 0 do t[#t] = nil end
print("emptied", #t, next(t))

-- non-positive and non-integer keys do not affect a sequence
local s = {1, 2, 3, [0] = "zero", [-1] = "neg", [1.5] = "half", x = "x"}
print("with extras", #s)
local s2 = {[0] = "only zero"}
print("only zero", #s2)
local s3 = {x = 1, y = 2}
print("only hash", #s3)

-- sequence built entirely in the hash part
local h = {}
h[3] = "c"; h[1] = "a"; h[2] = "b"
print("reverse built", #h)
local h2 = {[1] = 1, [2] = 2, [3] = 3, [4] = 4}
print("bracket ctor", #h2)

-- float keys that are integral count as the integer key
local f = {}
f[1.0] = "a"; f[2.0] = "b"; f[3] = "c"
print("float keys", #f)

-- # on strings is the byte count
print("str", #"", #"abc", #"\0\0", #"h\u{e9}llo", #("x"):rep(1000))
local str = "hello"
print("str var", #str, #str .. "!", -#str)

-- precedence: # binds tighter than arithmetic and concat
print("prec", #"ab" + 1, #"abc" * 2, 2 ^ #"ab", #{1, 2} .. "x")

-- # of {} vs #{nil}: both zero
print("nil ctor", #{nil}, #{nil, nil})

-- length of a sequence with trailing nil from constructor of varargs
local function pack(...) return {...} end
print("pack", #pack(1, 2, 3), #pack())

-- large sequence
local big = {}
for i = 1, 10000 do big[i] = i end
print("big", #big)
big[#big] = nil
print("big-1", #big)

-- length used in loops
local acc = {}
local words = {"alpha", "beta", "gamma"}
for i = 1, #words do acc[#acc + 1] = words[i]:upper() end
print(table.concat(acc, ","))

-- errors: # on non-string non-table values
for _, v in ipairs({1, true, 2.5}) do
  local ok, err = pcall(function() return #v end)
  print("len err", type(v), ok, err)
end
local ok, err = pcall(function() local n = nil; return #n end)
print("len nil", ok, err)
ok, err = pcall(function() return #print end)
print("len fn", ok, err)
