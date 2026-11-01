-- ALTER TABLE DROP COLUMN and ADD COLUMN text placement.
create table t(a integer primary key, b text, c int default 7, d real, e);
insert into t values (1, 'one', 10, 1.5, x'00ff'), (2, 'two', 20, 2.0, null), (3, null, null, 3, 'three');
alter table t drop column c;
select * from t order by a;
select sql from sqlite_schema where name = 't';
alter table t drop column e;
select * from t order by a;
select sql from sqlite_schema where name = 't';
alter table t drop column "b";
select * from t order by a;
pragma table_info(t);
alter table t drop column nosuch;
alter table t drop column a;
alter table t drop d;
select sql from sqlite_schema where name = 't';
create table single(only_col);
alter table single drop column only_col;
-- Constraints that refer to the column block the drop.
create table u(x, y unique, z check (z > 0), w, v, check (w <> v), primary key (x, v));
alter table u drop column y;
alter table u drop column w;
alter table u drop column v;
alter table u drop column x;
alter table u drop column z;
select sql from sqlite_schema where name = 'u';
create table i(p, q, r, s);
create index iq on i(q);
alter table i drop column q;
alter table i drop column r;
alter table i drop column s;
select sql from sqlite_schema where name like 'i%' order by name;
create table fk(m, n, foreign key (n) references i(p));
alter table fk drop column n;
alter table fk drop column m;
create table uq(a, b, c, unique (a, b));
alter table uq drop column b;
alter table uq drop column c;
select sql from sqlite_schema where name = 'uq';
-- Rows written before ADD COLUMN pick up the default when rewritten.
create table grow(a, b);
insert into grow values (1, 2);
alter table grow add column c default 'dflt';
alter table grow add column d;
insert into grow values (3, 4, 'given', 5);
alter table grow drop column b;
select * from grow order by a;
select typeof(c), typeof(d) from grow order by a;
-- ADD COLUMN goes before table constraints.
create table tc(a, b, primary key (a), check (b > 0));
alter table tc add column c text;
select sql from sqlite_schema where name = 'tc';
insert into tc values (1, 2, 'x');
select * from tc;
-- Many rows and an index on another column survive.
create table many(id integer primary key, keep text, gone blob, other int);
create index many_other on many(other);
with recursive n(i) as (select 1 union all select i + 1 from n where i < 2000)
insert into many select i, printf('row %d', i), zeroblob(100 + i % 50), i % 97 from n;
alter table many drop column gone;
select count(*), sum(other), min(keep), max(keep) from many;
select id from many where other = 13 order by id limit 5;
pragma integrity_check;
-- Inside a transaction, then rolled back.
begin;
alter table many drop column keep;
select * from many where id = 5;
rollback;
select * from many where id = 5;
-- Like SQLite, a drop that breaks a view succeeds, and later ALTERs fail.
create table base(k, l, m);
create view vw as select k, m from base;
alter table base drop column m;
alter table base drop column l;
select sql from sqlite_schema where name = 'base';
