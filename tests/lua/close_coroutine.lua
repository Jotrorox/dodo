-- <close> variables inside coroutines: pending variables of a suspended
-- coroutine are closed by coroutine.close (with nil error), a coroutine
-- that dies with an error closes its variables during the error, a
-- coroutine that finishes normally closes on return, coroutine.close on
-- dead/suspended coroutines returns status values, errors in __close
-- reported by coroutine.close, and coroutine.wrap propagation.

local log = {}
local function res(name, fail)
  return setmetatable({}, {__close = function(_, err)
    log[#log + 1] = name .. "(" .. tostring(err) .. ")"
    if fail then error(fail, 0) end
  end})
end
local function flush(label)
  print(label, table.concat(log, " "))
  log = {}
end

-- suspended coroutine closed explicitly
local co = coroutine.create(function()
  local a <close> = res("a")
  local b <close> = res("b")
  coroutine.yield("y1")
  log[#log + 1] = "not reached"
end)
print("resume", coroutine.resume(co))
flush("before close")
print("close", coroutine.close(co))
flush("after close")
print("status", coroutine.status(co))
print("close again", coroutine.close(co))
print("resume closed", coroutine.resume(co))

-- normal completion closes on return
co = coroutine.create(function()
  local c <close> = res("c")
  coroutine.yield()
  return "done"
end)
coroutine.resume(co)
print("finish", coroutine.resume(co))
flush("normal")

-- an error inside the coroutine does not close its variables; a later
-- coroutine.close does, passing the error object, and returns it
co = coroutine.create(function()
  local d <close> = res("d")
  error("co error", 0)
end)
print("error resume", coroutine.resume(co))
flush("error (nothing closed)")
print("close dead with error", coroutine.close(co))
flush("closed after error")
print("close again after error", coroutine.close(co))

-- __close failing while being closed by coroutine.close
co = coroutine.create(function()
  local e <close> = res("e", "close failed")
  local f <close> = res("f")
  coroutine.yield()
end)
coroutine.resume(co)
print("close with failing handler", coroutine.close(co))
flush("failing handler")
print("status after", coroutine.status(co))

-- closing a never-started coroutine
co = coroutine.create(function() local g <close> = res("g") end)
print("close unstarted", coroutine.close(co))
flush("unstarted")

-- __close handlers may yield (allowed since 5.4.3)
co = coroutine.create(function()
  local y <close> = setmetatable({}, {__close = function()
    log[#log + 1] = "closing"
    coroutine.yield("from close")
    log[#log + 1] = "closed"
  end})
  return "body done"
end)
print("yield in close 1", coroutine.resume(co))
print("yield in close 2", coroutine.resume(co))
flush("yield in close")

-- coroutine.wrap: error closes and propagates to the caller
local w = coroutine.wrap(function()
  local h <close> = res("h")
  coroutine.yield(1)
  error("wrap error", 0)
end)
print("wrap 1", w())
print("wrap 2", pcall(w))
flush("wrap")

-- errors closing a coroutine that is not suspended or dead
print("close running", pcall(coroutine.close, coroutine.running()))
local outer
outer = coroutine.create(function()
  local inner = coroutine.create(function() return pcall(coroutine.close, outer) end)
  return coroutine.resume(inner)
end)
print("close normal", coroutine.resume(outer))
print("close bad arg", pcall(coroutine.close, 42))

-- a to-be-closed variable in the main function of a coroutine yields its
-- value through multiple resumes before closing
co = coroutine.wrap(function()
  local r <close> = res("multi")
  for i = 1, 3 do coroutine.yield(i) end
end)
print("multi", co(), co(), co())
co()
flush("multi")
