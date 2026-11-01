-- Benchmark: hash tables keyed by strings. Generates pseudo-random text
-- from an LCG over a fixed syllable inventory, tokenizes it with gmatch,
-- counts word frequencies, then sorts the vocabulary by (count desc, word)
-- and reports the top entries. Stresses string interning, table hashing
-- with string keys, and table growth.

local WORDS = 3000000

local syllables = {"ka", "lo", "mi", "ne", "su", "ta", "ri", "po", "an", "el",
                   "or", "us", "ve", "zi", "qu", "th"}
local seed = 12345
local function rand(n)
  seed = (seed * 1103515245 + 12345) % 2147483648
  return (seed >> 16) % n + 1
end

-- build the text in chunks
local chunks = {}
local chunk = {}
for i = 1, WORDS do
  local len = rand(3)
  if rand(8) == 1 then len = len + 1 end
  local w = syllables[rand(16)]
  for _ = 2, len do w = w .. syllables[rand(16)] end
  chunk[#chunk + 1] = w
  if #chunk == 1000 then
    chunks[#chunks + 1] = table.concat(chunk, rand(10) == 1 and ", " or " ")
    chunk = {}
  end
end
if #chunk > 0 then chunks[#chunks + 1] = table.concat(chunk, " ") end
local text = table.concat(chunks, ".\n")

local freq, total = {}, 0
for w in text:gmatch("%a+") do
  freq[w] = (freq[w] or 0) + 1
  total = total + 1
end

local vocab = {}
for w in pairs(freq) do vocab[#vocab + 1] = w end
table.sort(vocab, function(a, b)
  local fa, fb = freq[a], freq[b]
  if fa ~= fb then return fa > fb end
  return a < b
end)

local top = {}
for i = 1, 5 do top[i] = vocab[i] .. "=" .. freq[vocab[i]] end
print("words", total, "distinct", #vocab)
print("top", table.concat(top, " "))
