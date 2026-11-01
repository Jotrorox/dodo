#!/usr/bin/env python3
"""Interoperability with SQLite on one database file: hot-journal recovery in
both directions and the POSIX lock protocol. Usage: interop.py SHELL WORKDIR"""
import os
import sqlite3
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
    for p in (db, db + "-journal"):
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

sys.exit(1 if failures else 0)
