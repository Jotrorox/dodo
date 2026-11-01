-- Triggers: timing, events, NEW/OLD, WHEN, UPDATE OF, RAISE, nesting,
-- INSTEAD OF on views, and changes()/last_insert_rowid() inside them.
create table t(a integer primary key, b, c text default 'dflt');
create table log(n integer primary key, x);
create trigger t1 after insert on t begin insert into log(x) values ('t1 ' || new.a || ' ' || new.b); end;
create trigger t2 after insert on t begin insert into log(x) values ('t2 ' || new.a); end;
create trigger t0 before insert on t begin insert into log(x) values ('before ' || new.a || ' ' || new.rowid || ' ' || typeof(new.b) || ' ' || new.c); end;
insert into t(b) values ('5'), ('x');
insert into t values (10, 1, 'c');
select changes(), total_changes(), last_insert_rowid();
select * from log order by n;
select name, tbl_name, sql from sqlite_schema where type = 'trigger' order by name;
-- WHEN and UPDATE OF.
create trigger u1 after update of b on t when new.b <> old.b begin insert into log(x) values ('u1 ' || old.a || ': ' || old.b || ' -> ' || new.b); end;
create trigger u0 before update on t begin insert into log(x) values ('u0 ' || old.a || ' ' || new.c); end;
update t set c = 'cc' where a = 1;
update t set b = b where a = 2;
update t set b = 'changed', c = c || '!' where a >= 2;
select * from t order by a;
delete from log;
-- DELETE triggers see OLD.
create trigger d0 before delete on t begin insert into log(x) values ('d0 ' || old.a || ' ' || old.b); end;
create trigger d1 after delete on t when old.a > 1 begin insert into log(x) values ('d1 ' || old.a); end;
delete from t where a in (1, 2);
select changes();
select x from log order by n;
delete from t;
select count(*) from t;
select x from log order by n;
-- RAISE.
create table r(a integer primary key, b);
create trigger rb before insert on r begin select raise(ignore) where new.b = 'skip'; select raise(abort, 'no negatives') where new.b < 0; end;
insert into r(b) values ('a'), ('skip'), ('c');
select changes();
select * from r order by a;
insert into r(b) values (1), (-1);
select * from r order by a;
begin;
insert into r(b) values (2);
insert into r(b) values (-5);
select * from r order by a;
commit;
create trigger rf before update on r when new.b = 'fail' begin select raise(fail, 'failing'); end;
update r set b = 'fail' where a = 3;
create trigger rr before delete on r when old.b = 'c' begin select raise(rollback, 'rolled back'); end;
begin;
insert into r(b) values ('in txn');
delete from r where b = 'c';
select * from r order by a;
commit;
select * from r order by a;
-- Nested triggers fire; a trigger does not fire itself recursively.
create table chain1(v);
create table chain2(v);
create table chain3(v);
create trigger c1 after insert on chain1 begin insert into chain2 values (new.v * 10); insert into chain1 values (new.v + 1000); end;
create trigger c2 after insert on chain2 begin insert into chain3 values (new.v + 1); update chain1 set v = v + 1 where v = new.v / 10; end;
create trigger c3 after update on chain1 begin insert into chain3 values ('upd ' || old.v || ' ' || new.v); end;
insert into chain1 values (1), (2);
select * from chain1 order by rowid;
select * from chain2 order by rowid;
select * from chain3 order by rowid;
select changes(), last_insert_rowid() > 0;
-- Subqueries and NEW in trigger bodies, multiple statements.
create table acct(id integer primary key, bal int not null);
create table moves(id, amount, total);
create trigger mv after update of bal on acct begin
  insert into moves values (new.id, new.bal - old.bal, (select sum(bal) from acct));
  insert into moves select new.id, count(*), max(bal) from acct where bal > old.bal;
end;
insert into acct values (1, 100), (2, 50), (3, 10);
update acct set bal = bal + 5;
update acct set bal = bal * 2 where id = 2;
select * from moves order by rowid;
-- UPSERT fires UPDATE triggers; OR REPLACE does not fire DELETE triggers.
create table kv(k text primary key, v);
create trigger kvu after update on kv begin insert into log(x) values ('kv ' || old.k || ' ' || old.v || ' -> ' || new.v); end;
create trigger kvd after delete on kv begin insert into log(x) values ('kv deleted ' || old.k); end;
create trigger kvi after insert on kv begin insert into log(x) values ('kv inserted ' || new.k); end;
delete from log;
insert into kv values ('a', 1), ('b', 2);
insert into kv values ('a', 10) on conflict(k) do update set v = excluded.v + v;
insert or replace into kv values ('b', 20);
insert into kv values ('c', 3) on conflict do nothing;
select * from kv order by k;
select x from log order by n;
-- Constraint errors in trigger bodies abort the statement.
create table strict_t(a unique);
create table src(a);
create trigger st after insert on src begin insert into strict_t values (new.a % 3); end;
insert into src values (1), (2);
insert into src values (4);
select * from src order by rowid;
select * from strict_t order by rowid;
-- OR IGNORE on the firing statement overrides the body.
insert or ignore into src values (5);
select * from src order by rowid;
select * from strict_t order by rowid;
-- INSTEAD OF triggers on views.
create table base(id integer primary key, name text, qty int);
insert into base values (1, 'apple', 3), (2, 'pear', 5);
create view v as select id, name, qty, qty * 2 as dbl from base;
insert into v values (3, 'fig', 1, 2);
create trigger vi instead of insert on v begin insert into base values (new.id, upper(new.name), new.qty); end;
create trigger vu instead of update of qty on v begin update base set qty = new.qty where id = old.id; insert into log(x) values ('vu ' || old.name || ' ' || old.dbl || ' -> ' || new.qty); end;
create trigger vd instead of delete on v begin delete from base where id = old.id; end;
insert into v values (3, 'fig', 1, 2);
insert into v(name, qty) values ('kiwi', 7);
select changes();
update v set qty = qty + 1 where id < 3;
update v set name = 'x';
select changes();
delete from v where name = 'FIG';
select * from base order by id;
select x from log where x like 'vu%' order by n;
-- Errors at creation.
create trigger t1 after insert on t begin select 1; end;
create trigger if not exists t1 after insert on t begin select 1; end;
create trigger bad before insert on v begin select 1; end;
create trigger bad2 instead of insert on t begin select 1; end;
create trigger bad3 after insert on nosuch begin select 1; end;
create trigger bad4 after insert on t begin insert into t values (1) returning *; end;
select raise(abort, 'outside');
-- A body referencing a missing table fails when it fires.
create trigger late after insert on base begin insert into missing values (1); end;
insert into base(name) values ('boom');
drop trigger late;
insert into base(name) values ('fine');
-- DROP TRIGGER, and dropping a table or view takes its triggers along.
drop trigger t2;
drop trigger nosuch;
drop trigger if exists nosuch;
drop view v;
drop table kv;
select name from sqlite_schema where type = 'trigger' order by name;
pragma integrity_check;
