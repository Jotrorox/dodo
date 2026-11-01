-- Classic coroutine patterns: producer/consumer pipelines with filters,
-- recursive generators (tree walk, permutations), errors raised inside
-- coroutines (resume returns false + message), deep recursion inside a
-- coroutine, and many coroutines alive at once.

-- producer / filter / consumer
local function producer(items)
  return coroutine.create(function()
    for _, v in ipairs(items) do coroutine.yield(v) end
    return nil
  end)
end
local function filter(src, pred)
  return coroutine.create(function()
    while true do
      local ok, v = coroutine.resume(src)
      if v == nil then return nil end
      if pred(v) then coroutine.yield(v) end
    end
  end)
end
local function consume(src)
  local out = {}
  while true do
    local ok, v = coroutine.resume(src)
    if not ok then error(v) end
    if v == nil then break end
    out[#out + 1] = v
  end
  return table.concat(out, " ")
end
print("evens:", consume(filter(producer({1, 2, 3, 4, 5, 6, 7, 8}), function(x) return x % 2 == 0 end)))

-- recursive tree walk generator
local tree = {v = 4, l = {v = 2, l = {v = 1}, r = {v = 3}}, r = {v = 6, l = {v = 5}, r = {v = 7}}}
local function walk(t)
  if t then walk(t.l); coroutine.yield(t.v); walk(t.r) end
end
local inorder = {}
for v in coroutine.wrap(function() walk(tree) end) do inorder[#inorder + 1] = v end
print("inorder:", table.concat(inorder, ","))

-- permutations generator
local function permgen(a, n)
  n = n or #a
  if n <= 1 then coroutine.yield(a) return end
  for i = 1, n do
    a[n], a[i] = a[i], a[n]
    permgen(a, n - 1)
    a[n], a[i] = a[i], a[n]
  end
end
local perms = {}
for p in coroutine.wrap(function() permgen({"a", "b", "c"}) end) do
  perms[#perms + 1] = table.concat(p)
end
print("perms:", #perms, table.concat(perms, " "))

-- errors inside coroutines
local co = coroutine.create(function(x)
  coroutine.yield(x)
  local y = x + nil
end)
print(coroutine.resume(co, 1))
print(coroutine.resume(co))
print(coroutine.status(co))
co = coroutine.create(function() error({msg = "tbl"}) end)
local ok, e = coroutine.resume(co)
print(ok, type(e), e.msg)
co = coroutine.create(function() error("lvl2", 2) end)
print(coroutine.resume(co))
co = coroutine.create(function() local function f() error("in f", 2) end f() end)
print(coroutine.resume(co))
-- error caught inside the coroutine does not kill it
co = coroutine.create(function()
  local ok2, m = pcall(error, "inner", 0)
  coroutine.yield(ok2, m)
  return "alive"
end)
print(coroutine.resume(co))
print(coroutine.resume(co))

-- deep recursion inside a coroutine, yielding at the bottom
local function deep(n)
  if n == 0 then return coroutine.yield("bottom") end
  return 1 + deep(n - 1)
end
co = coroutine.create(deep)
print(coroutine.resume(co, 5000))
print(coroutine.resume(co, 0))
-- stack overflow inside a coroutine is a catchable error
local function inf(n) return 1 + inf(n + 1) end
co = coroutine.create(inf)
local ok3, m3 = coroutine.resume(co, 1)
print("overflow:", ok3, string.find(m3, "stack overflow", 1, true) ~= nil)

-- many live coroutines
local cos = {}
for i = 1, 200 do
  cos[i] = coroutine.wrap(function() local s = i; while true do s = s + coroutine.yield(s) end end)
  cos[i](0)
end
local total = 0
for round = 1, 3 do for i = 1, 200 do total = total + cos[i](round) end end
print("total:", total)

-- breaking out of a for over a wrap does NOT close the coroutine (no 4th value)
local closed = false
local function range(n)
  return coroutine.wrap(function()
    local guard <close> = setmetatable({}, {__close = function() closed = true end})
    for i = 1, n do coroutine.yield(i) end
  end)
end
for i in range(10) do if i == 3 then break end end
print("guard closed after break:", closed)
