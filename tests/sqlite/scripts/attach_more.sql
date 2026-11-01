-- More ATTACH: other page sizes, WAL-mode attachments, ATTACH inside a
-- transaction, TEMP triggers on attached tables, cross-database views.
create table t(id integer primary key, v);
insert into t values (1, 'one');
-- An attached database with larger pages (created by PRAGMA before its
-- first table) and one with smaller pages.
attach 'big.db' as big;
pragma big.page_size = 8192;
create table big.b(x integer primary key, y);
pragma big.page_size;
insert into big.b(y) values (zeroblob(5000)), (zeroblob(7000));
insert into big.b(y) select y from big.b;
insert into big.b(y) select y from big.b;
select count(*), sum(length(y)) from big.b;
attach 'small.db' as small;
pragma small.page_size = 1024;
create table small.s(x integer primary key, y);
insert into small.s(y) select hex(y) from big.b;
select count(*), sum(length(y)) from small.s;
pragma small.page_size;
pragma page_size;
pragma big.integrity_check;
pragma small.integrity_check;
-- A WAL-mode attachment next to a rollback-mode main database.
attach 'wal.db' as w;
pragma w.journal_mode = wal;
create table w.k(a, b);
insert into w.k values (1, 2);
pragma w.journal_mode;
pragma journal_mode;
begin;
insert into w.k values (3, 4);
insert into t values (2, 'two');
insert into small.s(y) values ('in txn');
commit;
select * from w.k;
select count(*) from t;
begin;
insert into w.k values (5, 6);
insert into t values (3, 'three');
rollback;
select count(*) from w.k;
select count(*) from t;
pragma w.wal_checkpoint;
pragma w.journal_mode = delete;
select * from w.k;
-- ATTACH inside a transaction joins it.
begin;
insert into t values (10, 'before attach');
attach 'late.db' as late;
create table late.l(z);
insert into late.l values ('late');
commit;
select * from late.l;
select count(*) from t;
begin;
insert into late.l values ('rolled back');
insert into t values (11, 'rolled back');
rollback;
select * from late.l;
-- DETACH of a database the transaction uses fails.
begin;
insert into late.l values ('x');
detach late;
rollback;
detach late;
-- TEMP triggers on attached tables.
attach 'aux.db' as aux;
create table aux.a(id integer primary key, n);
create table aux.audit(what);
create temp trigger ta after insert on aux.a begin insert into aux.audit values ('ins ' || new.id); end;
create temp trigger tb after update of n on aux.a begin insert into aux.audit values ('upd ' || old.n || '>' || new.n); end;
create temp trigger tc before delete on aux.a begin insert into t values (100 + old.id, 'deleted from aux'); end;
insert into aux.a values (1, 10), (2, 20);
update aux.a set n = n + 1;
delete from aux.a where id = 1;
select * from aux.audit order by rowid;
select * from t where id > 100;
select type, name, tbl_name, sql from sqlite_temp_master order by name;
-- Dropping the attached table drops the TEMP triggers on it.
drop table aux.a;
select type, name from sqlite_temp_master order by name;
-- TEMP views may join databases.
create temp view tv as select t.v, late_count from t, (select count(*) as late_count from small.s);
select * from tv order by v;
-- The same table name in several databases.
create table aux.t(id integer primary key, v);
insert into aux.t values (1, 'aux one');
create temp table t(id integer primary key, v);
insert into t values (1, 'temp one');
select v from t;
select v from main.t where id = 1;
select v from aux.t;
drop table t;
select v from t where id = 1;
drop table t;
select v from t;
select v from aux.t;
detach aux;
select v from t;
-- Unique constraints and rollbacks inside one attached file.
create table small.u(k unique);
insert into small.u values (1), (2);
begin;
insert into small.u values (3);
insert into small.u values (2);
insert into small.u values (4);
commit;
select * from small.u order by k;
pragma database_list;
-- A transaction locks only the databases it uses: an untouched one can be
-- detached, a used one cannot.
attach 'j1.db' as j1;
attach 'j2.db' as j2;
create table j1.a(x);
create table j2.b(y);
begin;
select count(*) from t;
insert into j1.a values (1);
detach j2;
detach j1;
commit;
detach j1;
select * from j2.b;
-- An unqualified journal_mode change applies to every attached database.
attach 'j1.db' as j1;
pragma journal_mode = wal;
pragma j1.journal_mode;
pragma small.journal_mode;
attach 'j3.db' as j3;
pragma j3.journal_mode;
insert into j1.a values (2);
select * from j1.a;
pragma journal_mode = delete;
pragma j1.journal_mode;
pragma temp.journal_mode;
