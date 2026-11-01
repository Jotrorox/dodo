-- ALTER TABLE RENAME rewrites references in views, triggers and foreign
-- keys (legacy_alter_table off, SQLite's default).
pragma legacy_alter_table = off;
pragma legacy_alter_table;
create table p(a, b, c);
create table q(x, y);
create table child(id, pid references p(a));
create trigger tr after insert on p begin insert into q values (new.a, new.c); end;
create trigger tq after update of y on q when new.y > 0 begin update p set b = new.y where a = old.x; end;
create view vp as select a, b, q.y from p join q on p.a = q.x where p.c is not null;
create view vq as select x, y from q, p where x = a;
alter table p rename column a to aa;
alter table q rename column x to xx;
alter table q rename to q2;
alter table p rename to p2;
select name, tbl_name, sql from sqlite_schema order by name;
insert into p2 values (1, 2, 3);
select * from q2;
update q2 set y = 9;
select * from p2;
select * from vp;
select * from vq;
-- Legacy mode only follows the table in triggers on it.
pragma legacy_alter_table = 1;
create table r(m);
create view vr as select m from r;
create trigger trr after insert on r begin insert into q2 values (new.m, 0); end;
alter table r rename to r2;
select name, tbl_name, sql from sqlite_schema where name in ('r2', 'vr', 'trr') order by name;
