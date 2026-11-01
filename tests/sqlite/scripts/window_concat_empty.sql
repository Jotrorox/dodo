-- requires: 3.54.1
-- A sliding group_concat over only empty strings is '' in SQLite 3.54.1,
-- a one-byte NUL string in 3.54.0 and NULL in older versions (3.45 here).
create table n(id integer primary key, v);
insert into n(v) values (1), (2), (0.5), (3), (4), ('7'), ('x'), (null), (5), (9223372036854775806), (2), (-9223372036854775807), (10);
select id, group_concat(case when id % 3 = 0 then '' else v end) over (order by id rows between 1 preceding and current row) from n order by id;
