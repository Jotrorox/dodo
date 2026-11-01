--! cmd: -e "package.path = './modules/?.lua'; package.cpath = ''" -l no_such_module_cli {script}
-- -l with a module that cannot be found: the require error (listing the
-- controlled search paths) is reported as "lua: module '...' not found:"
-- and the script does not run; exit status 1.

print("the script must not run")
