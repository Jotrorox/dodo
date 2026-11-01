-- Strings: concatenation, length operator, comparison and embedded zeros
local function esc(s) return (s:gsub("%z", "\\0")) end

-- concatenation
print(1 .. 2, 1 .. 2 .. 3, "a" .. 1 .. 2.5, 1.0 .. "", -1 .. -2)
print("" .. "", ("x"):rep(3) .. "y", "a" .. "b" == "ab")
local parts = {}
for i = 1, 5 do parts[#parts + 1] = i .. "" end
print(table.concat(parts, "-"))
-- right associativity with a __concat metamethod
local mt = {__concat = function(a, b)
  local function n(x) return type(x) == "table" and "T" or tostring(x) end
  return "(" .. n(a) .. "+" .. n(b) .. ")"
end}
local T = setmetatable({}, mt)
print("a" .. "b" .. T, T .. "a" .. "b", 1 .. T, T .. 2.0, T .. T)
print("x" .. T .. "y")

local function try(label, f)
  local ok, v = pcall(f)
  print(label, ok, v)
end
local tbl, nothing, bool = {}, nil, true
try("str .. table", function() return "a" .. tbl end)
try("table .. str", function() return tbl .. "a" end)
try("str .. nil local", function() return "a" .. nothing end)
try("str .. nil global", function() return "a" .. undefined_name end)
try("str .. bool", function() return "a" .. bool end)
try("nil .. nil", function() return nil .. nil end)
try("str .. field", function() return "a" .. tbl.field end)
try("str .. function", function() return "a" .. print end)
try("long chain error", function() return "a" .. "b" .. "c" .. tbl .. "d" end)

-- length operator
print(#"", #"a", #"hello", #"a\0b", #"\0\0\0", #"\u{10FFFF}", #("x"):rep(1000))
local s = "abc"
print(#s, #s .. "", -#s, #s + 1, #s == 3)

-- comparisons are bytewise in the C locale
local pairs_ = {
  {"a", "b"}, {"a", "a"}, {"a", "ab"}, {"ab", "b"}, {"Z", "a"}, {"", "a"}, {"", ""},
  {"abc", "abd"}, {"a\0b", "a\0c"}, {"a", "a\0"}, {"a\0", "a\0\0"}, {"\0", ""},
  {"\255", "a"}, {"\128", "\127"}, {"10", "9"}, {"-1", "-2"}, {"a\0z", "a\1"},
  {" ", "\t"}, {"A", "a"},
}
for _, p in ipairs(pairs_) do
  local a, b = p[1], p[2]
  local function q(x) return (string.format("%q", x):gsub("[\128-\255]", function(c) return "\\" .. c:byte() end)) end
  local qa, qb = q(a), q(b)
  print(string.format("%-8s %-8s < %-5s <= %-5s == %-5s > %s", qa, qb,
    tostring(a < b), tostring(a <= b), tostring(a == b), tostring(a > b)))
end

-- strings with embedded zeros keep all bytes
local z = "one\0two\0three"
print(#z, esc(z), esc(z:upper()), z:find("\0", 1, true), z:find("two"))
print(esc(z:sub(4, 8)), z:byte(4), esc(("\0"):rep(3, "-")))
print("a\0b" == "a\0b", "a\0b" == "a\0c", "a\0b" ~= "a")
-- string keys with embedded zeros are distinct
local k = {["a\0b"] = 1, ["a\0c"] = 2, ["a"] = 3}
print(k["a\0b"], k["a\0c"], k["a"], k["a\0"])
