-- Benchmark: formatting and pattern matching. string.format with integer,
-- string, hex and fixed-point directives, string.find (plain and pattern),
-- string.match captures, gsub with string/table/function replacements, and
-- gmatch tokenization over generated log lines.

local LINES = 250000

local levels = {"INFO", "WARN", "ERROR", "DEBUG"}
local users = {"alice", "bob", "carol", "dave", "eve"}

local seed = 42
local function rand(n)
  seed = (seed * 1103515245 + 12345) % 2147483648
  return (seed >> 16) % n + 1
end

local log = {}
for i = 1, LINES do
  log[i] = string.format("%05d %-5s user=%s latency=%d.%03dms code=0x%04X path=/api/v%d/items/%d",
    i, levels[rand(4)], users[rand(5)], rand(900), rand(999), rand(65535), rand(3), rand(10000))
end

local counts, total_latency, errors, plain_hits = {}, 0, 0, 0
local words = 0
local rewritten_len = 0
local repl = {alice = "A", bob = "B", carol = "C", dave = "D", eve = "E"}
for i = 1, LINES do
  local line = log[i]
  local level, user, ms, frac = line:match("^%d+ (%u+)%s+user=(%a+) latency=(%d+)%.(%d+)ms")
  counts[level] = (counts[level] or 0) + 1
  total_latency = total_latency + tonumber(ms) * 1000 + tonumber(frac)
  if line:find("ERROR", 1, true) then errors = errors + 1 end
  if line:find("/v2/", 1, true) then plain_hits = plain_hits + 1 end
  local code = tonumber(line:match("code=0x(%x+)"), 16)
  local s = line:gsub("user=(%a+)", repl)
  s = s:gsub("%d+", function(d) return #d end)
  s = s:gsub("/", "\\")
  rewritten_len = rewritten_len + #s + code % 7
  for _ in line:gmatch("%w+") do words = words + 1 end
end

local summary = {}
for _, lv in ipairs(levels) do summary[#summary + 1] = string.format("%s=%d", lv, counts[lv] or 0) end
print(table.concat(summary, " "))
print(string.format("latency_us=%d errors=%d v2=%d words=%d rewritten=%d",
  total_latency, errors, plain_hits, words, rewritten_len))
