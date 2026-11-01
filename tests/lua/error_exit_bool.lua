-- Uncaught error whose value is a boolean: lua.c reports
-- "(error object is a boolean value)"; stdout written before is kept.

io.write("written ", 1, " ", 2.0, "\n")
error(false)
