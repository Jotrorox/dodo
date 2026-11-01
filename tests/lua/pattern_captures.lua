-- Patterns: captures, position captures, %b balance, %f frontier, back-references
local function show(label, ...)
  local t = table.pack(...)
  for i = 1, t.n do t[i] = type(t[i]) == "string" and "'" .. t[i] .. "'" or tostring(t[i]) end
  print(label, t.n, table.concat(t, " "))
end

-- captures
show("nested", ("abcd"):match("((a)(b))((c)d)"))
show("capture order", ("key = value"):match("(%w+)%s*=%s*(%w+)"))
show("empty capture", ("abc"):match("a()()b"))
show("empty string capture", ("abc"):match("a(x*)b"))
show("position only", ("hello"):find("()ll()"))
show("position at end", ("abc"):match("()$"))
show("position start", ("abc"):match("^()"))
show("capture with quantifier", ("aaa"):match("(a)*"))
show("optional capture group", ("ab"):match("(a?)(b?)(c?)"))
show("whole match when no captures", ("xyz"):match("y"))
show("find returns captures", ("x=1"):find("(%w)=(%d)"))
show("max captures 32", ("abcdefghijklmnopqrstuvwxyzABCDEF"):match(("(.)"):rep(32)))

-- %b balanced matches
show("%b()", ("x(a(b)c)y"):match("%b()"))
show("%b() first only", ("(a)(b)"):match("%b()"))
show("%b unbalanced", ("((a)"):match("%b()"))
show("%b none", ("abc"):match("%b()"))
show("%b[]", ("t[1][2]"):gsub("%b[]", "<>"))
show("%b same char", ('say "hi" now'):match('%b""'))
show("%b xy", ("xaxbyy"):match("%bxy"))
show("%b in capture", ("f(a, (b))"):match("(%w+)(%b())"))
show("%b{} nested", ("{{}{}}x"):match("^%b{}"))

-- %f frontier patterns
show("%f words", ("THE (quick) fox"):gsub("%f[%a]%a+", "W"))
show("%f end of word", ("hello world"):gsub("%f[%A]", "|"))
show("%f at start", ("abc"):find("%f[%a]"))
show("%f at end", ("abc"):find("%f[%z]"))
show("%f at end with \\0 class", ("abc"):find("%f[\0]"))
show("%f digits", ("a1b22c333"):gsub("%f[%d]%d+", "#"))
show("%f whole word", ("the then other the"):gsub("%f[%w]the%f[%W]", "THE"))
show("%f no match", ("aaa"):find("%f[%d]"))
show("%f with set", ("ab12cd"):match("%f[0-9](%d+)"))
show("%f negated", ("  word"):find("%f[^%s]"))

-- back-references
show("%1 quotes", ([[say 'hi' and "bye"]]):match("(['\"])(.-)%1"))
show("%1 double", ("hello"):match("(l)%1"))
show("%1 none", ("helo"):match("(l)%1"))
show("%2 order", ("abba"):match("(a)(b)%2%1"))
show("%1 of empty", ("abc"):match("(x*)a%1b"))
show("repeat word", ("this is is a test"):match("(%a+) %1"))
show("gsub backref", ("aa bb cd"):gsub("(%a)%1", "<%1%1>"))

-- anchors with captures
show("anchored capture", ("  trim  "):match("^%s*(.-)%s*$"))
show("anchored empty", (""):match("^(.-)$"))
show("multiple anchors literal", ("^^"):match("^^"))
show("anchor in middle literal", ("a^"):match("a^"))
