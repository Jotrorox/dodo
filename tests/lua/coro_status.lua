-- coroutine.status in every state (suspended, running, normal, dead),
-- coroutine.running in the main thread and inside coroutines,
-- coroutine.isyieldable, and resume errors for non-suspended coroutines.

local main, ismain = coroutine.running()
print("main running:", type(main), ismain)
print("main status:", coroutine.status(main))
print("main yieldable:", coroutine.isyieldable())
print("main yieldable(arg):", coroutine.isyieldable(main))

local A, B
A = coroutine.create(function()
  local self, im = coroutine.running()
  print("A: running is A", self == A, im)
  print("A: status(A)", coroutine.status(A))
  print("A: status(main)", coroutine.status(main))
  print("A: yieldable", coroutine.isyieldable(), coroutine.isyieldable(A))
  print("A: resume B ->", coroutine.resume(B))
  print("A: status(B) after B yielded", coroutine.status(B))
  coroutine.yield("A yielded")
  return "A finished"
end)
B = coroutine.create(function()
  print("B: status(A) while A resumes B", coroutine.status(A))
  print("B: status(B)", coroutine.status(B))
  print("B: resume A ->", coroutine.resume(A))
  print("B: resume main ->", coroutine.resume(main))
  print("B: resume self ->", coroutine.resume(B))
  coroutine.yield("B yielded")
end)

print("before:", coroutine.status(A), coroutine.status(B))
print("resume A ->", coroutine.resume(A))
print("after 1:", coroutine.status(A), coroutine.status(B))
print("resume A ->", coroutine.resume(A))
print("after 2:", coroutine.status(A), coroutine.status(B))
print("resume A again ->", coroutine.resume(A))
print("resume B ->", coroutine.resume(B))
print("after 3:", coroutine.status(B))
print("resume B again ->", coroutine.resume(B))

-- status of a coroutine that died with an error
local E = coroutine.create(function() error("boom", 0) end)
print("E resume ->", coroutine.resume(E))
print("E status:", coroutine.status(E))
print("E resume again ->", coroutine.resume(E))

-- running inside a wrap'ed function is a thread, not main
local w = coroutine.wrap(function()
  local co, im = coroutine.running()
  return type(co), im, coroutine.status(co), coroutine.isyieldable()
end)
print("wrap running:", w())

-- isyieldable inside a non-yieldable C call (sort comparator on main thread)
local seen
table.sort({3, 1, 2}, function(a, b)
  seen = coroutine.isyieldable()
  return a < b
end)
print("yieldable in sort cmp (main):", seen)
local co = coroutine.create(function()
  local s
  table.sort({3, 1, 2}, function(a, b) s = coroutine.isyieldable(); return a < b end)
  return s, coroutine.isyieldable()
end)
print("yieldable in sort cmp (coroutine):", coroutine.resume(co))

-- yield from the main thread
print(pcall(coroutine.yield, 1))

-- argument errors
print(pcall(coroutine.status, nil))
print(pcall(coroutine.status, {}))
print(pcall(coroutine.isyieldable, 1))
