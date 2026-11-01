--! opts: -E -e "print('-e still runs with -E')"
-- -E ignores the LUA_INIT/LUA_PATH/LUA_CPATH environment variables (the
-- harness unsets them anyway, so this only checks that the option is
-- accepted, appears in arg, and that package.path/cpath are still set to
-- string defaults). With -E, package.path never comes from LUA_PATH.

print("arg[-1] =", arg[-1], "arg[-2] =", arg[-2], "arg[-3] =", arg[-3])
print("path types:", type(package.path), type(package.cpath))
print("default path has templates:", package.path:find("?", 1, true) ~= nil)
print("LUA_INIT visible:", os.getenv("LUA_INIT"))
