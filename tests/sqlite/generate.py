#!/usr/bin/env python3
"""Generates the bulk differential scripts (deterministic, seeded) into a
directory: b-tree splits and merges in table and index trees, overflow
pages, freelist reuse, large transactions; real-number formatting and
parsing over many random doubles; random Unicode text in UTF-16 databases."""
import os
import random
import struct
import sys


def bulk(seed, rows):
    r = random.Random(seed)
    out = []
    w = out.append
    w("create table t(id integer primary key, name text, n int, data blob);")
    w("create index t_name on t(name);")
    w("create index t_n on t(n, name);")
    w("begin;")
    ids = list(range(1, rows + 1))
    r.shuffle(ids)
    for i in ids:
        size = r.choice([0, 5, 40, 300, 1500, 5000, 20000]) if r.random() < 0.3 else r.randint(0, 60)
        name = "name%06d" % r.randint(0, rows * 3) + ("x" * r.randint(0, 200) if r.random() < 0.2 else "")
        w(f"insert into t values ({i * 7}, '{name}', {r.randint(-1000, 1000)}, randomblob({size}));")
    w("commit;")
    w("select count(*), sum(n), sum(length(data)), count(distinct name) from t;")
    w("select id, name, n, length(data) from t order by id limit 20;")
    w("select name, count(*) from t group by name order by count(*) desc, name limit 10;")
    # Deletes in random order shrink pages and trigger merges.
    victims = r.sample(ids, rows // 2)
    w("begin;")
    for i in victims:
        w(f"delete from t where id = {i * 7};")
    w("commit;")
    w("select count(*), sum(n), sum(length(data)) from t;")
    w("select id, n from t where n between -10 and 10 order by n, id;")
    # Updates that change sizes (overflow chains grow and shrink).
    w("begin;")
    for i in r.sample([x for x in ids if x not in set(victims)], rows // 5):
        w(f"update t set data = randomblob({r.choice([0, 10, 8000, 30000])}), n = n + 1 where id = {i * 7};")
    w("commit;")
    w("select count(*), sum(n), sum(length(data)) from t;")
    w("delete from t where n % 3 = 0;")
    w("select count(*) from t;")
    w("select min(name), max(name), min(id), max(id) from t;")
    # Freed pages are reused by a new table.
    w("create table u as_placeholder;") if False else None
    w("create table u(k text primary key, v);")
    w("begin;")
    for k in range(rows // 2):
        w(f"insert into u values ('key{r.randint(0, 10**9):010d}', {k});")
    w("commit;")
    w("select count(*), sum(v) from u;")
    w("drop table t;")
    w("create table v(a, b);")
    w("begin;")
    for k in range(rows // 4):
        w(f"insert into v values ({k}, '{'z' * r.randint(0, 3000)}');")
    w("commit;")
    w("select count(*), sum(length(b)) from v;")
    w("delete from v;")
    w("select count(*) from v;")
    w("pragma integrity_check;")
    return "\n".join(x for x in out if x) + "\n"


def sequential(rows):
    out = ["create table s(id integer primary key, a text, b real);", "create index s_a on s(a);", "begin;"]
    for i in range(rows):
        out.append(f"insert into s(a, b) values ('v{i % 977:05d}', {i * 1.25});")
    out.append("commit;")
    out.append("select count(*), sum(b), min(a), max(a) from s;")
    out.append("select a, count(*) from s group by a order by a limit 5;")
    out.append("delete from s where id % 2 = 0;")
    out.append("select count(*), sum(b) from s;")
    out.append("update s set a = 'w' || a where id % 3 = 0;")
    out.append("select a, b from s where a like 'w%' order by b limit 5;")
    out.append("select count(*) from s where a >= 'v00500' and a < 'v00600';")
    out.append("pragma integrity_check;")
    return "\n".join(out) + "\n"


def random_double(r, i):
    kind = i % 6
    if kind == 0:
        # Any finite bit pattern.
        while True:
            x = struct.unpack("<d", struct.pack("<Q", r.getrandbits(64)))[0]
            if x == x and abs(x) != float("inf"):
                return x
    if kind == 1:
        return r.uniform(-1e6, 1e6)
    if kind == 2:
        return round(r.uniform(-1e4, 1e4), r.randint(0, 6))
    if kind == 3:
        return float(r.randint(-10 ** r.randint(1, 19), 10 ** r.randint(1, 19))) * 10.0 ** r.randint(-30, 30)
    if kind == 4:
        # Subnormals, tiny and huge exponents.
        e = r.choice([0, 1, 2, 1022, 1023, 1024, 2045, 2046])
        return struct.unpack("<d", struct.pack("<Q", r.getrandbits(52) | (e << 52)))[0]
    # Short decimals, whose shortest form has few digits.
    return float("%.*g" % (r.randint(1, 17), r.uniform(-1, 1) * 10.0 ** r.randint(-20, 20)))


def reals(seed, count):
    """Reals printed (cast, printf, quote) and parsed (literals, cast from
    text, round) exactly as SQLite does it."""
    r = random.Random(seed)
    formats = ["%.17g", "%.15g", "%!.20g", "%e", "%.3f", "%.25f", "%!.22e", "%g", "%#.10g",
               "%.0f", "%!.30f", "%E", "%.1e", "%+.4f", "%G", "%!.17g", "%#f", "% .9g", "%.40f",
               "%!.15g", "%10.4e", "%-12.6g|", "%015.3f"]
    # SQLite 3.53 changed its decimal conversion; older versions round some
    # last digits (even of 6-digit %g output) differently.
    out = ["-- requires: 3.53.0", "create table r(id integer primary key, x real);"]
    w = out.append
    for i in range(count):
        x = random_double(r, i)
        lit = r.choice([repr(x), "%.17g" % x, "%.16g" % x, "%.15g" % x, "%.20e" % x])
        text = "%.*g" % (r.randint(1, 25), x * r.random())
        f1, f2, f3 = r.sample(formats, 3)
        w(f"select cast({lit} as text), printf('{f1}', {lit}), printf('{f2}', {lit}), printf('{f3}', {lit}), "
          f"cast('{text}' as real), cast(cast('{text}' as real) as text), round({lit}, {r.randint(0, 32)}), quote({lit});")
        if i % 10 == 0:
            w(f"insert into r(x) values ({lit});")
    w("select id, x, x * 3, x / 7, -x from r order by id;")
    w("select sum(x), total(x), avg(x) from r where abs(x) < 1e300;")
    return "\n".join(out) + "\n"


def unicode_text(seed, rows, encoding):
    """Random strings over several Unicode ranges in a UTF-16 database: the
    BINARY order of UTF-16 bytes drives index order, sorting, and ranges."""
    r = random.Random(seed)
    ranges = [(0x20, 0x7e), (0xa0, 0xff), (0x100, 0x17f), (0x370, 0x3ff), (0x4e00, 0x4fff),
              (0xe000, 0xe0ff), (0xfb00, 0xfb06), (0xff01, 0xff5e), (0x1f600, 0x1f64f), (0x10400, 0x1044f)]

    def word():
        chars = []
        for _ in range(r.randint(0, 12)):
            lo, hi = r.choice(ranges)
            c = chr(r.randint(lo, hi))
            chars.append("''" if c == "'" else c)
        return "".join(chars)

    out = [f"pragma encoding = '{encoding}';", "create table w(id integer primary key, s text, t text collate nocase);",
           "create index w_s on w(s);", "create index w_t on w(t, s);", "begin;"]
    w = out.append
    words = [word() for _ in range(rows)]
    for i, s in enumerate(words):
        w(f"insert into w(s, t) values ('{s}', '{r.choice(words)}');")
    w("commit;")
    w("pragma encoding;")
    w("select count(*), count(distinct s), min(s), max(s), sum(length(s)), sum(octet_length(s)) from w;")
    w("select id, s from w order by s, id limit 60;")
    w("select id, s from w order by s desc, id limit 60;")
    w("select s, count(*) from w group by s having count(*) > 1 order by s;")
    w("select t, count(*) from w group by t order by t limit 40;")
    for _ in range(20):
        a, b = sorted([r.choice(words), r.choice(words)])
        w(f"select count(*), min(id), max(id) from w where s >= '{a}' and s < '{b}';")
    w("select id, hex(s) from w where id % 97 = 0 order by id;")
    w("delete from w where id % 3 = 0;")
    w("update w set s = s || 'é😀' where id % 5 = 0;")
    w("select id, s from w order by s, id limit 40;")
    w("pragma integrity_check;")
    return "\n".join(out) + "\n"


def main():
    target = sys.argv[1]
    os.makedirs(target, exist_ok=True)
    with open(os.path.join(target, "bulk_random.sql"), "w") as f:
        f.write(bulk(7, 3000))
    with open(os.path.join(target, "bulk_sequential.sql"), "w") as f:
        f.write(sequential(20000))
    with open(os.path.join(target, "bulk_reals.sql"), "w", encoding="utf-8") as f:
        f.write(reals(11, 6000))
    with open(os.path.join(target, "bulk_utf16le.sql"), "w", encoding="utf-8") as f:
        f.write(unicode_text(5, 1500, "UTF-16le"))
    with open(os.path.join(target, "bulk_utf16be.sql"), "w", encoding="utf-8") as f:
        f.write(unicode_text(6, 1500, "UTF-16be"))


if __name__ == "__main__":
    main()
