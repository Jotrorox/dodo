-- Lexer: long brackets (strings and comments) with levels, newline handling
local function esc(s)
  return (s:gsub("[%c\\]", function(c)
    if c == "\n" then return "\\n" end
    if c == "\r" then return "\\r" end
    if c == "\\" then return "\\\\" end
    return string.format("\\%d", c:byte())
  end))
end
local function show(label, s) print(label, #s, "[" .. esc(s) .. "]") end

show("empty", [[]])
show("simple", [[abc]])
show("leading newline skipped", [[
line1
line2]])
show("only first newline skipped", [[

x]])
show("trailing newline kept", [[x
]])
show("level1", [=[a]]b]=])
show("level2", [==[ ]=] ]] ]===] ]==])
show("nested open brackets", [==[[[nested]]]==])
show("no escapes", [[\n\t\\\z]])
show("quotes", [["'"']])
show("close at level0 inside level1", [=[]]]=])
show("ends with bracket", [=[x]]=])
show("empty level3", [===[]===])
show("only newline", [[
]])

-- carriage returns are normalised to "\n" (any of \n, \r, \r\n, \n\r count once)
local cases = {
  "return [[\r\nabc]]",
  "return [[\n\rabc]]",
  "return [[\rabc]]",
  "return [[a\r\nb\n\rc\rd\ne]]",
  "return [[a\r\r\nb]]",
  "return [[a\n\n\rb]]",
  "return [==[\r\n]==]",
}
for i, src in ipairs(cases) do
  local f = assert(load(src))
  show("cr" .. i, f())
end

-- long comments
local x = 1 --[[ a long
comment ]] + 2
print("after comment", x)
local y = 10 --[==[ contains ]] and ]=] ]==] + 5
print("level2 comment", y)
--[ this is a line comment because the bracket is not doubled
print("single bracket comment ok")
--[= also a line comment
print("bracket-equals comment ok")
--[[]] print("code after empty long comment runs")
---[[ line comment trick
print("dash trick active")
--]]

-- long string used as an expression and method receiver
print(([[abc]]):upper(), #[[four]], [[a]] .. [=[b]=])
-- long brackets can be used where a string arg is expected
print(string.len[[12345]])
print(type[==[x]==])

-- line numbers continue to count inside long strings
local f = load("local s = [[\n\n\n]]\nerror('here')", "=chunk")
print(pcall(f))
f = load("--[[\n\n]] error('there')", "=chunk")
print(pcall(f))
f = load("local s = 'a\\\n\\\nb' error('cont')", "=chunk")
print(pcall(f))
