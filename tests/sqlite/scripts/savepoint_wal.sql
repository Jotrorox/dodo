-- Savepoints in WAL mode.
pragma journal_mode = wal;
create table t(a integer primary key, b text);
create index tb on t(b);
savepoint one;
insert into t(b) values ('x');
savepoint two;
insert into t(b) values ('y');
select * from t order by a;
rollback to two;
select * from t order by a;
insert into t(b) values ('z');
release two;
select * from t order by a;
rollback to one;
select count(*) from t;
insert into t(b) values ('kept');
release one;
select * from t order by a;
-- RELEASE of the outermost savepoint commits.
rollback;
select * from t order by a;
-- Savepoints inside BEGIN.
begin;
insert into t(b) values ('b1');
savepoint s1;
insert into t(b) values ('b2');
savepoint s2;
insert into t(b) values ('b3');
savepoint s3;
delete from t;
select count(*) from t;
rollback to s2;
select * from t order by a;
release s1;
select * from t order by a;
rollback;
select * from t order by a;
-- Names are case-insensitive; the newest of equal names wins.
savepoint A;
insert into t(b) values ('a1');
savepoint a;
insert into t(b) values ('a2');
rollback to A;
select * from t order by a;
release a;
select * from t order by a;
release a;
select * from t order by a;
-- Errors.
release nosuch;
rollback to nosuch;
savepoint q;
begin;
rollback;
select * from t order by a;
-- ROLLBACK TO keeps the savepoint open; it can be rolled back to again.
savepoint r;
insert into t(b) values ('r1');
rollback to r;
insert into t(b) values ('r2');
rollback to savepoint r;
insert into t(b) values ('r3');
release savepoint r;
select * from t order by a;
-- A failing statement inside a savepoint undoes only itself.
create table u(x integer primary key, y unique);
savepoint f;
insert into u values (1, 'one'), (2, 'two');
insert into u values (3, 'three'), (4, 'one');
select * from u order by x;
release f;
select * from u order by x;
-- Schema changes roll back too.
savepoint ddl;
create table v(z);
insert into v values (1);
drop table u;
rollback to ddl;
select name from sqlite_schema order by name;
select * from u order by x;
create table w(k);
release ddl;
select name from sqlite_schema order by name;
-- A large transaction with nested savepoints touching many pages.
create table big(id integer primary key, payload text);
create index big_payload on big(payload);
begin;
with recursive c(i) as (select 1 union all select i + 1 from c where i < 3000)
insert into big(payload) select printf('%08d-%080d', i, i * 7) from c;
savepoint grow;
with recursive c(i) as (select 1 union all select i + 1 from c where i < 3000)
insert into big(payload) select printf('%08d-second-%0120d', i, i * 13) from c;
update big set payload = 'changed' || payload where id % 3 = 0;
savepoint inner_most;
delete from big where id % 2 = 0;
select count(*) from big;
rollback to inner_most;
select count(*) from big;
rollback to grow;
select count(*), sum(length(payload)) > 0 from big;
select count(*) from big where payload like 'changed%';
release grow;
commit;
select count(*) from big;
pragma integrity_check;
savepoint later;
update big set payload = 'x' where id < 1500;
delete from big where id > 2500;
rollback to later;
release later;
select count(*), min(id), max(id) from big;
select count(*) from big where payload = 'x';
pragma integrity_check;
