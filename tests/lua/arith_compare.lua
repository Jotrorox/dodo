-- Arithmetic: exact comparisons between integers and floats, mixed-type errors
local maxi, mini = math.maxinteger, math.mininteger
local function cmp(label, a, b)
  print(string.format("%-28s ==%-5s <%-5s <=%-5s >%-5s >=%-5s ~=%s", label,
    tostring(a == b), tostring(a < b), tostring(a <= b), tostring(a > b), tostring(a >= b), tostring(a ~= b)))
end

cmp("1 vs 1.0", 1, 1.0)
cmp("1 vs 1.5", 1, 1.5)
cmp("-1 vs -0.5", -1, -0.5)
cmp("0 vs -0.0", 0, -0.0)
cmp("maxint vs 2^63", maxi, 2^63)
cmp("mini vs -2^63", mini, -2^63)
cmp("maxint vs maxint+0.0", maxi, maxi + 0.0)
cmp("maxint-1 vs 2^63", maxi - 1, 2^63)
cmp("mini vs -2^63-2^11", mini, -2^63 - 2^11)
cmp("mini+1 vs -2^63", mini + 1, -2^63)
cmp("2^53+1 vs 2^53", (1 << 53) + 1, 2^53)
cmp("2^53 vs 2^53", 1 << 53, 2^53)
cmp("2^53+1 vs 2^53+1.0", (1 << 53) + 1, 2^53 + 1.0)
cmp("2^53+1 vs 2^53+2.0", (1 << 53) + 1, 2^53 + 2.0)
cmp("-(2^53+1) vs -2^53", -((1 << 53) + 1), -2^53)
cmp("maxint vs 9.2233720368547e18", maxi, 9.2233720368547e18)
cmp("maxint vs huge", maxi, math.huge)
cmp("mini vs -huge", mini, -math.huge)
cmp("5 vs nan", 5, 0 / 0)
cmp("2^62 vs 2^62", 1 << 62, 2^62)
cmp("3 vs 2.9999999999999996", 3, 2.9999999999999996)
cmp("1e308 vs maxint", 1e308, maxi)
cmp("-1e308 vs mini", -1e308, mini)

-- float vs float and int vs int
cmp("0.1+0.2 vs 0.3", 0.1 + 0.2, 0.3)
cmp("mini vs maxi", mini, maxi)
cmp("-1 vs 0", -1, 0)

-- strings compare lexicographically, never numerically
cmp("'10' vs '9'", "10", "9")
cmp("'a' vs 'b'", "a", "b")
cmp("'' vs 'a'", "", "a")
print("'10' == 10", "10" == 10, "1 == '1'", 1 == "1", "1.0 == '1.0'", 1.0 == "1.0")

-- equality never raises for mixed types
print("mixed ==", {} == {}, nil == false, true == 1, "x" == nil, print == print)

-- ordering mixed types raises errors
local function try(label, f)
  local ok, err = pcall(f)
  print(label, ok, err)
end
try("1 < '2'", function() return 1 < "2" end)
try("'1' < 2", function() return "1" < 2 end)
try("1 <= nil", function() return 1 <= nil end)
try("{} < {}", function() return {} < {} end)
try("true < false", function() return true < false end)
try("nil > 1", function() return nil > 1 end)
try("{} >= 1", function() return {} >= 1 end)
try("print < print", function() return print < print end)
local a, s = 1, "x"
try("local int < local str", function() return a < s end)
try("str >= int", function() return s >= a end)

-- comparison metamethods
local mt = {__lt = function(x, y) return x.v < y.v end, __le = function(x, y) return x.v <= y.v end,
            __eq = function(x, y) return x.v == y.v end}
local A, B, C = setmetatable({v = 1}, mt), setmetatable({v = 2}, mt), setmetatable({v = 1}, mt)
print("mt", A < B, A <= B, A > B, A >= B, A == C, A ~= C, A == B)
print("mt result converted to boolean", setmetatable({}, {__lt = function() return 1 end}) < {})
try("mt with number", function() return A < 1 end)
