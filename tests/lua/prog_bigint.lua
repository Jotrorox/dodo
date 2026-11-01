-- Program test: arbitrary-precision signed integers stored as little-endian
-- arrays of base-10^7 limbs. Implements parsing, tostring, comparison,
-- add/sub/mul, divmod by small integers and by bigints (long division),
-- power, and gcd, wired up through metamethods (__add, __sub, __mul,
-- __idiv, __mod, __pow, __unm, __eq, __lt, __le, __tostring, __concat).
-- Prints factorial(50), 2^200, fibonacci(300), and checks identities, plus
-- cross-checks small results against native 64-bit integer arithmetic.

local BASE, WIDTH = 10000000, 7

local Big = {}
Big.__index = Big

local function normalize(limbs)
  while #limbs > 1 and limbs[#limbs] == 0 do limbs[#limbs] = nil end
  return limbs
end

local function make(limbs, neg)
  normalize(limbs)
  if #limbs == 1 and limbs[1] == 0 then neg = false end
  return setmetatable({limbs = limbs, neg = neg}, Big)
end

function Big.from(v)
  if getmetatable(v) == Big then return v end
  if math.type(v) == "integer" then
    local neg = v < 0
    local limbs = {}
    if v == math.mininteger then return Big.parse(tostring(v)) end
    v = math.abs(v)
    repeat
      limbs[#limbs + 1] = v % BASE
      v = v // BASE
    until v == 0
    return make(limbs, neg)
  elseif type(v) == "string" then
    return Big.parse(v)
  end
  error("cannot convert " .. type(v) .. " to bigint", 2)
end

function Big.parse(s)
  local sign, digits = s:match("^%s*([-+]?)(%d+)%s*$")
  if not digits then error("invalid bigint literal '" .. s .. "'", 2) end
  local limbs = {}
  for i = #digits, 1, -WIDTH do
    local j = math.max(1, i - WIDTH + 1)
    limbs[#limbs + 1] = tonumber(digits:sub(j, i))
  end
  return make(limbs, sign == "-")
end

function Big:__tostring()
  local parts = { tostring(self.limbs[#self.limbs]) }
  for i = #self.limbs - 1, 1, -1 do
    parts[#parts + 1] = string.format("%07d", self.limbs[i])
  end
  return (self.neg and "-" or "") .. table.concat(parts)
end

local function cmp_abs(a, b)
  if #a ~= #b then return #a < #b and -1 or 1 end
  for i = #a, 1, -1 do
    if a[i] ~= b[i] then return a[i] < b[i] and -1 or 1 end
  end
  return 0
end

local function add_abs(a, b)
  local r, carry = {}, 0
  for i = 1, math.max(#a, #b) do
    local s = (a[i] or 0) + (b[i] or 0) + carry
    r[i] = s % BASE
    carry = s // BASE
  end
  if carry > 0 then r[#r + 1] = carry end
  return r
end

local function sub_abs(a, b) -- requires |a| >= |b|
  local r, borrow = {}, 0
  for i = 1, #a do
    local s = a[i] - (b[i] or 0) - borrow
    if s < 0 then s = s + BASE; borrow = 1 else borrow = 0 end
    r[i] = s
  end
  return r
end

local function mul_abs(a, b)
  local r = {}
  for i = 1, #a + #b do r[i] = 0 end
  for i = 1, #a do
    local carry, ai = 0, a[i]
    for j = 1, #b do
      local cur = r[i + j - 1] + ai * b[j] + carry
      r[i + j - 1] = cur % BASE
      carry = cur // BASE
    end
    local k = i + #b
    while carry > 0 do
      local cur = r[k] + carry
      r[k] = cur % BASE
      carry = cur // BASE
      k = k + 1
    end
  end
  return r
end

-- divide |a| by a small positive integer d < BASE
local function divmod_small(a, d)
  local q, rem = {}, 0
  for i = #a, 1, -1 do
    local cur = rem * BASE + a[i]
    q[i] = cur // d
    rem = cur % d
  end
  return q, rem
end

-- schoolbook long division of magnitudes, one limb at a time with binary search
local function divmod_abs(a, b)
  if cmp_abs(a, b) < 0 then return {0}, {table.unpack(a)} end
  if #b == 1 then
    local q, r = divmod_small(a, b[1])
    return q, {r}
  end
  local q, rem = {}, {0}
  for i = #a, 1, -1 do
    table.insert(rem, 1, a[i])
    normalize(rem)
    local lo, hi = 0, BASE - 1
    while lo < hi do
      local mid = (lo + hi + 1) // 2
      if cmp_abs(normalize(mul_abs(b, {mid})), rem) <= 0 then lo = mid else hi = mid - 1 end
    end
    q[i] = lo
    if lo > 0 then rem = normalize(sub_abs(rem, normalize(mul_abs(b, {lo})))) end
  end
  return q, rem
end

local function binop(f)
  return function(a, b) return f(Big.from(a), Big.from(b)) end
end

Big.__add = binop(function(a, b)
  if a.neg == b.neg then return make(add_abs(a.limbs, b.limbs), a.neg) end
  local c = cmp_abs(a.limbs, b.limbs)
  if c == 0 then return Big.from(0) end
  if c > 0 then return make(sub_abs(a.limbs, b.limbs), a.neg) end
  return make(sub_abs(b.limbs, a.limbs), b.neg)
end)
Big.__unm = function(a) return make({table.unpack(a.limbs)}, not a.neg) end
Big.__sub = binop(function(a, b) return a + (-b) end)
Big.__mul = binop(function(a, b) return make(mul_abs(a.limbs, b.limbs), a.neg ~= b.neg) end)

-- floor division semantics like Lua integers
local function divmod(a, b)
  if #b.limbs == 1 and b.limbs[1] == 0 then error("bigint division by zero", 3) end
  local q, r = divmod_abs(a.limbs, b.limbs)
  local qb, rb = make(q, a.neg ~= b.neg), make(r, a.neg)
  if not (#rb.limbs == 1 and rb.limbs[1] == 0) and a.neg ~= b.neg then
    qb = qb - 1
    rb = rb + b
  end
  return qb, rb
end
Big.__idiv = binop(function(a, b) return (divmod(a, b)) end)
Big.__mod = binop(function(a, b) local _, r = divmod(a, b); return r end)
Big.__pow = function(a, n)
  a = Big.from(a)
  assert(math.type(n) == "integer" and n >= 0, "exponent must be a non-negative integer")
  local result = Big.from(1)
  while n > 0 do
    if n & 1 == 1 then result = result * a end
    a = a * a
    n = n >> 1
  end
  return result
end
Big.__eq = function(a, b) return a.neg == b.neg and cmp_abs(a.limbs, b.limbs) == 0 end
Big.__lt = binop(function(a, b)
  if a.neg ~= b.neg then return a.neg end
  local c = cmp_abs(a.limbs, b.limbs)
  if a.neg then return c > 0 else return c < 0 end
end)
Big.__le = binop(function(a, b) return not (b < a) end)
Big.__concat = function(a, b) return tostring(a) .. tostring(b) end

local function gcd(a, b)
  a, b = Big.from(a), Big.from(b)
  if a.neg then a = -a end
  if b.neg then b = -b end
  while not (b == Big.from(0)) do a, b = b, a % b end
  return a
end

-- tests ------------------------------------------------------------------

local function factorial(n)
  local r = Big.from(1)
  for i = 2, n do r = r * i end
  return r
end

local f50 = factorial(50)
print("50! = " .. f50)
print("digits:", #tostring(f50), "digit sum:", (function()
  local s = 0
  for d in tostring(f50):gmatch("%d") do s = s + tonumber(d) end
  return s
end)())
print("2^200 = " .. Big.from(2) ^ 200)
local a, b = Big.from(0), Big.from(1)
for _ = 1, 300 do a, b = b, a + b end
print("fib(300) = " .. a)
print("100!/98! = " .. factorial(100) // factorial(98))
print("50! % 1000003 = " .. f50 % 1000003)
print("50! // 49! == 50:", f50 // factorial(49) == Big.from(50))
local big1 = Big.parse("123456789012345678901234567890")
local big2 = Big.parse("-987654321098765432109876543210")
print("sum  = " .. big1 + big2)
print("diff = " .. big1 - big2)
print("prod = " .. big1 * big2)
print("quot = " .. big2 // big1, "rem = " .. big2 % big1)
local q, r = big2 // big1, big2 % big1
print("q*b+r == a:", q * big1 + r == big2)
print("compare:", big2 < big1, big1 < big2, big1 <= big1, -big1 == Big.parse("-123456789012345678901234567890"))
print("gcd(2^64*3^20, 6^30) = " .. gcd(Big.from(2) ^ 64 * Big.from(3) ^ 20, Big.from(6) ^ 30))
print("(2^127-1) prime-ish check mod 7 = " .. (Big.from(2) ^ 127 - 1) % 7)
print("mininteger:", Big.from(math.mininteger), Big.from(math.mininteger) - 1)
print("parse zero forms:", Big.parse("-0"), Big.parse("000"), Big.parse("+0000123"))

-- cross-check against native integers
local seed = 7
local function rand()
  seed = (seed * 48271) % 2147483647
  return seed
end
local bad = 0
for _ = 1, 300 do
  local x = rand() - 1073741823
  local y = rand() % 100000 - 50000
  if y == 0 then y = 1 end
  local bx, by = Big.from(x), Big.from(y)
  if tostring(bx + by) ~= tostring(x + y) then bad = bad + 1 end
  if tostring(bx - by) ~= tostring(x - y) then bad = bad + 1 end
  if tostring(bx * by) ~= tostring(x * y) then bad = bad + 1 end
  if tostring(bx // by) ~= tostring(x // y) then bad = bad + 1 end
  if tostring(bx % by) ~= tostring(x % y) then bad = bad + 1 end
  if (bx < by) ~= (x < y) then bad = bad + 1 end
end
print("native cross-check mismatches:", bad)

print(pcall(function() return Big.from(1) // 0 end))
print(pcall(Big.parse, "12a"))
print(pcall(Big.from, {}))
