-- ipairs: iterates 1..n until the first nil, honours __index (5.4), does
-- not use __len, returns a 3-value iterator triple, and works with
-- proxies. Also stateless iteration by calling the iterator directly.

local function collect(t)
  local out = {}
  for i, v in ipairs(t) do out[#out + 1] = i .. "=" .. tostring(v) end
  return table.concat(out, " ")
end

print("seq", collect({"a", "b", "c"}))
print("empty", "[" .. collect({}) .. "]")
print("stops at nil", collect({1, 2, nil, 4}))
print("starts nil", "[" .. collect({nil, 2, 3}) .. "]")
print("ignores hash", collect({1, 2, x = 3, [0] = 0, [4] = "four"}))
print("false is not nil", collect({true, false, true}))

-- iterator triple
local f, s, init = ipairs({})
print("triple", type(f), init)
local t = {"x", "y"}
local f2, s2 = ipairs(t)
print("same state", s2 == t)
-- calling the iterator directly
print("direct", f2(t, 0))
print("direct 1", f2(t, 1))
print("direct end", f2(t, 2))
print("direct far", f2(t, 10))
print("direct negative", f2({[-1] = "neg", [0] = "zero"}, -2))
-- the same iterator function for all tables
print("shared iterator", select(1, ipairs({})) == select(1, ipairs({1})))

-- __index is honoured
local backing = {"p", "q", "r"}
local proxy = setmetatable({}, {__index = backing})
print("proxy", collect(proxy))
local gen = setmetatable({}, {__index = function(_, i) if i <= 4 then return i * i end end})
print("generated", collect(gen))
-- partial own values plus __index fallback
local partial = setmetatable({"own1", nil, "own3"}, {__index = function(_, i) if i == 2 then return "idx2" end end})
print("partial", collect(partial))
-- __len is ignored by ipairs
local lenlie = setmetatable({1, 2, 3}, {__len = function() return 1 end})
print("ignores __len", collect(lenlie))
-- __index errors propagate
local boom = setmetatable({}, {__index = function(_, i) error("no index " .. i, 0) end})
print("index error", pcall(collect, boom))

-- modifying values during ipairs (assigning existing indices)
local m = {1, 2, 3}
for i, v in ipairs(m) do m[i] = v * 100 end
print("modified", collect(m))
-- appending during ipairs keeps going until a nil is met
local grow = {1}
local steps = 0
for i, v in ipairs(grow) do
  steps = steps + 1
  if i < 5 then grow[i + 1] = v + 1 end
end
print("grow", steps, collect(grow))
-- truncating during ipairs stops early
local cut = {1, 2, 3, 4, 5}
local seen = {}
for i, v in ipairs(cut) do
  seen[#seen + 1] = v
  if i == 2 then cut[3] = nil end
end
print("cut", table.concat(seen, ","))

-- errors
print("ipairs none", pcall(ipairs))
local oki, fi, si, ii = pcall(ipairs, nil)
print("ipairs nil", oki, fi == ipairs({}), si, ii)
print("ipairs number iter", pcall(function() for _ in ipairs(5) do end end))
-- strings index through the string metatable: ("abc")[1] is nil
print("ipairs string", pcall(function() for _, c in ipairs("abc") do return c end end))
-- break out of ipairs
local found
for i, v in ipairs({"a", "b", "target", "c"}) do
  if v == "target" then found = i; break end
end
print("break", found)
