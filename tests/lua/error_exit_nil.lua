-- Uncaught error with a nil error value: lua.c reports
-- "(error object is a nil value)" and exits with status 1.

print("start")
print(pcall(error, nil))
error(nil)
