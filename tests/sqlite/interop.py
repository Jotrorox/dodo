#!/usr/bin/env python3
"""Interoperability with SQLite on one database file: hot-journal recovery in
both directions, the POSIX lock protocol, and UTF-16 databases written by
both. Usage: interop.py SHELL WORKDIR"""
import os
import sqlite3
import subprocess
import sys
import time

SHELL, WORK = sys.argv[1], sys.argv[2]
failures = []


def shell(db, script, **kw):
    path = os.path.join(WORK, "interop_script.sql")
    with open(path, "w", encoding="utf-8") as f:
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

# 4. UTF-16 databases change hands: each side writes into the other's file
# (index seeks compare UTF-16 keys) and the other side reads it back.
words = ["zeta", "Ā", "ÿ", "€", "😀", "日本", "", "abc", "\ue000", "𝄞", "Ωmega", "naïve"]
for enc in ("UTF-16le", "UTF-16be"):
    db = fresh(f"utf16_{enc}.db")
    c = sqlite3.connect(db, isolation_level=None)
    c.execute(f"pragma encoding = '{enc}'")
    c.execute("create table t(id integer primary key, s text unique, n int)")
    c.execute("create index t_n on t(n, s)")
    for i, w in enumerate(words):
        c.execute("insert into t values (?, ?, ?)", (i, w * 3 + str(i), i % 4))
    c.close()
    script = "".join(f"insert into t(s, n) values ('{w}x', {i % 3});\n" for i, w in enumerate(words))
    script += "update t set s = s || 'é' where n = 1;\ndelete from t where n = 2;\npragma encoding;\nselect id, s from t order by s, id;\n"
    out = shell(db, script).stdout.decode()
    c = sqlite3.connect(db)
    want = c.execute("pragma encoding").fetchone()[0] + "\n" + "".join(f"{i}|{w}\n" for i, w in c.execute("select id, s from t order by s, id"))
    ok = c.execute("pragma integrity_check").fetchone()[0]
    c.close()
    check(out == want and ok == "ok", f"Dodo writes into SQLite's {enc} database")
    db2 = fresh(f"utf16_dodo_{enc}.db")
    shell(db2, f"pragma encoding = '{enc}';\ncreate table t(s text primary key, n);\ncreate index t_n on t(n, s);\n" + "".join(f"insert into t values ('{w}{i}', {i % 3});\n" for i, w in enumerate(words)))
    c = sqlite3.connect(db2, isolation_level=None)
    for i, w in enumerate(words):
        c.execute("insert into t values (?, ?)", (w + "!", i))
    c.execute("delete from t where n = 1")
    want = c.execute("pragma encoding").fetchone()[0] + "\n" + "".join(f"{s}|{n}\n" for s, n in c.execute("select s, n from t order by s"))
    ok = c.execute("pragma integrity_check").fetchone()[0]
    c.close()
    out = shell(db2, "pragma encoding;\nselect s, n from t order by s;\n").stdout.decode()
    check(out == want and ok == "ok", f"SQLite writes into Dodo's {enc} database")

sys.exit(1 if failures else 0)
