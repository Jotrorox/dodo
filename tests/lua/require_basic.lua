-- require with Lua modules found through package.path: caching in
-- package.loaded, the second result (loader data = file path), the name
-- and path passed to the chunk as ..., modules returning nil/false
-- (stored as true / false), modules that fill package.loaded themselves,
-- dotted names mapping to directories, ?/init.lua, nested requires and
-- cycles.

package.path = "./modules/?.lua;./modules/?/init.lua"
package.cpath = ""

local m, data = require("simplemod")
print(type(m), m.name, m.data, m.nargs, data)
print(m.greet("world"))
print(package.loaded.simplemod == m)

-- second require: cached, the chunk is not run again; only one result
local m2, data2 = require("simplemod")
print(m2 == m, data2, select("#", require("simplemod")))

-- side effects run once
local c1 = require("counter")
local c2 = require("counter")
print(c1 == c2, c1.loads, counter_loads)
-- removing it from package.loaded forces a reload
package.loaded.counter = nil
local c3 = require("counter")
print(c3 == c1, c3.loads)

-- module returning nothing: require returns true and stores true
print(require("returns_nil"))
print(package.loaded.returns_nil, returns_nil_ran)
-- module returning false: stored as false; since false is not a true
-- value, the next require runs the chunk again (counter shows it)
print(require("returns_false"))
print(package.loaded.returns_false)
package.loaded.counter = false
print(require("counter").loads)
-- module that registered itself: its value is kept
print(require("sets_loaded").self_registered)
-- module returning two values: only the first is used
print(require("multi"))

-- dotted names and init.lua
local sub, subpath = require("pkg.sub")
print(sub.kind, sub.name, sub.file, subpath)
local pk, pkpath = require("pkg")
print(pk.kind, pk.name, pkpath)
local idir, ipath = require("initdir")
print(idir.kind, idir.name, idir.file, ipath)

-- nested require
print(require("requires_other").greeting)

-- cycle: the second require sees the partial value in package.loaded
local a = require("cycle_a")
print(a.partial, a.b.saw_partial)

-- preload takes priority over the path
package.preload["simplemod2"] = function(...)
  print("preload loader args:", ...)
  return {preloaded = true}
end
local p, pdata = require("simplemod2")
print(p.preloaded, pdata)

-- package.loaded holds the standard libraries
print(package.loaded.string == string, package.loaded._G == _G,
      package.loaded.package == package, package.loaded.coroutine == coroutine)

-- require of an already loaded standard library returns it
print(require("string") == string, require("math") == math)

-- package.config: directory separator first, then ';', '?', '!', '-'
local cfg = {}
for line in package.config:gmatch("[^\n]+") do cfg[#cfg + 1] = line end
print(#cfg, cfg[2], cfg[3], cfg[4], cfg[5], cfg[1] == "/" or cfg[1] == "\\")

-- searchers: preload, Lua, C, all-in-one
print(type(package.searchers), #package.searchers)
print(type(package.searchpath), type(package.loadlib))
