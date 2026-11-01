-- Uncaught error whose value is a number: lua.c converts numbers to
-- strings (no "error object" wording) and no position is added by error().
-- Also checks that a float error value keeps its float formatting.

print("start")
print(pcall(error, 42))
print(pcall(error, 2.5))
error(1.5e300 * 10)
