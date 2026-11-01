#!/usr/bin/env python3
"""Interoperability with SQLite on one database file: hot-journal recovery in
both directions, the POSIX lock protocol, and WAL mode (crash recovery both
ways, concurrent readers and writers, checkpoints, big-endian checksums).
Usage: interop.py SHELL WORKDIR"""
import os
import shutil
import sqlite3
import struct
import subprocess
import sys
import time

SHELL, WORK = sys.argv[1], sys.argv[2]
failures = []


def shell(db, script, **kw):
    path = os.path.join(WORK, "interop_script.sql")
    with open(path, "w") as f:
        f.write(script)
    return subprocess.run([SHELL, db, path], capture_output=True, **kw)


def fresh(name):
    db = os.path.join(WORK, name)
    for p in (db, db + "-journal", db + "-wal", db + "-shm"):
        if os.path.exists(p):
            os.remove(p)
    return db


def check(cond, what):
    print(("ok   " if cond else "FAIL ") + what)
    if not cond:
        failures.append(what)


def summary(db):
    c = sqlite3.connect(db)
    r = c.execute("select count(*), sum(n), sum(length(data)), group_concat(substr(hex(data), 1, 4), '') from t").fetchone()
    ok = c.execute("pragma integrity_check").fetchone()[0]
    c.close()
    return r, ok


# 1. SQLite crashes mid-transaction after spilling pages; Dodo rolls back.
db = fresh("crash_sqlite.db")
c = sqlite3.connect(db, isolation_level=None)
c.execute("create table t(id integer primary key, n int, data blob)")
c.execute("begin")
for i in range(2000):
    c.execute("insert into t values (?, ?, ?)", (i, i, bytes([i % 251]) * 1500))
c.execute("commit")
c.close()
before, _ = summary(db)
child = """
import os, sqlite3, sys
c = sqlite3.connect(sys.argv[1], isolation_level=None)
c.execute("pragma cache_size = 5")
c.execute("begin")
c.execute("update t set data = zeroblob(1500), n = -n")
c.execute("delete from t where id % 3 = 0")
os._exit(0)
"""
subprocess.run([sys.executable, "-c", child, db], check=True)
check(os.path.exists(db + "-journal"), "SQLite left a hot journal")
out = shell(db, "select count(*), sum(n), sum(length(data)) from t;\n")
check(out.stdout.decode().strip() == f"{before[0]}|{before[1]}|{before[2]}", "Dodo rolled the hot journal back: " + out.stdout.decode().strip())
after, ok = summary(db)
check(after == before and ok == "ok", "database restored and intact after Dodo's recovery")
check(not os.path.exists(db + "-journal"), "Dodo deleted the journal")

# 2. Dodo crashes mid-transaction after spilling pages; SQLite rolls back.
db = fresh("crash_dodo.db")
c = sqlite3.connect(db, isolation_level=None)
c.execute("create table t(id integer primary key, n int, data blob)")
c.execute("begin")
for i in range(6000):
    c.execute("insert into t values (?, ?, ?)", (i, i, bytes([i % 251]) * 1500))
c.execute("commit")
c.close()
before, _ = summary(db)
out = shell(db, "begin;\nupdate t set data = zeroblob(1500), n = -n;\ndelete from t where id % 3 = 0;\nselect '__crash__';\n")
check(out.returncode != 0, "Dodo aborted inside the transaction")
check(os.path.exists(db + "-journal"), "Dodo left a hot journal")
after, ok = summary(db)
check(after == before and ok == "ok", "SQLite rolled Dodo's hot journal back")
check(not os.path.exists(db + "-journal"), "SQLite deleted the journal")

# 3. Locks: a SQLite writer blocks Dodo; a Dodo writer blocks SQLite.
db = fresh("locks.db")
c = sqlite3.connect(db, isolation_level=None, timeout=0)
c.execute("create table t(id integer primary key, n int, data blob)")
c.execute("begin immediate")
c.execute("insert into t values (1, 1, x'00')")
out = shell(db, "insert into t values (2, 2, x'00');\n")
check("database is locked" in out.stdout.decode(), "Dodo sees SQLite's write lock: " + out.stdout.decode().strip())
c.execute("commit")
out = shell(db, "insert into t values (2, 2, x'00');\nselect count(*) from t;\n")
check(out.stdout.decode().strip() == "2", "Dodo writes after SQLite commits")
# SQLite holds a read transaction: Dodo cannot commit a write.
c.execute("begin")
c.execute("select count(*) from t").fetchone()
out = shell(db, "insert into t values (3, 3, x'00');\nselect count(*) from t;\n")
check("database is locked" in out.stdout.decode(), "Dodo cannot commit while SQLite reads")
c.execute("commit")
# Dodo holds a write transaction while sleeping.
proc = subprocess.Popen([SHELL, db, os.path.join(WORK, "interop_hold.sql")], stdout=subprocess.PIPE) if False else None
hold = os.path.join(WORK, "interop_hold.sql")
with open(hold, "w") as f:
    f.write("begin immediate;\ninsert into t values (10, 10, x'00');\nselect '__sleep__', 1500;\ncommit;\n")
proc = subprocess.Popen([SHELL, db, hold], stdout=subprocess.PIPE)
time.sleep(0.6)
try:
    c.execute("insert into t values (11, 11, x'00')")
    locked = False
except sqlite3.OperationalError as error:
    locked = "locked" in str(error)
check(locked, "SQLite sees Dodo's write lock")
proc.wait()
c.execute("insert into t values (11, 11, x'00')")
check(c.execute("select count(*) from t").fetchone()[0] == 4, "both writers' rows are present")
c.close()
_, ok = summary(db)
check(ok == "ok", "integrity after interleaved writers")

# ---------------------------------------------------------------------------
# WAL mode


def wal_size(db):
    return os.path.getsize(db + "-wal") if os.path.exists(db + "-wal") else 0


def out_lines(out):
    return out.stdout.decode().strip().splitlines()


def make_wal_db(name, rows=300):
    db = fresh(name)
    c = sqlite3.connect(db, isolation_level=None)
    c.execute("pragma journal_mode=wal")
    c.execute("create table t(id integer primary key, n int, data blob)")
    c.execute("begin")
    for i in range(rows):
        c.execute("insert into t values (?, ?, ?)", (i, i, bytes([i % 251]) * 200))
    c.execute("commit")
    c.close()
    return db


# 4. A SQLite process dies with committed transactions only in the WAL and
#    an uncommitted (spilled) one after them; Dodo rebuilds the wal-index from
#    the log and sees exactly the committed state.
db = fresh("wal_sqlite_crash.db")
child = """
import os, sqlite3, sys
c = sqlite3.connect(sys.argv[1], isolation_level=None)
c.execute("pragma journal_mode=wal")
c.execute("pragma wal_autocheckpoint=0")
c.execute("create table t(id integer primary key, n int, data blob)")
for k in range(5):
    c.execute("begin")
    for i in range(k * 400, k * 400 + 400):
        c.execute("insert into t values (?, ?, ?)", (i, i, bytes([i % 251]) * 700))
    c.execute("commit")
c.execute("update t set n = -n where id % 7 = 0")
c.execute("pragma cache_size = 10")
c.execute("pragma cache_spill = 10")
c.execute("begin")
c.execute("update t set data = zeroblob(700), n = 0")
c.execute("insert into t values (99999, 1, x'00')")
os._exit(0)
"""
subprocess.run([sys.executable, "-c", child, db], check=True)
expect_n = sum(-i if i % 7 == 0 else i for i in range(2000))
check(wal_size(db) > 2000 * 700 + 300 * 4096, "SQLite left an un-checkpointed WAL with spilled frames: %d bytes" % wal_size(db))
check(os.path.getsize(db) < 100 * 1024, "the database file itself is still small")
out = shell(db, "pragma journal_mode;\nselect count(*), sum(n), sum(length(data)) from t;\nselect count(*) from t where id = 99999;\n")
check(out_lines(out) == ["wal", f"2000|{expect_n}|{2000 * 700}", "0"], "Dodo recovered SQLite's WAL: " + " / ".join(out_lines(out)))
check(not os.path.exists(db + "-wal"), "Dodo checkpointed and deleted the WAL on close")
c = sqlite3.connect(db)
r = c.execute("select count(*), sum(n), sum(length(data)) from t").fetchone()
ok = c.execute("pragma integrity_check").fetchone()[0]
mode = c.execute("pragma journal_mode").fetchone()[0]
c.close()
check(r == (2000, expect_n, 2000 * 700) and ok == "ok" and mode == "wal", f"SQLite reads the checkpointed database: {r} {ok} {mode}")

# 5. Dodo dies mid-transaction after spilling frames, with earlier commits
#    still in the WAL; SQLite recovers the committed state.
db = make_wal_db("wal_dodo_crash.db", rows=6000)
script = "pragma wal_autocheckpoint = 0;\n"
script += "insert into t values (100000, 5, x'01');\n"
script += "update t set n = n + 1 where id < 100;\n"
script += "begin;\nupdate t set data = zeroblob(1500), n = -n;\ndelete from t where id % 3 = 0;\nselect '__crash__';\n"
out = shell(db, script)
check(out.returncode != 0, "Dodo aborted inside the WAL transaction")
check(wal_size(db) > 6000 * 1000, "Dodo left frames in the WAL: %d bytes" % wal_size(db))
c = sqlite3.connect(db)
r = c.execute("select count(*), sum(n), sum(length(data)) from t").fetchone()
ok = c.execute("pragma integrity_check").fetchone()[0]
c.close()
want = (6001, sum(range(6000)) + 100 + 5, 6000 * 200 + 1)
check(r == want and ok == "ok", f"SQLite read Dodo's committed WAL frames only: {r} vs {want}, {ok}")

# 6. Live sharing: a SQLite connection stays open (its wal-index is live, no
#    recovery) while Dodo processes read and write.
db = make_wal_db("wal_live.db")
c = sqlite3.connect(db, isolation_level=None, timeout=5)
c.execute("insert into t values (1000, 1000, x'00')")
out = shell(db, "select count(*), sum(n) from t;\ninsert into t values (1001, 1001, x'00');\n")
check(out_lines(out) == [f"301|{sum(range(300)) + 1000}"], "Dodo reads SQLite's live WAL: " + " / ".join(out_lines(out)))
check(c.execute("select count(*) from t").fetchone()[0] == 302, "SQLite sees Dodo's commit")
check(os.path.exists(db + "-wal"), "Dodo kept the WAL while SQLite is connected")
# A SQLite read transaction keeps its snapshot while Dodo commits.
c.execute("begin")
before = c.execute("select count(*) from t").fetchone()[0]
out = shell(db, "insert into t values (1002, 1002, x'00');\nselect count(*) from t;\n")
during = c.execute("select count(*) from t").fetchone()[0]
c.execute("commit")
after = c.execute("select count(*) from t").fetchone()[0]
check(out_lines(out) == ["303"] and before == 302 and during == 302 and after == 303, f"snapshot isolation: {before} {during} {after} {out_lines(out)}")
# The reader's snapshot also limits Dodo's checkpoint.
c.execute("begin")
c.execute("select count(*) from t").fetchone()
out = shell(db, "insert into t values (1003, 1003, x'00');\npragma wal_checkpoint;\n")
lines = out_lines(out)
busy, log, done = (int(x) for x in lines[0].split("|"))
check(busy == 0 and 0 < done < log, "PASSIVE checkpoint stops at the reader's mark: " + lines[0])
out = shell(db, "pragma wal_checkpoint(full);\n")
check(out_lines(out)[0].startswith("1|"), "FULL checkpoint reports busy while SQLite reads: " + out_lines(out)[0])
c.execute("commit")
out = shell(db, "pragma wal_checkpoint(truncate);\nselect count(*) from t;\n")
check(out_lines(out) == ["0|0|0", "304"], "TRUNCATE checkpoint once the reader is done: " + " / ".join(out_lines(out)))
check(wal_size(db) == 0, "the WAL was truncated")
# Writers exclude each other.
c.execute("begin immediate")
c.execute("insert into t values (1004, 1004, x'00')")
out = shell(db, "insert into t values (1005, 1005, x'00');\nselect count(*) from t;\n")
check(out_lines(out) == ["ERROR: database is locked", "304"], "Dodo waits for SQLite's WAL write lock: " + " / ".join(out_lines(out)))
c.execute("commit")
hold = os.path.join(WORK, "interop_hold.sql")
with open(hold, "w") as f:
    f.write("begin immediate;\ninsert into t values (1006, 1006, x'00');\nselect '__sleep__', 1500;\ncommit;\n")
proc = subprocess.Popen([SHELL, db, hold], stdout=subprocess.PIPE)
time.sleep(0.6)
c2 = sqlite3.connect(db, isolation_level=None, timeout=0)
try:
    c2.execute("insert into t values (1007, 1007, x'00')")
    locked = False
except sqlite3.OperationalError as error:
    locked = "locked" in str(error)
check(locked, "SQLite sees Dodo's WAL write lock")
check(c2.execute("select count(*) from t").fetchone()[0] == 305, "SQLite reads while Dodo writes")
proc.wait()
check(c2.execute("select count(*) from t").fetchone()[0] == 306, "SQLite sees Dodo's commit after the lock is released")
c2.close()
c.close()
_, ok = summary(db)
check(ok == "ok", "integrity after live sharing")

# 7. Concurrent writers (two Dodo, two SQLite) moving units between two rows
#    in transactions, with frequent automatic checkpoints (so log restarts)
#    and every checkpoint mode from SQLite, while a Dodo reader checks that
#    the total never changes.
db = make_wal_db("wal_stress.db", rows=10)
c = sqlite3.connect(db, isolation_level=None)
c.execute("create table bal(id integer primary key, v int)")
c.execute("insert into bal values (1, 1000), (2, 0)")
c.close()
N = 120
procs = []
for w in range(2):
    path = os.path.join(WORK, f"interop_loop{w}.sql")
    with open(path, "w") as f:
        f.write("pragma busy_timeout = 20000;\npragma wal_autocheckpoint = 7;\n")
        for i in range(N):
            f.write(f"insert into t values ({20000 + w * 1000 + i}, 1, zeroblob({i % 50 * 40}));\n")
            f.write("begin immediate;\nupdate bal set v = v - 1 where id = 1;\nupdate bal set v = v + 1 where id = 2;\ncommit;\n")
            if i % 10 == 0:
                f.write("select 'total', sum(v) from bal;\n")
    procs.append(subprocess.Popen([SHELL, db, path], stdout=subprocess.PIPE, stderr=subprocess.PIPE))
child = """
import sqlite3, sys
w = int(sys.argv[2])
c = sqlite3.connect(sys.argv[1], isolation_level=None, timeout=20)
c.execute("pragma wal_autocheckpoint = 5")
for i in range(%d):
    c.execute("insert into t values (?, 2, zeroblob(?))", (30000 + w * 1000 + i, i %% 40 * 50))
    c.execute("begin immediate")
    c.execute("update bal set v = v - 1 where id = 1")
    c.execute("update bal set v = v + 1 where id = 2")
    c.execute("commit")
    if i %% 10 == 0:
        assert c.execute("select sum(v) from bal").fetchone()[0] == 1000
    if i %% 15 == 0:
        c.execute("pragma wal_checkpoint(%%s)" %% ["passive", "full", "restart", "truncate"][i // 15 %% 4]).fetchone()
""" % N
for w in range(2):
    procs.append(subprocess.Popen([sys.executable, "-c", child, db, str(w)], stdout=subprocess.PIPE, stderr=subprocess.PIPE))
reader = os.path.join(WORK, "interop_reader.sql")
with open(reader, "w") as f:
    f.write("pragma busy_timeout = 20000;\n")
    for i in range(300):
        f.write("select 'total', sum(v) from bal;\n")
        if i % 40 == 0:
            f.write("pragma wal_checkpoint;\n")
procs.append(subprocess.Popen([SHELL, db, reader], stdout=subprocess.PIPE, stderr=subprocess.PIPE))
problems = []
for i, p in enumerate(procs):
    o, err = p.communicate()
    bad = [l for l in o.decode().splitlines() if l.startswith("ERROR") or (l.startswith("total|") and l != "total|1000")]
    if p.returncode != 0 or bad:
        problems.append(f"process {i}: rc {p.returncode} {bad[:3]} {err.decode()[-300:]}")
check(not problems, "concurrent Dodo and SQLite writers, readers and checkpoints: " + "; ".join(problems))
c = sqlite3.connect(db)
r = c.execute("select count(*), sum(n) from t").fetchone()
b = c.execute("select v from bal order by id").fetchall()
ok = c.execute("pragma integrity_check").fetchone()[0]
c.close()
check(r == (10 + 4 * N, sum(range(10)) + 2 * N * 1 + 2 * N * 2) and b == [(1000 - 4 * N,), (4 * N,)] and ok == "ok", f"every transaction is present exactly once: {r} {b} {ok}")

# 8. Logs checksummed in big-endian word order (written by big-endian
#    hosts): Dodo recovers them and appends with the same order.
def to_big_endian_wal(path):
    data = bytearray(open(path, "rb").read())
    magic, version, page_size = struct.unpack(">III", data[0:12])
    assert magic == 0x377F0682

    def cksum(buf, s1, s2, big):
        fmt = ">%dI" if big else "<%dI"
        words = struct.unpack(fmt % (len(buf) // 4), buf)
        for i in range(0, len(words), 2):
            s1 = (s1 + words[i] + s2) & 0xFFFFFFFF
            s2 = (s2 + words[i + 1] + s1) & 0xFFFFFFFF
        return s1, s2
    s_le = cksum(bytes(data[0:24]), 0, 0, False)
    assert s_le == struct.unpack(">II", data[24:32])
    struct.pack_into(">I", data, 0, 0x377F0683)
    s_be = cksum(bytes(data[0:24]), 0, 0, True)
    struct.pack_into(">II", data, 24, *s_be)
    frame = 24 + page_size
    pos = 32
    while pos + frame <= len(data):
        hdr = data[pos:pos + 24]
        page = data[pos + 24:pos + frame]
        if hdr[8:16] != data[16:24]:
            break
        s_le = cksum(bytes(hdr[0:8]) + bytes(page), *s_le, False)
        if s_le != struct.unpack(">II", hdr[16:24]):
            break
        s_be = cksum(bytes(hdr[0:8]) + bytes(page), *s_be, True)
        struct.pack_into(">II", data, pos + 16, *s_be)
        pos += frame
    open(path, "wb").write(data[:pos])
    return (pos - 32) // frame


db = fresh("wal_bigendian.db")
child = """
import os, sqlite3, sys
c = sqlite3.connect(sys.argv[1], isolation_level=None)
c.execute("pragma journal_mode=wal")
c.execute("pragma wal_autocheckpoint=0")
c.execute("create table t(id integer primary key, n int, data blob)")
for i in range(50):
    c.execute("insert into t values (?, ?, zeroblob(300))", (i, i))
os._exit(0)
"""
subprocess.run([sys.executable, "-c", child, db], check=True)
os.remove(db + "-shm")
frames = to_big_endian_wal(db + "-wal")
copy = os.path.join(WORK, "wal_bigendian_copy.db")
for suffix in ("", "-wal"):
    shutil.copyfile(db + suffix, copy + suffix)
if os.path.exists(copy + "-shm"):
    os.remove(copy + "-shm")
c = sqlite3.connect(copy)
sanity = c.execute("select count(*), sum(n) from t").fetchone()
c.close()
check(frames > 50 and sanity == (50, sum(range(50))), f"converted SQLite's WAL to big-endian checksums ({frames} frames, SQLite reads {sanity})")
out = shell(db, "pragma wal_autocheckpoint = 0;\nselect count(*), sum(n) from t;\ninsert into t values (50, 50, x'01');\nselect '__crash__';\n")
check(out_lines(out)[:2] == ["0", f"50|{sum(range(50))}"], "Dodo recovered the big-endian WAL: " + " / ".join(out_lines(out)))
with open(db + "-wal", "rb") as f:
    magic = struct.unpack(">I", f.read(4))[0]
check(magic == 0x377F0683, "Dodo appended to the big-endian log in place")
os.remove(db + "-shm")
c = sqlite3.connect(db)
r = c.execute("select count(*), sum(n) from t").fetchone()
ok = c.execute("pragma integrity_check").fetchone()[0]
c.close()
check(r == (51, sum(range(51))) and ok == "ok", f"SQLite validated Dodo's big-endian frames: {r} {ok}")

# 9. journal_mode conversions both ways between the two engines.
db = fresh("wal_convert.db")
out = shell(db, "pragma journal_mode = wal;\ncreate table t(id integer primary key, n int, data blob);\ninsert into t values (1, 1, x'01');\n")
c = sqlite3.connect(db, isolation_level=None)
mode = c.execute("pragma journal_mode").fetchone()[0]
c.execute("insert into t values (2, 2, x'02')")
out2 = shell(db, "pragma journal_mode = delete;\n")
check(mode == "wal" and "locked" in out2.stdout.decode(), "Dodo cannot leave WAL mode while SQLite is connected: " + out2.stdout.decode().strip())
c.close()
out2 = shell(db, "pragma journal_mode = delete;\nselect count(*) from t;\n")
check(out_lines(out2) == ["delete", "2"] and not os.path.exists(db + "-wal"), "Dodo converted to rollback mode: " + " / ".join(out_lines(out2)))
c = sqlite3.connect(db, isolation_level=None)
mode = c.execute("pragma journal_mode").fetchone()[0]
c.execute("pragma journal_mode = wal")
c.execute("insert into t values (3, 3, x'03')")
c.close()
out2 = shell(db, "pragma journal_mode;\nselect count(*) from t;\n")
check(mode == "delete" and out_lines(out2) == ["wal", "3"], "Dodo opens a database SQLite converted to WAL: " + " / ".join(out_lines(out2)))
_, ok = summary(db)
check(ok == "ok", "integrity after conversions")

# 10. A writer dies with uncommitted frames while another process keeps the
#     wal-index live: the others ignore those frames, and the next writer
#     clears their stale hash entries (both directions).
for victim in ("sqlite", "dodo"):
    db = make_wal_db(f"wal_live_crash_{victim}.db", rows=3000)
    keeper = sqlite3.connect(db, isolation_level=None, timeout=5)
    keeper.execute("select count(*) from t").fetchone()
    before = wal_size(db)
    if victim == "sqlite":
        child = """
import os, sqlite3, sys
c = sqlite3.connect(sys.argv[1], isolation_level=None)
c.execute("pragma cache_size = 10")
c.execute("pragma cache_spill = 10")
c.execute("begin")
c.execute("update t set n = -1, data = zeroblob(300)")
os._exit(0)
"""
        subprocess.run([sys.executable, "-c", child, db], check=True)
    else:
        out = shell(db, "begin;\nupdate t set n = -1, data = zeroblob(3000);\nselect '__crash__';\n")
    grown = wal_size(db)
    out = shell(db, "select count(*), sum(n) from t;\ninsert into t values (5000, 7, x'07');\nupdate t set n = n + 1 where id < 10;\nselect count(*), sum(n) from t;\n")
    want = [f"3000|{sum(range(3000))}", f"3001|{sum(range(3000)) + 7 + 10}"]
    check(grown > before + 20 * 4096 and out_lines(out) == want, f"{victim} crashed with frames in a live WAL; Dodo ignores them and writes over them: " + " / ".join(out_lines(out)))
    keeper.execute("insert into t values (5001, 8, x'08')")
    r = keeper.execute("select count(*), sum(n) from t").fetchone()
    ok = keeper.execute("pragma integrity_check").fetchone()[0]
    keeper.close()
    check(r == (3002, sum(range(3000)) + 7 + 10 + 8) and ok == "ok", f"SQLite agrees after the {victim} crash: {r} {ok}")

sys.exit(1 if failures else 0)
