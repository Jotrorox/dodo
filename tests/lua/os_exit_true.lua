-- os.exit(true) exits with EXIT_SUCCESS (0) even when called from deep
-- inside nested functions and a coroutine.

local function deep(n)
  if n == 0 then
    print("exiting from depth")
    os.exit(true)
  end
  deep(n - 1)
end
coroutine.wrap(function() deep(10) end)()
print("not reached")
