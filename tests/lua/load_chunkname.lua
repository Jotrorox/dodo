-- Chunk names and how they appear in messages: "=name" is used verbatim,
-- "@file" as a file name, anything else as [string "..."] with the first
-- line only and truncation with "..." for long sources. Also the default
-- chunk name of load (the source itself, or "=(load)" for readers) and
-- the short_src field of debug.getinfo.

local function msg(src, name)
  local f, err = load(src, name)
  if f then
    local ok, e = pcall(f)
    return e
  end
  return err
end

-- = and @ prefixes
print(msg("error('x')", "=custom name"))
print(msg("error('x')", "@some/file.lua"))
print(msg("error('x')", "plain name"))
print(msg("\n\nerror('line 3')", "=three"))
print(msg("x = = 1", "=syn"))
print(msg("x = = 1", "@syn.lua"))

-- default chunkname is the source string itself
print(msg("error('default')"))
print(msg("local a = 1\nerror('second line')"))
print(msg("x ="))

-- readers default to "=(load)"
local done = false
print(msg(function() if done then return nil end done = true return "error('rd')" end))

-- truncation of long string sources: try several lengths
for _, len in ipairs({30, 35, 36, 37, 38, 39, 40, 45, 50, 60, 80}) do
  local body = "error('e')--"
  local src = body .. string.rep("a", len - #body)
  print(len, msg(src))
end
-- long chunk names given explicitly
for _, len in ipairs({40, 55, 59, 60, 61, 70}) do
  print(len, msg("error('e')", string.rep("n", len)))
  print(len, msg("error('e')", "=" .. string.rep("q", len)))
  print(len, msg("error('e')", "@" .. string.rep("f", len)))
end

-- multi-line sources show only the first line followed by "..."
print(msg("error('ml')\nlocal x = 1"))
print(msg("\nerror('starts with newline')"))
print(msg("local t = {}\n\n\nreturn t.x.y"))
-- a lone CR is kept in the short source (shown here as <CR>)
print((msg("error('cr')\rmore"):gsub("\r", "<CR>")))

-- getinfo short_src/source for the various forms
for _, name in ipairs({"=eq", "@at.lua", "plain", "multi\nline"}) do
  local f = load("return debug.getinfo(1, 'S')", name)
  local i = f()
  print(string.format("%q -> source=%q short_src=%q what=%s", name, i.source, i.short_src, i.what))
end

-- empty chunk name
print(msg("error('e')", ""))
print(msg("error('e')", "="))
print(msg("error('e')", "@"))

-- the chunkname is also used by syntax errors at EOF
print(msg("function f(", "=eof"))
print(msg("return 'unfinished", "=str"))
print(msg("return [[unfinished", "=long"))
