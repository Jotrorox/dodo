-- string literals: escapes, long brackets, constants
local s1 = "plain"
local s2 = 'single \'quoted\' "x"'
local s3 = "escapes: \a\b\f\n\r\t\v\\\"\'"
local s4 = "hex \x41\x42\x7a and dec \65\066\0677 and zero \0 end"
local s5 = "unicode \u{48}\u{7FF}\u{FFFF}\u{10FFFF}\u{7FFFFFFF}"
local s6 = "skip \z
            spaces"
local s7 = [[long
string]]
local s8 = [==[
first newline skipped ]] still inside ]=] ]==]
local s9 = [[]]
local s10 = "line\
continued"
local s11 = "a very long string constant that is definitely longer than forty bytes"
local s12 = "a very long string constant that is definitely longer than forty bytes"
local s13 = s11 .. s12 .. "short"
--[[ long
comment ]] local s14 = "after comment"
--[==[ another ]] ]==] local s15 = 1
return s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11, s12, s13, s14, s15
