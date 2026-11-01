-- WITHOUT ROWID tables: PRIMARY KEY-ordered storage, secondary indexes that
-- carry the PRIMARY KEY, constraints, UPSERT, triggers and ALTER TABLE.
pragma legacy_alter_table = off;
create table w(a text, b unique, c, d, primary key (c desc, a)) without rowid;
create index wd on w(d);
insert into w values ('x', 1, 2, 3), ('y', 2, 2, 4), ('a', 3, 9, 4), ('b', null, -1, null);
select * from w;
select * from w order by d, a;
select a, c from w where d = 4 order by a;
select count(*), sum(c) from w;
select type, name, tbl_name from sqlite_schema order by name;
pragma table_info(w);
pragma index_info(wd);
-- Constraints.
insert into w values ('x', 10, 2, 0);
insert into w values (null, 11, 5, 0);
insert into w values ('z', 1, 5, 0);
insert or ignore into w values ('x', 12, 2, 0);
insert or replace into w values ('x', 13, 2, 30);
insert or replace into w values ('q', 3, 7, 31);
select * from w order by c, a;
select rowid from w;
-- UPDATE, including the PRIMARY KEY itself.
update w set d = d * 10 where a > 'b';
update w set a = a || '!' where c = 2;
update w set c = 9, a = 'a' where a = 'q';
update w set b = 2 where a = 'x!';
select * from w order by c desc, a;
-- UPSERT on the PRIMARY KEY and on a UNIQUE column.
insert into w values ('y!', 99, 2, 0) on conflict (c, a) do update set d = excluded.d + 1;
insert into w values ('new', 13, 50, 1) on conflict (b) do update set d = -1;
insert into w values ('k', 77, 70, 1) on conflict do nothing;
select * from w order by c desc, a;
-- DELETE.
delete from w where d < 0;
delete from w where a like 'y%';
select * from w order by c desc, a;
pragma integrity_check;
-- Text keys with collations, integer keys, many rows (multi-level tree).
create table kv(k text primary key collate nocase, v) without rowid;
insert into kv values ('Apple', 1), ('banana', 2), ('CHERRY', 3);
insert into kv values ('apple', 9);
select * from kv order by k;
select v from kv where k = 'BANANA';
create table big(id integer primary key, x int, y text) without rowid;
with recursive n(i) as (select 1 union all select i + 1 from n where i < 3000)
insert into big select (i * 7919) % 3001, i % 17, printf('%050d', i) from n;
create index big_x on big(x, y);
select count(*), min(id), max(id), sum(x) from big;
select id, x from big where x = 5 order by y limit 3;
delete from big where id % 3 = 0;
update big set x = x + 100 where id % 5 = 0;
select count(*), sum(x) from big;
select count(*) from big where x > 100;
pragma integrity_check;
-- Triggers and RETURNING.
create table audit(msg);
create trigger kvt after update on kv begin insert into audit values (old.k || ':' || old.v || '->' || new.v); end;
update kv set v = v * 100 returning k, v;
select * from audit;
-- ALTER TABLE.
alter table w add column e default 'e';
alter table w drop column b;
alter table w drop column c;
alter table w rename column a to aa;
select * from w order by c desc, aa;
select sql from sqlite_schema where name = 'w';
pragma integrity_check;
-- Errors.
create table nopk(a, b) without rowid;
create table ai(a integer primary key autoincrement) without rowid;
-- PRIMARY KEY lookups.
create table pkl(a int, b text, c, primary key (a, b desc)) without rowid;
with recursive n(i) as (select 1 union all select i + 1 from n where i < 500)
insert into pkl select i % 50, printf('k%03d', i), i from n;
select c from pkl where a = 7 order by b desc;
select c from pkl where a = 7 and b = 'k257';
select c from pkl where a = '7' and b = 'k257';
select count(*) from pkl where a = 51;
select p.c, q.c from pkl p join pkl q on q.a = p.a and q.b = p.b where p.a = 3 order by p.b;
