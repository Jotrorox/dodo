--! opts: -e "package.path = './modules/?.lua;./modules/?/init.lua'" -l simplemod -l alias=simplemod -lcounter -l pkg.sub
-- -l mod requires a module and stores it in the global "mod"; the 5.4
-- form -l g=mod stores it in global g; the argument may be attached;
-- a dotted name is stored under the full name (global "pkg.sub", no
-- table "pkg"). Options run left to right, so the -e that sets
-- package.path takes effect first.

print("simplemod global:", type(simplemod), simplemod and simplemod.name)
print("alias global:", type(alias), alias == simplemod)
print("simplemod data:", simplemod.data)
print("counter:", type(counter), counter_loads)
print("pkg.sub global:", type(_G["pkg.sub"]), type(pkg))
print("pkg.sub value:", _G["pkg.sub"] and _G["pkg.sub"].kind)
print("loaded:", package.loaded.simplemod == simplemod, package.loaded.alias)
local opts = {}
for i = -1, -20, -1 do
  if arg[i - 1] == nil then break end
  opts[#opts + 1] = arg[i]
end
print(#opts, opts[1], opts[2], opts[3])
