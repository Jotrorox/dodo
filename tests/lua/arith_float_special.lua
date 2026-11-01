-- Arithmetic: infinities, NaN, negative zero and float table keys
local inf, nan = math.huge, 0 / 0
local function isnan(x) return x ~= x end
local function f(x) if isnan(x) then return "nan" end return tostring(x) end

print("inf", inf, -inf, math.type(inf))
print("inf+1", f(inf + 1), "inf-inf", f(inf - inf), "inf*0", f(inf * 0))
print("inf*-1", f(inf * -1), "inf/inf", f(inf / inf), "1/inf", f(1 / inf), "-1/inf", f(-1 / inf))
print("inf^0", f(inf ^ 0), "nan^0", f(nan ^ 0), "1^nan", f(1 ^ nan), "0^0", f(0 ^ 0))
print("0^-1", f(0 ^ -1), "(-0.0)^-1", f((-0.0) ^ -1), "inf^-1", f(inf ^ -1))
print("nan+1", f(nan + 1), "nan*0", f(nan * 0), "-nan is nan", isnan(-nan))

-- comparisons with NaN are always false (except ~=)
print("nan==nan", nan == nan, "nan~=nan", nan ~= nan)
print("nan<1", nan < 1, "nan>1", nan > 1, "nan<=nan", nan <= nan, "nan>=1", nan >= 1)
print("1<nan", 1 < nan, "int nan", 5 < nan, 5 > nan, 5 == nan)
print("not (nan<1)", not (nan < 1), "max", f(math.max(nan, 1)), f(math.max(1, nan)))
print("inf comparisons", inf > math.maxinteger, -inf < math.mininteger, inf == inf, -inf < inf)

-- negative zero
local nz = -0.0
print("-0.0", nz, "== 0", nz == 0, "== 0.0", nz == 0.0, "< 0", nz < 0)
print("1/-0.0", 1 / nz, "1/0.0", 1 / 0.0, "-(0.0)", -(0.0), "0.0-0.0", 0.0 - 0.0)
print("-0.0+0.0", nz + 0.0, "-0.0*1", nz * 1, "-0.0-0.0", nz - 0.0)
print("-0 int", -0, math.type(-0), "0*-1", 0 * -1)
print("tostring(-0.0)", tostring(-0.0), string.format("%g", -0.0), string.format("%.1f", -0.0))
print("math.tointeger(-0.0)", math.tointeger(-0.0), "floor", math.floor(-0.0), "ceil(-0.5)", math.ceil(-0.5))

-- table keys: floats with integral values are normalised to integers
local t = {}
t[1.0] = "a"; t[2] = "b"; t[2^53] = "c"
print("t[1]", t[1], "t[2.0]", t[2.0], "t[2^53 int]", t[1 << 53])
for k, v in pairs({[3.0] = true}) do print("key", k, math.type(k)) end
t[-0.0] = "zero"
print("t[0]", t[0], "t[0.0]", t[0.0])
t[1.5] = "frac"
print("t[1.5]", t[1.5], "#keys sorted:")
local keys = {}
for k in pairs(t) do keys[#keys + 1] = k end
table.sort(keys)
for _, k in ipairs(keys) do print("", math.type(k), k, t[k]) end
print("t[nan] read", t[nan])
print(pcall(function() t[nan] = 1 end))
print(pcall(rawset, t, nan, 1))
print("t[inf]", pcall(function() t[inf] = "inf"; return t[inf] end))

-- floor/ceil on huge values stay floats
print("floor(inf)", math.floor(inf), math.type(math.floor(inf)))
print("floor(2^70)", math.floor(2^70), math.type(math.floor(2^70)))
print("floor(-2^63)", math.floor(-2^63), math.type(math.floor(-2^63)))
print("floor(2^63)", math.floor(2^63), math.type(math.floor(2^63)))
print("ceil(2^62+0.0)", math.ceil(2^62), math.type(math.ceil(2^62)))
print("floor(nan) is nan", isnan(math.floor(nan)))
print("modf", math.modf(3.7))
print("modf", math.modf(-3.7))
print("modf inf", math.modf(inf))
print("modf -inf", math.modf(-inf))
print("modf int", math.modf(5))
print("huge equality", 1e308 * 10 == inf, -1e308 * 10 == -inf, 2^1024 == inf)
