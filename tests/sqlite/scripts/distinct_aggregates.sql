-- DISTINCT aggregates: equal values under the comparison rules count once.
create table v(g int, x, t text, n text collate nocase, r text collate rtrim, b blob);
insert into v values
  (1, 1, 'a', 'Abc', 'x', x'61'),
  (1, 1.0, 'A', 'abc', 'x ', x'61'),
  (1, 0, 'a ', 'ABC', 'x  ', x'41'),
  (1, -0.0, 'a', 'abd', 'y', x''),
  (2, 0.0, '', 'é', 'é ', null),
  (2, 9223372036854775807, 'é', 'É', 'é', x'c3a9'),
  (2, 9223372036854775808.0, 'É', 'é', '', x'c3a9'),
  (2, 9.223372036854775807e18, null, null, null, null),
  (3, -9223372036854775808, 'x', 'X', 'x', x'78'),
  (3, -9223372036854775808.0, 'x', 'x', 'x ', x'78'),
  (3, 1e300, '1', '1', '1', x'31'),
  (3, 1e300, 1, 1, 1, 1),
  (3, 0.5, '0.5', '0.5', '0.5', 0.5),
  (3, '0.5', 0.5, 0.5, 0.5, '0.5'),
  (3, 'abc', 'abc', 'ABC', 'abc ', x'616263'),
  (3, x'616263', x'616263', x'616263', x'616263', 'abc');
select count(distinct x), count(distinct t), count(distinct n), count(distinct r), count(distinct b) from v;
select g, count(distinct x), count(distinct t), count(distinct n), count(distinct r), count(distinct b) from v group by g order by g;
select count(distinct t collate nocase), count(distinct t collate rtrim), count(distinct n collate binary), count(distinct r collate binary) from v;
select g, sum(distinct x), total(distinct x), avg(distinct x) from v where typeof(x) in ('integer', 'real') and x between -1e10 and 1e10 group by g order by g;
select group_concat(distinct n) from v where g < 3;
select count(distinct x), count(distinct coalesce(t, 'z')) filter (where g > 1) from v;
-- Many values: the set grows through many splits, with groups resetting it.
create table w(g int, k int, s text);
with recursive c(i) as (select 0 union all select i + 1 from c where i < 4999)
insert into w select i % 7, (i * 37) % 2003, 'k' || ((i * 13) % 1777) from c;
select count(distinct k), count(distinct s), count(distinct k * 1.0), count(distinct upper(s)), sum(distinct k) from w;
select g, count(distinct k), count(distinct s), sum(distinct k % 100) from w group by g order by g;
select count(distinct k || 'x' || s) from w;
select count(*) from (select g from w group by g having count(distinct k) > 600);
