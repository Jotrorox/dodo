-- Program test: a Lua-value serializer that emits loadable Lua source.
-- Handles shared references and cycles (by emitting a builder function that
-- patches references after construction), sorted keys of mixed types
-- (numbers before strings before booleans), sequences vs. hash parts,
-- float/integer distinctions (including inf, -inf, nan, -0.0, mininteger),
-- string escaping via %q, identifier-like keys, and a pretty printer with
-- depth limits. Round-trips through load() and checks deep equality that
-- respects sharing structure.

local function sort_keys(a, b)
  local ta, tb = type(a), type(b)
  if ta ~= tb then
    local rank = {number = 1, string = 2, boolean = 3, table = 4}
    return (rank[ta] or 9) < (rank[tb] or 9)
  end
  if ta == "number" or ta == "string" then return a < b end
  if ta == "boolean" then return (not a) and b end
  return false
end

local function sorted_keys(t)
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = k end
  table.sort(keys, sort_keys)
  return keys
end

local function literal(v)
  local tv = type(v)
  if tv == "string" then return string.format("%q", v) end
  if tv == "number" then
    if v ~= v then return "(0/0)" end
    if v == math.huge then return "math.huge" end
    if v == -math.huge then return "-math.huge" end
    if math.type(v) == "integer" then
      if v == math.mininteger then return "math.mininteger" end
      return string.format("%d", v)
    end
    if v == 0 and 1 / v < 0 then return "-0.0" end
    return string.format("%q", v) -- hex float, exact
  end
  if tv == "boolean" or tv == "nil" then return tostring(v) end
  error("cannot serialize a " .. tv, 0)
end

local function keyrepr(k)
  if type(k) == "string" and k:match("^[%a_][%w_]*$") and not ({["and"]=1, ["end"]=1, ["nil"]=1, ["for"]=1})[k] then
    return k
  end
  return "[" .. literal(k) .. "]"
end

-- serialize: first pass counts references, second pass emits
local function serialize(root)
  local refs, order = {}, {}
  local function count(v)
    if type(v) ~= "table" then return end
    if refs[v] then refs[v] = refs[v] + 1 return end
    refs[v] = 1
    order[#order + 1] = v
    for _, k in ipairs(sorted_keys(v)) do
      count(k)
      count(v[k])
    end
  end
  count(root)

  local names, nnames = {}, 0
  for _, t in ipairs(order) do
    if refs[t] > 1 then nnames = nnames + 1; names[t] = "t" .. nnames end
  end

  local lines, patches = {"local t = {}"}, {}
  local emitting = {}
  local function emit(v)
    if type(v) ~= "table" then return literal(v) end
    if names[v] and emitting[v] == "done" then return "t." .. names[v] end
    if emitting[v] == "open" then return nil end -- cycle: patch later
    emitting[v] = "open"
    local parts = {}
    local n = 0 -- length of the 1..n prefix (avoids # on tables with holes)
    while v[n + 1] ~= nil do n = n + 1 end
    local keys = sorted_keys(v)
    for _, k in ipairs(keys) do
      local val = v[k]
      local in_seq = math.type(k) == "integer" and k >= 1 and k <= n
      local ks = not (in_seq or type(k) == "table") and keyrepr(k) or nil
      local vs = emit(val)
      local kk = type(k) == "table" and emit(k) or nil
      if vs == nil or (type(k) == "table" and kk == nil) then
        patches[#patches + 1] = {owner = v, key = k, val = val}
        if math.type(k) == "integer" and k >= 1 and k <= n then
          -- keep sequence positions: placeholder replaced by the patch
          parts[#parts + 1] = "[" .. k .. "]=false"
        end
      elseif type(k) == "table" then
        parts[#parts + 1] = "[" .. kk .. "]=" .. vs
      elseif ks then
        parts[#parts + 1] = ks .. "=" .. vs
      else
        parts[#parts + 1] = "[" .. k .. "]=" .. vs
      end
    end
    local src = "{" .. table.concat(parts, ",") .. "}"
    emitting[v] = "done"
    if names[v] then
      lines[#lines + 1] = string.format("t.%s = %s", names[v], src)
      return "t." .. names[v]
    end
    return src
  end
  local top = emit(root)
  for _, p in ipairs(patches) do
    local owner = names[p.owner] and ("t." .. names[p.owner]) or error("unnamed cycle owner")
    local key = type(p.key) == "table" and ("t." .. names[p.key]) or literal(p.key)
    local val = type(p.val) == "table" and ("t." .. names[p.val]) or literal(p.val)
    lines[#lines + 1] = string.format("%s[%s] = %s", owner, key, val)
  end
  local body = table.concat(lines, "\n")
  return body .. "\nreturn " .. top
end

-- deep equality that also checks sharing structure (isomorphism)
local function iso(a, b, map_ab, map_ba)
  map_ab, map_ba = map_ab or {}, map_ba or {}
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then
    if a ~= a and b ~= b then return true end
    if a == 0 and b == 0 and math.type(a) == "float" then return 1 / a == 1 / b end
    return a == b and math.type(a) == math.type(b)
  end
  if map_ab[a] or map_ba[b] then return map_ab[a] == b and map_ba[b] == a end
  map_ab[a], map_ba[b] = b, a
  local ka, kb = sorted_keys(a), sorted_keys(b)
  if #ka ~= #kb then return false end
  for i = 1, #ka do
    local x, y = ka[i], kb[i]
    if type(x) == "table" then
      if not iso(x, y, map_ab, map_ba) then return false end
    elseif x ~= y then
      return false
    end
    if not iso(a[x], b[y], map_ab, map_ba) then return false end
  end
  return true
end

local function pretty(v, indent, depth, seen)
  indent, depth, seen = indent or "", depth or 0, seen or {}
  if type(v) ~= "table" then return literal(v) end
  if seen[v] then return "<cycle>" end
  if depth >= 3 then return "{...}" end
  seen[v] = true
  local keys = sorted_keys(v)
  if #keys == 0 then seen[v] = nil return "{}" end
  local out = {"{"}
  for _, k in ipairs(keys) do
    local ks = type(k) == "table" and "[<table>]" or keyrepr(k)
    out[#out + 1] = indent .. "  " .. ks .. " = " .. pretty(v[k], indent .. "  ", depth + 1, seen) .. ","
  end
  out[#out + 1] = indent .. "}"
  seen[v] = nil
  return table.concat(out, "\n")
end

local function roundtrip(label, value)
  local src = serialize(value)
  local fn, err = load(src, "=" .. label, "t", {math = math})
  if not fn then print(label, "LOAD ERROR", err) print(src) return end
  local back = fn()
  print(string.format("== %s: %d bytes, %d lines, iso=%s", label, #src, select(2, src:gsub("\n", "")) + 1,
                      tostring(iso(value, back))))
  return src, back
end

-- simple values
local simple = {
  1, 2.5, "three", true, false, -0.0, 1/0, -1/0, math.mininteger, math.maxinteger, 0.1, 1e300, 2^-60,
  name = "x", ["with space"] = "y", ["end"] = "kw", [20] = "sparse", [-1] = "neg", [1.5] = "float key",
  [true] = "bool key", nested = { deeper = { deepest = { "bottom" } } }, quote = "a\"b\\c\nd\0e\127",
}
local src = roundtrip("simple", simple)
print(src)

-- nan survives (iso treats nan == nan)
local _, nb = roundtrip("nan", {0/0, x = 0/0})
print("nan back:", nb[1] ~= nb[1], nb.x ~= nb.x)

-- shared references
local shared = {"shared leaf"}
local dag = {a = shared, b = shared, list = {shared, shared, {shared}}}
local src2, back2 = roundtrip("dag", dag)
print(src2)
print("sharing preserved:", back2.a == back2.b, back2.list[1] == back2.a, back2.list[3][1] == back2.b)

-- cycles
local node1, node2 = {name = "n1"}, {name = "n2"}
node1.next, node2.next = node2, node1
node1.self = node1
local ring = {head = node1, items = {node1, node2}}
local src3, back3 = roundtrip("cycle", ring)
print(src3)
print("cycle preserved:", back3.head.next.next == back3.head, back3.head.self == back3.head,
      back3.items[2] == back3.head.next)

-- self-containing root and table keys
local root = {}
root.me = root
root[root] = "root as key"
local key = {"k"}
root.holder = {[key] = key} -- at most one table key per table keeps key order deterministic
local src4, back4 = roundtrip("selfroot", root)
print(src4)
print("root ok:", back4.me == back4, back4[back4] == "root as key", next(back4.holder) == back4.holder[next(back4.holder)])

-- pretty printer
print(pretty(simple))
print(pretty(ring))

-- errors
print(pcall(serialize, {f = print}))
print(pcall(serialize, {co = coroutine.create(print)}))
