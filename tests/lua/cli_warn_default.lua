--! stderr: full
-- Without -W, warnings start switched off: warn() prints nothing until a
-- script turns them on with warn("@on"); "@off" turns them off again.

warn("not shown: warnings start off")
print("after first warn")
io.stdout:flush()
warn("@on")
warn("shown after @on")
warn("@off")
warn("not shown after @off")
print("end")
