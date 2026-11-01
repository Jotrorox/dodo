-- Patterns: string.gsub with string, table and function replacements, n limit
local function show(label, ...)
  local t = table.pack(...)
  for i = 1, t.n do t[i] = type(t[i]) == "string" and "'" .. t[i] .. "'" or tostring(t[i]) end
  print(label, table.concat(t, " "))
end

-- string replacements
show("simple", ("hello world"):gsub("o", "0"))
show("no match", ("hello"):gsub("x", "y"))
show("%0 whole", ("abc"):gsub("%w", "<%0>"))
show("%1 capture", ("hello world"):gsub("(%w+)", "[%1]"))
show("swap", ("hello world"):gsub("(%w+) (%w+)", "%2 %1"))
show("%% literal", ("50"):gsub("%d+", "%0%%"))
show("%1 without captures", ("abc"):gsub("b", "%1%1"))
show("position capture", ("abc"):gsub("()b", "%1"))
show("empty pattern", ("abc"):gsub("", "-"))
show("star pattern", ("abc"):gsub("x*", "-"))
show("word star", ("hello world"):gsub("%w*", "X"))
show("anchored", ("aaa"):gsub("^a", "b"))
show("anchored empty", ("abc"):gsub("^", ">"))
show("end anchor", ("abc"):gsub("$", "<"))
show("anchored all", ("abc"):gsub("^.*$", "[%0]"))
show("empty subject", (""):gsub("", "x"))
show("empty subject star", (""):gsub(".*", "x"))
show("number repl", ("abc"):gsub("b", 5))
show("float repl", ("abc"):gsub("b", 1.5))
show("repl with zero", ("a-b"):gsub("-", "\0") == "a\0b")
show("%9 digits", ("abcdefghi"):gsub("(.)(.)(.)(.)(.)(.)(.)(.)(.)", "%9%8%7%6%5%4%3%2%1"))
show("lazy per match", ("<a><bb>"):gsub("<(.-)>", "%1"))
show("trim", ("  x y  "):gsub("^%s+", ""):gsub("%s+$", ""))

-- n limit
show("n=1", ("aaaa"):gsub("a", "b", 1))
show("n=2", ("aaaa"):gsub("a", "b", 2))
show("n=0", ("aaaa"):gsub("a", "b", 0))
show("n=-1", ("aaaa"):gsub("a", "b", -1))
show("n=100", ("aaaa"):gsub("a", "b", 100))
show("n=2.0", ("aaaa"):gsub("a", "b", 2.0))
show("n='3'", ("aaaa"):gsub("a", "b", "3"))
show("n=1 empty", ("abc"):gsub("", "-", 2))
print("n=1.5", pcall(string.gsub, "aaaa", "a", "b", 1.5))

-- table replacement: key is first capture (or whole match)
local t = {hello = "HI", world = false, num = 42, flt = 2.5}
show("table", ("hello world num flt other"):gsub("%w+", t))
show("table capture key", ("$hello $num $x"):gsub("%$(%w+)", t))
show("table position key", ("abc"):gsub("()", {[1] = "<1>", [3] = "<3>"}))
show("table metamethod", ("a b"):gsub("%w", setmetatable({}, {__index = function(_, k) return k:upper() end})))
print("table bad value", pcall(string.gsub, "x", "x", {x = {}}))
print("table bad bool", pcall(string.gsub, "x", "x", {x = true}))

-- function replacement
show("function upper", ("hello world"):gsub("%w+", string.upper))
show("function captures", ("a=1, b=2"):gsub("(%w)=(%w)", function(k, v) return v .. "=" .. k end))
show("function nil keeps", ("abc"):gsub("%w", function(c) if c == "b" then return "B" end end))
show("function false keeps", ("abc"):gsub("%w", function() return false end))
show("function number", ("abc"):gsub("%w", function(c) return c:byte() end))
show("function position", ("abc"):gsub("()%w", function(p) return p end))
show("function extra results", ("ab"):gsub("%w", function(c) return c, "ignored" end))
show("function empty", ("ab"):gsub("", function() return "|" end))
local count = 0
show("function called per match", ("a1b2c3"):gsub("%d", function() count = count + 1 end), count)
print("function bad value", pcall(string.gsub, "x", "x", function() return {} end))
print("function error", pcall(string.gsub, "x", "x", function() error("in repl") end))

-- argument errors
print("repl bool", pcall(string.gsub, "x", "x", true))
print("repl nil", pcall(string.gsub, "x", "x"))
print("bad %", pcall(string.gsub, "x", "x", "%a"))
print("trailing %", pcall(string.gsub, "x", "x", "abc%"))
print("bad capture index", pcall(string.gsub, "x", "(x)", "%2"))
print("bad capture no captures", pcall(string.gsub, "x", "x", "%2"))
print("subject number", string.gsub(12321, 2, 0))
