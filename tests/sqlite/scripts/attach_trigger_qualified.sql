-- requires: 3.54.0
-- TEMP trigger bodies may name tables of attached databases with their
-- schema; older SQLite rejects qualified names inside triggers.
attach 'aux.db' as aux;
create table aux.a(id integer primary key, n);
create table aux.audit(what);
create table audit(what);
create temp trigger ta after insert on aux.a begin insert into aux.audit values ('ins ' || new.id); end;
create temp trigger tb after update of n on aux.a begin update aux.audit set what = what || '!' where what = 'ins ' || old.id; end;
create temp trigger tc before delete on aux.a begin delete from aux.audit where what like 'ins ' || old.id || '%'; insert into main.audit values ('del ' || old.id); end;
insert into aux.a values (1, 10), (2, 20);
update aux.a set n = n + 1 where id = 2;
delete from aux.a where id = 1;
select * from aux.audit order by rowid;
select * from main.audit order by rowid;
select type, name, tbl_name, sql from sqlite_temp_master order by name;
detach aux;
