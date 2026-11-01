-- require failure modes and the searcher machinery: the "module not
-- found" message listing every tried path (deterministic with controlled
-- package.path/cpath), errors raised while loading a module, syntax
-- errors, package.searchpath (custom separators, rep, missing files),
-- custom searchers inserted into package.searchers, and preload errors.

package.path = "./modules/?.lua;./modules/?/init.lua"
package.cpath = "./modules/?.so"

local function show(...)
  local ok, err = pcall(...)
  print(ok, err)
end

-- module not found: message lists preload, path and cpath attempts
show(require, "nonexistent_mod")
show(require, "deep.missing.name")
package.cpath = ""
show(require, "nonexistent_mod")
package.path = ""
show(require, "nonexistent_mod")
package.path = "./modules/?.lua;./modules/?/init.lua"

-- errors while running the module chunk propagate; nothing is cached
show(require, "errmod")
print(package.loaded.errmod)
show(require, "syntaxmod")

-- package.searchpath
print(package.searchpath("simplemod", package.path))
print(package.searchpath("pkg.sub", package.path))
print(package.searchpath("nothere", "./modules/?.lua;./modules/?.luac"))
print(package.searchpath("pkg_sub", "./modules/?.lua", "_", "/"))
print(package.searchpath("pkg.sub", "./modules/?.lua", ""))
print(package.searchpath("a.b", "x/?.lua;;y/?.lua"))
print(package.searchpath("simplemod", ""))

-- custom searcher at the front of package.searchers
local log = {}
table.insert(package.searchers, 1, function(name)
  log[#log + 1] = name
  if name:sub(1, 7) == "virtual" then
    return function(modname, extra)
      return {modname = modname, extra = extra}
    end, "virtual-data"
  end
  return "no virtual module '" .. name .. "'"
end)
local v, vdata = require("virtual.thing")
print(v.modname, v.extra, vdata)
print(require("simplemod").name)
show(require, "really_missing")
print(table.concat(log, ","))
table.remove(package.searchers, 1)

-- searcher returning nil message
table.insert(package.searchers, 1, function() return nil end)
show(require, "missing_again")
table.remove(package.searchers, 1)

-- searchers table that is not a table
local saved = package.searchers
package.searchers = nil
show(require, "anything")
package.searchers = saved

-- preload loader raising an error, and returning nil
package.preload.bad_preload = function() error("preload exploded", 0) end
show(require, "bad_preload")
package.preload.nil_preload = function() end
print(require("nil_preload"), package.loaded.nil_preload)
-- preload value that is not a function
package.preload.not_a_function = 42
show(require, "not_a_function")

-- argument errors
show(require)
show(require, {})
print(pcall(package.searchpath, nil, ""))
