-- debug.getinfo: fields for Lua functions, the main chunk and C
-- functions (source, short_src, what, line ranges, nups, nparams,
-- isvararg), name/namewhat for local, global, method, field and upvalue
-- calls, currentline at levels, istailcall, sorted activelines, invalid
-- options and levels.

local function S(info)
  return string.format("what=%s src=%s short=%s lines=%d-%d",
    info.what, info.source, info.short_src, info.linedefined, info.lastlinedefined)
end

local function sample(a, b, ...)
  local x = a
  return x
end
print(S(debug.getinfo(sample)))
local u = debug.getinfo(sample, "u")
print("nups/nparams/vararg", u.nups, u.nparams, u.isvararg)
print(S(debug.getinfo(1, "S")))
local m = debug.getinfo(1, "u")
print("main nups/nparams/vararg", m.nups, m.nparams, m.isvararg)
local c = debug.getinfo(print)
print(c.what, c.source, c.short_src, c.linedefined, c.lastlinedefined, c.currentline, c.nups, c.nparams, c.isvararg)

-- upvalue counts
local up1, up2 = 1, 2
local function uses() return up1 + up2, print end
print("nups with _ENV:", debug.getinfo(uses, "u").nups)
print("nups none:", debug.getinfo(function() return 1 end, "u").nups)

-- names and namewhat: who() reports how its caller was called
local function who(level)
  local i = debug.getinfo(level or 2, "n")
  return tostring(i.name) .. "/" .. tostring(i.namewhat)
end
local function localfn() local r = who(); return r end
function globalfn() local r = who(); return r end
local obj = {}
function obj.field() local r = who(); return r end
function obj:method() local r = who(); return r end
print(localfn(), globalfn(), obj.field(), obj:method())
local t = {sub = {fn = function() local r = who(); return r end}}
print(t.sub.fn(), t.sub["fn"]())
local function viaupvalue() return (localfn()) end
print(viaupvalue())
local alias = obj.method
print(alias(obj))
print(who(1))
-- a function called from a C function (pcall) has no name
print(select(2, pcall(localfn)))
-- tail-called function: no name information
local function tailer() return localfn() end
print(tailer())
-- metamethods are named "metamethod"
local mm = setmetatable({}, {__index = function() local r = who(); return r end,
                            __add = function() local r = who(); return r end})
print(mm.anything, mm + 1)
-- functions called by a for iterator
for v in function(_, i) if not i then local r = who(); return r end end do print("for iterator:", v) end
print(pcall(who, 3))

-- currentline and levels
local function lines()
  local here = debug.getinfo(1, "l").currentline
  local caller = debug.getinfo(2, "l").currentline
  return here, caller
end
print(lines())
print(debug.getinfo(1, "l").currentline)
-- level beyond the stack
print(debug.getinfo(100))
-- istailcall
local function tc() return debug.getinfo(1, "t").istailcall end
local function calltail() return tc() end
local function callnotail() return (tc()) end
print("tail:", calltail(), "not tail:", callnotail())

-- activelines (sorted)
local function act(x)
  if x then
    return 1
  end
  return 2
end
local L = {}
for line in pairs(debug.getinfo(act, "L").activelines) do L[#L + 1] = line end
table.sort(L)
print("activelines:", table.concat(L, ","))
print("C activelines:", debug.getinfo(print, "L").activelines)

-- selected fields only
local sel = debug.getinfo(sample, "S")
print(sel.currentline, sel.nups, sel.name, sel.what)
local fsel = debug.getinfo(sample, "f")
print(fsel.func == sample, fsel.source)
-- the 'r' option: transfer info (outside hooks these are 0)
local r = debug.getinfo(1, "r")
print("ftransfer/ntransfer:", r.ftransfer, r.ntransfer)

-- invalid options and arguments
print(pcall(debug.getinfo, 1, ">S"))
print(pcall(debug.getinfo, sample, "X"))
print(pcall(debug.getinfo, {}))
print(pcall(debug.getinfo))
-- getinfo of a coroutine's frames
local co = coroutine.create(function() coroutine.yield() end)
coroutine.resume(co)
local ci = debug.getinfo(co, 1, "Sl")
print("coroutine level 1:", ci and ci.what, ci and ci.currentline)
print("coroutine level 0:", debug.getinfo(co, 0, "n").name)
