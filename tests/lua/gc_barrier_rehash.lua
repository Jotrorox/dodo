-- A value stored into a table whose insertion forces a rehash must survive
-- an incremental collection in progress: the store needs a write barrier
-- even when the rehash moves the key into the array part. Each trial
-- creates a table, lets the collector run a few steps (so the table may
-- already be marked) and then makes the first store, which rehashes.
collectgarbage("incremental", 100, 40)
local big = {}
for i = 1, 20000 do big[i] = {i} end
local lost = 0
for trial = 1, 400 do
  local t = {}
  for _ = 1, trial % 7 do collectgarbage("step", 0) end
  t[1] = string.rep("v", 50) .. trial
  local h = {}
  for _ = 1, trial % 5 do collectgarbage("step", 0) end
  h.key = {trial}
  collectgarbage("step", 0)
  collectgarbage("step", 0)
  if t[1] ~= string.rep("v", 50) .. trial then lost = lost + 1 end
  if type(h.key) ~= "table" or h.key[1] ~= trial then lost = lost + 1 end
end
print("lost", lost, #big)
