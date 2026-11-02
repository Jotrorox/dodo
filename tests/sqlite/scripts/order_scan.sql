-- ORDER BY the rowid reads the table in order (forwards or backwards)
-- instead of sorting; small pages make the trees several levels deep.
pragma page_size = 512;
create table t(id integer primary key, g int, v text);
create table n(g int, v text);
create table e(id integer primary key, v);
create table one(id integer primary key, v);
insert into one values (-7, 'only');
with recursive c(i) as (select 1 union all select i + 1 from c where i < 1500)
insert into t select i * 3, i % 11, printf('%0*d', 20 + i % 60, i) from c;
insert into n select g, v from t;
insert into t values (-9223372036854775808, 0, 'min'), (9223372036854775807, 0, 'max'), (-5, 1, 'neg');
delete from t where id % 7 = 0 and id between 600 and 3000;
select id, g, length(v) from t order by id limit 12;
select id, g, length(v) from t order by id desc limit 12;
select id from t order by rowid desc limit 5 offset 1000;
select id from t order by id desc limit 5 offset 1495;
select id from t order by id limit 3 offset 1500;
select count(*), sum(id % 1000) from (select id from t order by id desc);
select group_concat(id) from (select id from t where g = 3 order by id desc limit 40);
select id, v from t where g = 5 and id < 400 order by rowid desc;
select id, g from t order by id, g desc limit 4;
select id, g from t order by id desc, v limit 4;
select id from t order by id nulls last limit 3;
select id from t order by id desc nulls first limit 3;
select id from t where id > 4000 order by id limit 5;
select id from t where id > 4000 order by id desc limit 5;
select id from t where id >= 4479 order by id;
select id, v from t where id = 300 order by id desc;
select id, v from t where id = 301 order by id desc;
select rowid, g, v from n order by rowid desc limit 6;
select rowid from n where v like '%99' order by rowid desc limit 6;
select g, count(*) from (select g from n order by rowid desc limit 100) group by g order by g;
select id from t order by id + 0 desc limit 3;
select id from t order by -id limit 3;
select * from e order by id desc;
select * from e order by id;
select * from one order by id desc;
select * from one order by id desc limit 1 offset 1;
select id from t order by id desc limit 0;
select g, (select id from t t2 where t2.g = t.g order by id desc limit 1) from t where id < 40 order by id desc;
select a.id, b.id from one a, t b where b.id < 10 order by a.id desc, b.id;
select id from t order by id desc limit -1 offset 1497;
-- Changes between statements and after the scan has started.
update t set v = v || 'x' where id % 5 = 0;
delete from t where id between 1000 and 2500;
select id, length(v) from t order by id desc limit 8 offset 300;
select count(*) from t;
pragma integrity_check;
