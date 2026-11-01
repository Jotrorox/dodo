--! cmd: -e "print('first -e ran')" -e "error('raised in -e')" -e "print('never')" {script}
-- An error inside a -e chunk stops the interpreter immediately: later -e
-- options and the script are not run. The chunk name of -e code is
-- "=(command line)", so the message reads "lua: (command line):1: ...".

print("the script must not run")
