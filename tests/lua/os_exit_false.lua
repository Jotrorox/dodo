-- os.exit(false) exits with EXIT_FAILURE (1); nothing is written to
-- stderr (it is not an error), and pcall cannot intercept it.

print("start")
print(pcall(os.exit, false))
print("not reached")
