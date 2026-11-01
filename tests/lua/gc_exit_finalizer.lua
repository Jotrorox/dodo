-- Finalizers of objects still alive when the script ends run when lua.c
-- closes the state (lua_close), after the main chunk returns, in reverse
-- order of being marked; objects referenced from globals are finalized
-- too. Finalizers can still use the standard library (print, string).

-- automatic collection is stopped so that no object is finalized early
collectgarbage("stop")

local function make(name)
  return setmetatable({name = name}, {__gc = function(o)
    print(string.format("finalizing %s at exit", o.name))
  end})
end

global_obj = make("global object")
local local_obj = make("local object")
local chain = make("chain head")
chain.next = make("chain tail")
setmetatable({}, {__gc = function() print("garbage finalized at exit") end})

-- a finalizer that errors at exit is ignored (warnings are off)
setmetatable({}, {__gc = function() error("ignored at exit") end})

print("main chunk ends")
