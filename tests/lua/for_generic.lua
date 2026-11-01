-- Generic for: the iterator triple (function, state, control), stateless
-- iterators, closure-based iterators, the loop ending when the first value
-- is nil (false does not end it), extra values, and the 5.4 fourth value
-- (closing value) whose __close runs on normal end, break, return, goto
-- out, and error.

local function stateless(t, i)
  i = i + 1
  if t[i] ~= nil then return i, t[i] end
end
local out = {}
for i, v in stateless, {"a", "b", "c"}, 0 do out[#out + 1] = i .. v end
print("stateless", table.concat(out, " "))

-- the iterator receives the state and the previous control value
local log = {}
local function spy(s, c)
  log[#log + 1] = s .. ":" .. tostring(c)
  if c == nil then return 1 end
  if c < 3 then return c + 1 end
end
for _ in spy, "S" do end
print("args", table.concat(log, " "))

-- false as first value does not end the loop; nil does
local k = 0
local function fiter(_, c)
  k = k + 1
  if k <= 2 then return false, k end
end
local count = 0
for a, b in fiter do count = count + 1 end
print("false continues", count)

-- more than one returned value; missing values are nil
local function multi(_, c)
  c = c + 1
  if c <= 2 then return c, c * 10, c * 100 end
end
for a, b, c, d in multi, nil, 0 do print("multi", a, b, c, d) end

-- iterator expression list is adjusted to 4 values
local function triple() return stateless, {"x"}, 0 end
for i, v in triple() do print("from call", i, v) end
-- with parentheses only the function survives: state and control are nil
print("paren call", pcall(function() for i, v in (triple()) do end end))
-- extra values beyond 4 are dropped
local function five() return stateless, {"y"}, 0, nil, "ignored" end
for i, v in five() do print("five values", i, v) end

-- closing value (4th value): __close called when the loop ends
local closelog = {}
local function closer(name)
  return setmetatable({}, {__close = function(_, err)
    local e = type(err) == "table" and ("table code=" .. tostring(err.code)) or tostring(err)
    closelog[#closelog + 1] = name .. "(" .. e .. ")"
  end})
end
local function iter_with_close(n, name)
  local function it(_, c)
    c = c + 1
    if c <= n then return c end
  end
  return it, nil, 0, closer(name)
end
local function flush(label)
  print(label, table.concat(closelog, " "))
  closelog = {}
end

for i in iter_with_close(3, "normal") do end
flush("normal end")
for i in iter_with_close(3, "brk") do if i == 2 then break end end
flush("break")
local function early()
  for i in iter_with_close(3, "ret") do if i == 2 then return "returned" .. i end end
end
print("return value", early())
flush("return")
do
  for i in iter_with_close(3, "goto") do if i == 1 then goto out end end
  ::out::
end
flush("goto out")
local ok, err = pcall(function()
  for i in iter_with_close(3, "err") do if i == 2 then error("boom", 0) end end
end)
print("error", ok, err)
flush("error")
ok, err = pcall(function()
  for i in iter_with_close(3, "errtab") do error({code = 7}) end
end)
print("error table", ok, type(err), err.code)
flush("error table")
-- nested loops close inner first
for i in iter_with_close(2, "outer") do
  for j in iter_with_close(2, "inner" .. i) do end
end
flush("nested")
-- break from inner loop closes only the inner one
for i in iter_with_close(1, "o") do
  for j in iter_with_close(5, "i") do break end
  closelog[#closelog + 1] = "between"
end
flush("inner break")
-- nil and false closing values are allowed
for i in stateless, {1}, 0, nil do end
for i in stateless, {1}, 0, false do end
print("nil/false closing ok")
-- a non-closable closing value is an error raised before the loop starts
print("non-closable", pcall(function() for i in stateless, {1}, 0, {} do end end))
print("non-closable number", pcall(function() for i in stateless, {1}, 0, 42 do end end))
-- an error in the iterator also closes
ok, err = pcall(function()
  for i in function() error("iter fail", 0) end, nil, nil, closer("iterr") do end
end)
print("iterator error", ok, err)
flush("iterator error")
