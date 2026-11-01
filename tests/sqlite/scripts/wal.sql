pragma journal_mode=wal;
pragma journal_mode;
create table t(id integer primary key, n int, s text, b blob);
create index t_n on t(n);
insert into t values (1, 10, 'one', x'01');
insert into t values (2, 20, 'two', x'0202');
select * from t order by id;
pragma wal_checkpoint;
begin;
insert into t values (3, 30, 'three', zeroblob(100));
update t set n = n + 1 where id = 1;
select count(*), sum(n) from t;
rollback;
select count(*), sum(n) from t;
begin;
insert into t values (4, 40, 'four', x'04');
insert into t values (4, 41, 'dup', x'04');
insert into t values (5, 50, 'five', x'05');
commit;
select id, n, s from t order by id;
create table u(k text primary key, v);
insert into u values ('a', 1), ('b', 2), ('c', 3);
insert into u values ('a', 9);
select * from u order by k;
pragma wal_checkpoint(passive);
pragma wal_checkpoint(full);
pragma wal_checkpoint(restart);
insert into u values ('d', 4);
pragma wal_checkpoint(truncate);
pragma wal_checkpoint;
select * from u order by k;
pragma wal_autocheckpoint = 5;
pragma wal_autocheckpoint;
with recursive c(i) as (select 6 union all select i + 1 from c where i < 200)
insert into t select i, i * 10, 'row ' || i, cast(printf('%0300d', i * 7) as blob) from c;
insert into t values (201, 2010, 'x', x'');
insert into t values (202, 2020, 'y', x'');
insert into t values (203, 2030, 'z', x'');
select count(*), sum(n), sum(length(b)) from t;
delete from t where id % 3 = 0;
update t set s = upper(s) where id % 5 = 0;
select count(*), sum(n), sum(length(b)) from t;
pragma wal_autocheckpoint = 1000;
-- A transaction larger than the page cache spills frames before it commits.
begin;
with recursive c(i) as (select 1000 union all select i + 1 from c where i < 1099)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1100 union all select i + 1 from c where i < 1199)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1200 union all select i + 1 from c where i < 1299)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1300 union all select i + 1 from c where i < 1399)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1400 union all select i + 1 from c where i < 1499)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1500 union all select i + 1 from c where i < 1599)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1600 union all select i + 1 from c where i < 1699)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1700 union all select i + 1 from c where i < 1799)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1800 union all select i + 1 from c where i < 1899)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 1900 union all select i + 1 from c where i < 1999)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2000 union all select i + 1 from c where i < 2099)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2100 union all select i + 1 from c where i < 2199)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2200 union all select i + 1 from c where i < 2299)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2300 union all select i + 1 from c where i < 2399)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2400 union all select i + 1 from c where i < 2499)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2500 union all select i + 1 from c where i < 2599)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2600 union all select i + 1 from c where i < 2699)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2700 union all select i + 1 from c where i < 2799)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2800 union all select i + 1 from c where i < 2899)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 2900 union all select i + 1 from c where i < 2999)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 3000 union all select i + 1 from c where i < 3099)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 3100 union all select i + 1 from c where i < 3199)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 3200 union all select i + 1 from c where i < 3299)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 3300 union all select i + 1 from c where i < 3399)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
select count(*) from t where id >= 1000;
insert into t values (1000, 0, 'dup', x'');
select count(*) from t where id >= 1000;
delete from t where id >= 3000;
commit;
select count(*), sum(n) from t;
-- Rolling back a spilled transaction discards its frames.
begin;
with recursive c(i) as (select 5000 union all select i + 1 from c where i < 5099)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5100 union all select i + 1 from c where i < 5199)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5200 union all select i + 1 from c where i < 5299)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5300 union all select i + 1 from c where i < 5399)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5400 union all select i + 1 from c where i < 5499)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5500 union all select i + 1 from c where i < 5599)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5600 union all select i + 1 from c where i < 5699)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5700 union all select i + 1 from c where i < 5799)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5800 union all select i + 1 from c where i < 5899)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 5900 union all select i + 1 from c where i < 5999)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6000 union all select i + 1 from c where i < 6099)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6100 union all select i + 1 from c where i < 6199)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6200 union all select i + 1 from c where i < 6299)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6300 union all select i + 1 from c where i < 6399)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6400 union all select i + 1 from c where i < 6499)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6500 union all select i + 1 from c where i < 6599)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6600 union all select i + 1 from c where i < 6699)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6700 union all select i + 1 from c where i < 6799)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6800 union all select i + 1 from c where i < 6899)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 6900 union all select i + 1 from c where i < 6999)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 7000 union all select i + 1 from c where i < 7099)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 7100 union all select i + 1 from c where i < 7199)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 7200 union all select i + 1 from c where i < 7299)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
with recursive c(i) as (select 7300 union all select i + 1 from c where i < 7399)
insert into t select i, i, printf('%0200d', i), zeroblob(3000) from c;
rollback;
select count(*), sum(n) from t;
delete from t where id >= 1100;
pragma wal_checkpoint(truncate);
select count(*), sum(n) from t;
pragma integrity_check;
begin;
select count(*) from u;
pragma wal_checkpoint;
pragma journal_mode = delete;
commit;
begin;
pragma wal_checkpoint;
commit;
pragma journal_mode = delete;
pragma journal_mode;
insert into u values ('e', 5);
pragma journal_mode = wal;
insert into u values ('f', 6);
select * from u order by k;
pragma journal_mode;
