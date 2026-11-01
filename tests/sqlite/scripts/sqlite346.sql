-- requires: 3.46.0
-- Features added in SQLite 3.46: digit separators and more strftime fields.
select 1_000_000, 1_0.2_5;
select strftime('%u %U', '2024-07-04'), strftime('%V %G %g', '2021-01-01'), strftime('%V %G', '2020-12-31');
