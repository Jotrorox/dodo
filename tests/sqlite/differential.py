#!/usr/bin/env python3
"""Differential test: run SQL scripts through SQLite (Python's sqlite3 module)
and through the Dodo shell (tests/sqlite/shell.dodo), compare every result row,
then check the Dodo-written database with SQLite's integrity_check and compare
the final table contents.

Usage: differential.py SHELL_BINARY WORKDIR SCRIPT...
"""
import os
import sqlite3
import subprocess
import sys


def fmt(conn, value):
    if value is None:
        return "NULL"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return conn.execute("select cast(? as text)", (value,)).fetchone()[0]
    if isinstance(value, bytes):
        return "x'" + value.hex().upper() + "'"
    return value


def same_row(e, g):
    """Rows match exactly, or differ only in how reals are printed: SQLite
    3.47 changed real-to-text conversion from 15 to (up to) 17 significant
    digits, and this driver follows the newer rule."""
    if e == g:
        return True
    a, b = e.split("|"), g.split("|")
    if len(a) != len(b):
        return False
    for x, y in zip(a, b):
        if x == y:
            continue
        try:
            fx, fy = float(x), float(y)
        except ValueError:
            return False
        if ("." not in x and "e" not in x) or ("." not in y and "e" not in y):
            return False
        if abs(fx - fy) > 1e-13 * max(abs(fx), abs(fy)):
            return False
    return True


def statements(script):
    buf = ""
    for line in script.splitlines(keepends=True):
        buf += line
        if sqlite3.complete_statement(buf):
            if buf.strip():
                yield buf
            buf = ""
    if buf.strip():
        yield buf


def oracle(path, script):
    conn = sqlite3.connect(path, isolation_level=None)
    out = []
    for stmt in statements(script):
        try:
            cur = conn.execute(stmt)
            for row in cur.fetchall():
                out.append("|".join(fmt(conn, v) for v in row))
        except sqlite3.Error as error:
            out.append("ERROR: " + str(error))
    conn.close()
    return out


def contents(path):
    conn = sqlite3.connect(path)
    rows = []
    tables = [r[0] for r in conn.execute(
        "select name from sqlite_schema where type='table' order by name")]
    for t in tables:
        q = '"' + t.replace('"', '""') + '"'
        for row in conn.execute(f"select * from {q} order by rowid"):
            rows.append(t + ": " + "|".join(fmt(conn, v) for v in row))
    schema = [r for r in conn.execute(
        "select type, name, tbl_name, sql from sqlite_schema order by name")]
    check = conn.execute("pragma integrity_check").fetchall()
    conn.close()
    return rows, schema, check


def main():
    shell, workdir = sys.argv[1], sys.argv[2]
    failures = 0
    for script_path in sys.argv[3:]:
        name = os.path.basename(script_path)
        with open(script_path, encoding="utf-8") as f:
            script = f.read()
        # "-- requires: X.Y.Z" skips scripts for features newer SQLite added.
        first = script.split("\n", 1)[0]
        if first.startswith("-- requires:"):
            need = tuple(int(x) for x in first.split(":", 1)[1].strip().split("."))
            if sqlite3.sqlite_version_info < need:
                print(f"skip {name} (needs SQLite {first.split(':', 1)[1].strip()})")
                continue
        a = os.path.join(workdir, name + ".sqlite.db")
        b = os.path.join(workdir, name + ".dodo.db")
        for p in (a, b, b + "-journal"):
            if os.path.exists(p):
                os.remove(p)
        expected = "\n".join(oracle(a, script)).splitlines()
        run = subprocess.run([shell, b, script_path], capture_output=True)
        actual = run.stdout.decode("utf-8", "replace").splitlines()
        problems = []
        if run.returncode != 0:
            problems.append(f"shell exited with {run.returncode}: {run.stderr.decode()[-2000:]}")
        # "-- compare-errors" makes error messages count, not just errors.
        exact_errors = "-- compare-errors" in script.split("\n", 2)[:2]
        for i in range(max(len(expected), len(actual))):
            e = expected[i] if i < len(expected) else "<missing>"
            g = actual[i] if i < len(actual) else "<missing>"
            if e.startswith("ERROR") and g.startswith("ERROR") and not exact_errors:
                continue
            if not same_row(e, g):
                problems.append(f"row {i}: expected {e!r}, got {g!r}")
                if len(problems) > 20:
                    break
        if not problems and os.path.exists(b):
            ra, sa, _ = contents(a)
            rb, sb, cb = contents(b)
            if cb != [("ok",)]:
                problems.append(f"integrity_check of the Dodo database: {cb}")
            if len(ra) != len(rb) or not all(same_row(x, y) for x, y in zip(ra, rb)):
                for x, y in zip(ra, rb):
                    if not same_row(x, y):
                        problems.append(f"content differs: sqlite {x!r} dodo {y!r}")
                        break
                if len(ra) != len(rb):
                    problems.append(f"content rows: sqlite {len(ra)} dodo {len(rb)}")
            if sa != sb:
                problems.append(f"schema differs:\n  sqlite {sa}\n  dodo   {sb}")
        if not problems and os.path.exists(a):
            # Read the SQLite-written database back through Dodo.
            ra, _, _ = contents(a)
            conn = sqlite3.connect(a)
            tables = [r[0] for r in conn.execute(
                "select name from sqlite_schema where type='table' order by name")]
            conn.close()
            dump = os.path.join(workdir, name + ".dump.sql")
            with open(dump, "w", encoding="utf-8") as f:
                for t in tables:
                    q = '"' + t.replace('"', '""') + '"'
                    f.write(f"select * from {q} order by rowid;\n")
            run = subprocess.run([shell, a, dump], capture_output=True)
            got = run.stdout.decode("utf-8", "replace")
            want = []
            conn = sqlite3.connect(a)
            for t in tables:
                q = '"' + t.replace('"', '""') + '"'
                for row in conn.execute(f"select * from {q} order by rowid"):
                    want.append("|".join(fmt(conn, v) for v in row))
            conn.close()
            gl, wl = got.splitlines(), "\n".join(want).splitlines()
            if len(gl) != len(wl) or not all(same_row(x, y) for x, y in zip(wl, gl)):
                problems.append("reading the SQLite-written database through Dodo differs")
        if problems:
            failures += 1
            print(f"FAIL {name}")
            for p in problems:
                print("   ", p)
        else:
            print(f"ok   {name}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
