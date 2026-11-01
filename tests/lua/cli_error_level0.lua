-- An uncaught string error raised with level 0 has no position: stderr
-- shows just "lua: plain message" (multi-line messages are kept intact).

print("start")
error("plain message\nsecond line of the message", 0)
