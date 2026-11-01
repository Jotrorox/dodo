--! cmd: -e "print('ok so far')" -e "x = = 1" {script}
-- A syntax error in a -e chunk: "lua: (command line):1: unexpected
-- symbol near '='", exit status 1, nothing after it runs.

print("the script must not run")
