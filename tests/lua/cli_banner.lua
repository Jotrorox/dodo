--! cmd: -i {script} arg1
--! stdin: fixtures/repl_input.txt
-- The only test that prints the version banner: -i implies -v, so the
-- banner is printed first; then the script runs, then the REPL reads
-- stdin (not a terminal: no readline, prompts "> " and ">> " are written
-- to stdout without newlines). Expressions are printed, statements are
-- not, incomplete input continues on the next line, a local declared on
-- one line is gone on the next, and an error in the REPL is reported to
-- stderr without the "lua:" prefix (the REPL continues). EOF ends the
-- session with a newline and exit status 0.

function greet(who) return "hello " .. who end
print("script ran with", ...)
