-- Expression indexes and partial indexes: maintained on every write and
-- checked by SQLite's integrity_check afterwards.
create table t(id integer primary key, name text, qty int, price real, tag text);
create index t_lower on t(lower(name));
create index t_total on t(qty * price desc, id);
create unique index t_tag on t(upper(tag)) where tag is not null;
create index t_big on t(qty) where qty > 10;
create index t_mix on t(name collate nocase, length(name), tag) where id % 2 = 0;
insert into t values (1, 'Apple', 5, 1.5, 'a'), (2, 'banana', 12, 0.25, null), (3, 'Cherry', 30, 3.0, 'c');
insert into t(name, qty, price, tag) values ('apple', 11, 2.0, 'd'), ('Date', null, 1.0, null);
insert into t(name, qty, price, tag) values ('dup', 1, 1, 'A');
insert or ignore into t(name, qty, price, tag) values ('dup', 1, 1, 'A');
select * from t order by id;
select id from t where lower(name) = 'apple' order by id;
select id from t where qty > 10 order by id;
update t set qty = qty + 10 where qty < 10;
update t set name = upper(name) where id % 2 = 0;
update t set tag = 'C' where id = 1;
update t set tag = 'z' where id = 1;
select * from t order by id;
delete from t where id = 3;
replace into t values (4, 'replaced', 50, 2.5, 'Z');
insert into t values (10, 'ten', 10, 1, 'x') on conflict do nothing;
insert into t values (11, 'eleven', 11, 1, 'X') on conflict do update set qty = excluded.qty + 1000;
select * from t order by id;
pragma index_list(t);
pragma index_info(t_total);
pragma index_info(t_mix);
pragma integrity_check;
-- An index created over existing rows.
create table big(k integer primary key, v text, w int);
with recursive c(i) as (select 1 union all select i + 1 from c where i < 1500)
insert into big select i, printf('v%04d', (i * 37) % 1500), i % 13 from c;
create index big_expr on big(substr(v, 2) + 0, w);
create index big_part on big(v) where w in (1, 2, 3);
create unique index big_u on big(v || '-' || k) where w = 0;
select count(*) from big where w in (1, 2, 3);
delete from big where k % 5 = 0;
update big set w = (w + 1) % 13 where k % 7 = 0;
insert into big(v, w) select v || 'x', w from big where k < 100;
select count(*), sum(w) from big;
pragma integrity_check;
-- Errors: unknown columns, a UNIQUE expression conflict, an aggregate.
create index bad on t(nosuch + 1);
create index bad2 on t(qty) where nosuch > 0;
create unique index t_dup on t(lower(name));
create index bad3 on t(count(*));
select name from sqlite_schema where type = 'index' order by name;
-- DROP COLUMN sees columns used by these indexes.
alter table t drop column price;
alter table t drop column qty;
drop index t_total;
drop index t_big;
alter table t drop column price;
select sql from sqlite_schema where name = 't';
pragma integrity_check;
