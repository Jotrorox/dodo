-- Weak tables via __mode: "k" (weak keys / ephemerons), "v" (weak values),
-- "kv"; collected entries disappear after a full collectgarbage(); strings,
-- numbers and booleans are values (never removed); ephemeron semantics
-- (a value referring to its own key does not keep the key alive).

local function count(t)
  local n = 0
  for _ in pairs(t) do n = n + 1 end
  return n
end

-- weak values
local wv = setmetatable({}, {__mode = "v"})
local strong = {}
wv[1] = {}            -- only referenced from wv: collectable
wv[2] = strong        -- also referenced from a local
wv[3] = "a string"    -- strings are not collectable values here
wv[4] = 42
wv[5] = true
wv.f = function() end -- a fresh closure, otherwise unreferenced
collectgarbage()
print("weak v", wv[1], wv[2] == strong, wv[3], wv[4], wv[5], wv.f)

-- weak keys
local wk = setmetatable({}, {__mode = "k"})
local kstrong = {}
wk[{}] = "dropped"
wk[kstrong] = "kept"
wk["str"] = "string key kept"
wk[10] = "number key kept"
collectgarbage()
print("weak k", count(wk), wk[kstrong], wk.str, wk[10])

-- weak keys and values
local wkv = setmetatable({}, {__mode = "kv"})
local k2, v2 = {}, {}
wkv[k2] = {}          -- value collectable
wkv[{}] = v2          -- key collectable
wkv[k2 ~= nil and "x" or "y"] = v2 -- string key, strong value
wkv.y = {}            -- string key, collectable value
collectgarbage()
print("weak kv", count(wkv), wkv[k2], wkv.x == v2, wkv.y)

-- ephemeron: value references its own key, key unreachable otherwise
local eph = setmetatable({}, {__mode = "k"})
do
  local key = {}
  eph[key] = {ref = key}
end
collectgarbage()
print("ephemeron cycle collected", count(eph))
-- chain of ephemerons: k1 -> v1 refers to k2 -> v2 ... only k1 strong
local chain = setmetatable({}, {__mode = "k"})
local k1 = {}
do
  local ka, kb = {}, {}
  chain[k1] = ka
  chain[ka] = kb
  chain[kb] = "end"
end
collectgarbage()
print("ephemeron chain kept", count(chain))
k1 = nil
collectgarbage()
print("ephemeron chain dropped", count(chain))

-- values reachable from a strong place survive in a weak table
local cache = setmetatable({}, {__mode = "v"})
local function get(name)
  local o = cache[name]
  if not o then o = {name = name}; cache[name] = o end
  return o
end
local held = get("held")
get("temp")
collectgarbage()
print("cache", cache.held == held, cache.temp)

-- __mode set after entries were added still applies at the next collection
local late = {}
late[1] = {}
setmetatable(late, {__mode = "v"})
collectgarbage()
print("late mode", late[1])

-- weak table entries pointing to objects with finalizers: values are
-- removed before running finalizers, keys after (resurrected keys remain
-- until the next cycle)
local log = {}
local wf = setmetatable({}, {__mode = "v"})
do
  wf[1] = setmetatable({}, {__gc = function() log[#log + 1] = "fin" end})
end
collectgarbage()
print("finalized value", wf[1], table.concat(log))

-- strings as weak keys/values are never collected
local ws = setmetatable({}, {__mode = "kv"})
ws[string.rep("k", 10)] = string.rep("v", 10)
collectgarbage()
print("strings stay", ws[string.rep("k", 10)])

-- invalid modes are ignored (no error)
local bad = setmetatable({}, {__mode = "xyz"})
bad[1] = {}
collectgarbage()
print("bad mode keeps", type(bad[1]))
