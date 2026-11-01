--! opts: -W
--! stderr: full
-- -W turns warnings on: warn() writes "Lua warning: " plus the
-- concatenation of its arguments and a newline to stderr. Control
-- messages "@off"/"@on" switch them; unknown control messages are
-- ignored; a message starting with "@" in a multi-part warn is not a
-- control message. Numbers are accepted as strings; other types and a
-- missing argument raise errors.

warn("first warning")
warn("multi", "-", "part", " message")
io.stdout:write("stdout line 1\n"); io.stdout:flush()
warn("@off")
warn("hidden while off")
warn("@unknown control")
warn("@on")
warn("visible again")
warn("@not-control", " because multi-part")
print(pcall(warn))
print(pcall(warn, 1))
print(pcall(warn, "ok", {}))
print(pcall(warn, "a", nil))
warn("number args are strings: ", "1", " ", "2.5")
io.stdout:flush()
warn("")
warn("last")
