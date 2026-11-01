-- Compile-time goto/label errors reported by load(): jumping into the
-- scope of a local, undefined (no visible) labels, duplicate labels
-- (including in nested blocks, which 5.4 forbids), jumping into a nested
-- block or out of a function, and 'break' outside a loop.

local function compile(label, src)
  local f, err = load(src, "=src")
  if f then print(label, "compiled") else print(label, err) end
end

compile("into local scope", [[
goto f
local x = 1
::f::
print(x)
]])
compile("into scope, label then statement", [[
do
  goto f
  local a
  ::f::
  a = 1
end
]])
-- allowed: label at end of block, only void statements follow
compile("end of block ok", [[
do
  goto f
  local a
  ::f::
end
]])
compile("end of block with ; ok", [[
do
  goto f
  local a
  ::f:: ; ;
end
]])
-- a label right before 'until' does not count as the end of the block,
-- because the until expression is still inside the scope of body locals
compile("end of block before until", [[
repeat
  goto f
  local a
  ::f::
until true
]])
compile("repeat local in until", [[
repeat
  if true then goto f end
  local stop = true
  ::f::
until stop
]])
compile("no visible label", "goto nowhere")
compile("no visible label in block", "do goto l end do ::l:: end")
compile("label in nested block", "goto inner; do ::inner:: end")
compile("label in enclosing function", [[
::outer::
local function f() goto outer end
]])
compile("label in loop body from outside", [[
goto body
while true do ::body:: end
]])
compile("duplicate label", "::a:: ::a::")
compile("duplicate label separated", [[
::dup::
local x = 1
::dup::
]])
compile("duplicate in nested block", [[
::same::
do ::same:: end
]])
compile("same name sibling blocks ok", "do ::s:: end do ::s:: end")
compile("same name in nested function ok", "::s:: local function f() ::s:: end")
compile("break outside loop", "break")
compile("break in function in loop", "while true do local function f() break end end")
compile("goto keyword as label", "::goto::")
compile("label syntax", "::1::")
compile("missing ::", "::lbl")
compile("goto missing name", "goto")
compile("backward jump past local ok", [[
local n = 0
::top::
local x = n
n = n + 1
if n < 3 then goto top end
return x
]])
-- the error line numbers point at the goto (or label) position
compile("line numbers", "\n\n\ngoto later\nlocal z\n::later::\nprint(z)")
compile("break then label", "while true do break ::l:: end")
-- a continue label right before 'end' of a loop with locals declared
-- after the goto is fine
compile("continue after local ok", [[
for i = 1, 3 do
  if i == 2 then goto continue end
  local sq = i * i
  ::continue::
end
]])
