-- Program test: a cooperative coroutine scheduler with a virtual clock.
-- Tasks sleep, send/receive on channels (blocking rendezvous with buffers),
-- wait on other tasks (join), spawn children, and fail with errors that the
-- scheduler captures. Also covers generator pipelines via coroutine.wrap,
-- coroutine.status/isyieldable/running, yield across pcall, values passed
-- through resume/yield in both directions, and coroutine.close.

local Scheduler = {}
Scheduler.__index = Scheduler

function Scheduler.new()
  return setmetatable({ now = 0, ready = {}, sleeping = {}, tasks = {}, log = {}, next_id = 1, seq = 0 }, Scheduler)
end

function Scheduler:trace(fmt, ...)
  self.log[#self.log + 1] = string.format("[t=%3d] " .. fmt, self.now, ...)
end

function Scheduler:spawn(name, fn, ...)
  local args = table.pack(...)
  local task = { id = self.next_id, name = name, waiters = {} }
  self.next_id = self.next_id + 1
  task.co = coroutine.create(function() return fn(table.unpack(args, 1, args.n)) end)
  self.tasks[#self.tasks + 1] = task
  self.ready[#self.ready + 1] = { task = task, values = {} }
  return task
end

-- commands yielded by tasks
local function sleep(dt) return coroutine.yield("sleep", dt) end
local function join(task) return coroutine.yield("join", task) end
local function spawn(name, fn, ...) return coroutine.yield("spawn", name, fn, ...) end

local Channel = {}
Channel.__index = Channel
function Channel.new(name, cap) return setmetatable({ name = name, cap = cap or 0, buf = {}, recvq = {}, sendq = {} }, Channel) end
function Channel:send(v) return coroutine.yield("send", self, v) end
function Channel:recv() return coroutine.yield("recv", self) end

function Scheduler:wake(task, ...)
  self.ready[#self.ready + 1] = { task = task, values = table.pack(...) }
end

function Scheduler:handle(task, cmd, a, b, ...)
  if cmd == "sleep" then
    self.seq = self.seq + 1
    self.sleeping[#self.sleeping + 1] = { task = task, at = self.now + a, seq = self.seq }
  elseif cmd == "join" then
    if a.done then self:wake(task, a.ok, a.result) else a.waiters[#a.waiters + 1] = task end
  elseif cmd == "spawn" then
    local child = self:spawn(a, b, ...)
    self:wake(task, child)
  elseif cmd == "send" then
    local ch, v = a, b
    if #ch.recvq > 0 then
      local receiver = table.remove(ch.recvq, 1)
      self:wake(receiver, v)
      self:wake(task, true)
    elseif #ch.buf < ch.cap then
      ch.buf[#ch.buf + 1] = v
      self:wake(task, true)
    else
      ch.sendq[#ch.sendq + 1] = { task = task, v = v }
    end
  elseif cmd == "recv" then
    local ch = a
    if #ch.buf > 0 then
      local v = table.remove(ch.buf, 1)
      if #ch.sendq > 0 then
        local s = table.remove(ch.sendq, 1)
        ch.buf[#ch.buf + 1] = s.v
        self:wake(s.task, true)
      end
      self:wake(task, v)
    elseif #ch.sendq > 0 then
      local s = table.remove(ch.sendq, 1)
      self:wake(s.task, true)
      self:wake(task, s.v)
    else
      ch.recvq[#ch.recvq + 1] = task
    end
  else
    error("unknown command " .. tostring(cmd))
  end
end

function Scheduler:run()
  while true do
    while #self.ready > 0 do
      local entry = table.remove(self.ready, 1)
      local task = entry.task
      local res = table.pack(coroutine.resume(task.co, table.unpack(entry.values, 1, entry.values.n or #entry.values)))
      if coroutine.status(task.co) == "dead" then
        task.done, task.ok = true, res[1]
        task.result = res[2]
        if res[1] then
          self:trace("%s finished -> %s", task.name, tostring(res[2]))
        else
          self:trace("%s FAILED: %s", task.name, tostring(res[2]):gsub("^.-:%d+: ", ""))
        end
        for _, w in ipairs(task.waiters) do self:wake(w, task.ok, task.result) end
      else
        self:handle(task, table.unpack(res, 2, res.n))
      end
    end
    if #self.sleeping == 0 then break end
    table.sort(self.sleeping, function(x, y)
      if x.at ~= y.at then return x.at < y.at end
      return x.seq < y.seq
    end)
    local s = table.remove(self.sleeping, 1)
    self.now = s.at
    self:wake(s.task, self.now)
  end
  local blocked = {}
  for _, t in ipairs(self.tasks) do if not t.done then blocked[#blocked + 1] = t.name end end
  return blocked
end

-- scenario -------------------------------------------------------------------

local sched = Scheduler.new()
local jobs = Channel.new("jobs", 2)
local results = Channel.new("results", 0)

local function producer(n)
  for i = 1, n do
    sleep(3)
    jobs:send(i * 10)
    sched:trace("producer sent %d", i * 10)
  end
  jobs:send(false) -- one stop marker per worker
  jobs:send(false)
  return "produced " .. n
end

local function worker(name, delay)
  local count = 0
  while true do
    local job = jobs:recv()
    if job == false then break end
    sched:trace("%s got job %d", name, job)
    sleep(delay)
    results:send(name .. ":" .. job * 2)
    count = count + 1
  end
  return count
end

local function collector(expected)
  local got = {}
  for _ = 1, expected do got[#got + 1] = results:recv() end
  table.sort(got)
  sched:trace("collector has %s", table.concat(got, ","))
  return #got
end

local function supervisor()
  local child = spawn("faulty", function()
    sleep(5)
    local t = nil
    return t.field -- runtime error inside a task
  end)
  local ok, err = join(child)
  sched:trace("supervisor saw faulty ok=%s", tostring(ok))
  local waiter = spawn("waiter", function(x) local t0 = sleep(x); return "woke at " .. t0 end, 7)
  local ok2, msg = join(waiter)
  return "supervised " .. tostring(ok2) .. " " .. msg
end

local prod = sched:spawn("producer", producer, 6)
sched:spawn("worker-A", worker, "A", 4)
sched:spawn("worker-B", worker, "B", 9)
sched:spawn("collector", collector, 6)
sched:spawn("supervisor", supervisor)
sched:spawn("joiner", function() local ok, r = join(prod); return "producer said: " .. r end)
local deadlocked = Channel.new("never")
sched:spawn("stuck", function() return deadlocked:recv() end)

local blocked = sched:run()
for _, line in ipairs(sched.log) do print(line) end
print("final time:", sched.now, "blocked:", table.concat(blocked, ","))

-- generators and pipelines ----------------------------------------------------

local function range(n)
  return coroutine.wrap(function() for i = 1, n do coroutine.yield(i) end end)
end
local function filter(pred, gen)
  return coroutine.wrap(function()
    for v in gen do if pred(v) then coroutine.yield(v) end end
  end)
end
local function map(f, gen)
  return coroutine.wrap(function() for v in gen do coroutine.yield(f(v)) end end)
end
local function take(n, gen)
  local out = {}
  for v in gen do
    out[#out + 1] = v
    if #out == n then break end
  end
  return out
end
local primes = filter(function(n)
  if n < 2 then return false end
  for d = 2, math.floor(math.sqrt(n)) do if n % d == 0 then return false end end
  return true
end, range(1000))
print("primes squared:", table.concat(take(10, map(function(x) return x * x end, primes)), " "))

-- two-way communication
local acc = coroutine.create(function(first)
  local total = first
  while true do
    local v = coroutine.yield(total)
    if v == nil then return "closed with " .. total end
    total = total + v
  end
end)
local outs = {}
for _, v in ipairs({5, 10, -3, 8}) do
  local _, t = coroutine.resume(acc, v)
  outs[#outs + 1] = t
end
print("running totals:", table.concat(outs, " "), select(2, coroutine.resume(acc)), coroutine.status(acc))
print("resume dead:", coroutine.resume(acc))

-- yield across pcall, status, isyieldable, running
local co
co = coroutine.create(function()
  local main_co, is_main = coroutine.running()
  local ok, err = pcall(function()
    coroutine.yield("inside pcall", coroutine.isyieldable(), coroutine.status(co), is_main)
    error("boom")
  end)
  coroutine.yield("after pcall", ok, (err:gsub("^.-:%d+: ", "")))
  return "done"
end)
print(coroutine.resume(co))
print(coroutine.resume(co))
print(coroutine.resume(co))
print("status:", coroutine.status(co), "main yieldable:", coroutine.isyieldable(), select(2, coroutine.running()))

-- wrap propagates errors; close stops a suspended coroutine
local w = coroutine.wrap(function() coroutine.yield(1); error({code = 42}) end)
print("wrap first:", w())
local ok, e = pcall(w)
print("wrap error:", ok, type(e), e.code)
local closable = coroutine.create(function() coroutine.yield("paused") end)
coroutine.resume(closable)
print("close:", coroutine.close(closable), coroutine.status(closable))
