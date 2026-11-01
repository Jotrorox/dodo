-- Patterns: string.gmatch iteration, captures, anchors, init argument, empty matches
local function collect(s, p, init)
  local out = {}
  for a, b in string.gmatch(s, p, init) do
    local item = type(a) == "string" and "'" .. a .. "'" or tostring(a)
    if b ~= nil then item = item .. ":" .. (type(b) == "string" and "'" .. b .. "'" or tostring(b)) end
    out[#out + 1] = item
    if #out > 20 then out[#out + 1] = "..."; break end
  end
  return #out .. " [" .. table.concat(out, " ") .. "]"
end
local function show(label, s, p, init) print(label, collect(s, p, init)) end

show("words", "one two  three", "%a+")
show("pairs", "a=1, b=2, c=3", "(%w+)=(%w+)")
show("positions", "abcabc", "()b")
show("pos and capture", "k1 k2", "()(k%d)")
show("empty matches", "abc", "x*")
show("empty pattern", "abc", "")
show("word star", "hello world", "%w*")
show("lazy empty", "abc", ".-")
show("digits", "a1b22c333", "%d+")
show("single chars", "xyz", ".")
show("no match", "abc", "%d")
show("empty subject", "", ".*")
show("empty subject letters", "", "%a")
-- in gmatch a leading "^" is not an anchor: it matches a literal caret
show("caret is literal", "aaa", "^a")
show("caret literal matches", "x^a^ab", "^a")
show("anchor no match", "baa", "^a")
show("anchor all", "abc", "^.*")
show("anchor empty", "abc", "^")
show("dollar", "abc", "c$")
show("dollar mid", "a$b$", "%w%$")
show("frontier words", "the cat sat", "%f[%a]%a")
show("balanced", "(a)(b(c))", "%b()")
show("overlapping not repeated", "aaaa", "aa")

-- init argument (Lua 5.4)
show("init 2", "abcabc", "a", 2)
show("init 4", "abcabc", "a", 4)
show("init -3", "abcabc", "%a", -3)
show("init 0", "abc", "%a", 0)
show("init -100", "abc", "%a", -100)
show("init past end", "abc", "%a", 10)
show("init #s+1 empty", "abc", "", 4)
show("init past end empty", "abc", "", 5)
show("init with anchor", "xab", "^a", 2)
show("init with anchor fail", "xab", "^b", 2)
show("init float", "abc", "%a", 2.0)
print("init bad", pcall(string.gmatch, "abc", "a", 1.5))
print("init string", collect("abc", "%a", "2"))

-- the iterator is a plain function and can be called manually
local it = ("k=v; x=y"):gmatch("(%w)=(%w)")
print(type(it), it())
print(it())
print(it())
print(it())
-- gmatch on numbers (coerced to strings)
print(collect(1234.5, "%d"))
-- independent iterators
local i1, i2 = ("ab"):gmatch("."), ("ab"):gmatch(".")
print(i1(), i2(), i1(), i2(), i1())
-- errors are raised when the pattern is used, not when the iterator is created
local bad = string.gmatch("abc", "(")
print(type(bad), pcall(bad))
print(pcall(string.gmatch, "abc"))
