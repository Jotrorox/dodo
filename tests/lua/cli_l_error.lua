--! cmd: -e "package.path = './modules/?.lua'" -l errmod {script}
-- -l with a module whose chunk raises an error: lua.c reports the error
-- message with the module file position, exit status 1.

print("the script must not run")
