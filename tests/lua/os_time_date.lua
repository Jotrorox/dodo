-- os.time and os.date with fixed values (TZ=UTC in the test environment,
-- so local time equals UTC): os.time with tables, defaults for hour (12),
-- normalisation of out-of-range fields, os.date formats ("!%Y-%m-%d...",
-- "*t", "!*t" with sorted fields), locale-independent conversions in the
-- C locale, and errors for invalid conversions and missing fields.

local function fields(t)
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = k end
  table.sort(keys)
  local out = {}
  for _, k in ipairs(keys) do out[#out + 1] = k .. "=" .. tostring(t[k]) end
  return table.concat(out, " ")
end

print(os.time({year = 1970, month = 1, day = 1, hour = 0}))
print(os.time({year = 2000, month = 1, day = 1, hour = 0, min = 0, sec = 0}))
print(os.time({year = 2000, month = 1, day = 1}))           -- hour defaults to 12
print(os.time({year = 2024, month = 2, day = 29, hour = 23, min = 59, sec = 59}))
print(math.type(os.time({year = 2020, month = 6, day = 15})))
print(math.type(os.time()))

-- normalisation of out-of-range fields
local function norm(t)
  local ts = os.time(t)
  return ts, os.date("!%Y-%m-%d %H:%M:%S", ts)
end
print(norm({year = 2021, month = 13, day = 1, hour = 0}))
print(norm({year = 2021, month = 3, day = 0, hour = 0}))
print(norm({year = 2021, month = 1, day = -1, hour = 0}))
print(norm({year = 2021, month = 1, day = 1, hour = 25, min = 61, sec = 61}))
print(norm({year = 2021, month = 0, day = 1, hour = 0}))
print(norm({year = 2021, month = -11, day = 1, hour = 0}))
print(norm({year = 2020, month = 2, day = 30, hour = 0}))
print(norm({year = 2021, month = 1, day = 1, hour = 0, sec = -1}))
-- fields given as strings convertible to integers / floats with integral value
print(norm({year = "2010", month = "5", day = "6", hour = 7.0}))
-- the table is updated with normalised fields
local t = {year = 2021, month = 14, day = 35, hour = 12, isdst = false}
os.time(t)
print(fields(t))

-- errors
print(pcall(os.time, {year = 2020}))
print(pcall(os.time, {year = 2020, month = 1}))
print(pcall(os.time, {month = 1, day = 1}))
print(pcall(os.time, {year = 2020, month = 1.5, day = 1}))
print(pcall(os.time, {year = 2020, month = "x", day = 1}))
print(pcall(os.time, 1))

-- os.date formats with fixed timestamps
local ts = 1700000000   -- 2023-11-14 22:13:20 UTC, a Tuesday
print(os.date("!%Y-%m-%d %H:%M:%S", ts))
print(os.date("%Y-%m-%d %H:%M:%S", ts))
print(os.date("!%a %A %b %B %d %H %I %j %m %M %p %S %w %y %Y %%", ts))
print(os.date("!%a %A %b %B %d %H %I %j %m %M %p %S %w %y %Y", 0))
print(os.date("!%I %p", 1700000000 - 22 * 3600))   -- midnight
print(os.date("!%I %p", 1700000000 - 10 * 3600))   -- noon
print(os.date("!%c", ts))
print(os.date("!%x %X %D %T %R %e %n%t|", ts))
print(os.date("!%F %G %g %V %u %h %C", ts))
print(os.date("!no conversions here", ts))
print(os.date("!", ts) == "")
print(fields(os.date("!*t", ts)))
print(fields(os.date("*t", 0)))
print(fields(os.date("!*t", 951782400)))   -- 2000-02-29
-- round trip: os.time(os.date("*t", x)) == x
for _, x in ipairs({0, 86399, 951782400, 1700000000, 2147483647, 4102444800}) do
  io.write(tostring(os.time(os.date("*t", x)) == x), " ")
end
print()
-- negative timestamps (before 1970)
print(os.date("!%Y-%m-%d %H:%M:%S", -1))
print(os.date("!%Y-%m-%d", -86400 * 365))

-- invalid conversions
print(pcall(os.date, "%Ez", ts))
print(pcall(os.date, "%Q", ts))
print(pcall(os.date, "%", ts))
print(pcall(os.date, "abc %", ts))
print(pcall(os.date, "%5d", ts))
-- valid E and O modifiers
print(os.date("!%Ey %OH", ts))
-- bad time argument
print(pcall(os.date, "%Y", 1.5))
print(pcall(os.date, "%Y", "x"))
print(pcall(os.date, {}, 0))
print(type(os.date()), type(os.date("*t").year))

-- difftime
print(os.difftime(10, 5), os.difftime(5, 10), os.difftime(ts, 0), math.type(os.difftime(1, 1)))
print(pcall(os.difftime))
print(pcall(os.difftime, 1))
