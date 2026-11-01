#!/usr/bin/env python3
"""Generates the bulk differential scripts (deterministic, seeded) into a
directory: b-tree splits and merges in table and index trees, overflow
pages, freelist reuse, large transactions."""
import os
import random
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


def main():
    target = sys.argv[1]
    os.makedirs(target, exist_ok=True)
    with open(os.path.join(target, "bulk_random.sql"), "w") as f:
        f.write(bulk(7, 3000))
    with open(os.path.join(target, "bulk_sequential.sql"), "w") as f:
        f.write(sequential(20000))


if __name__ == "__main__":
    main()
