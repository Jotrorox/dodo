-- requires: 3.51.0
-- A +00:00 or -00:00 suffix marks a time as UTC (SQLite 3.51), so a later
-- 'utc' leaves it alone. Run like localtime.sql.
select datetime('2024-07-01 12:00:00', 'utc', 'utc'), datetime('2024-07-01 12:00:00Z', 'utc'), datetime('2024-07-01 12:00:00+05:00', 'utc'), datetime('2024-07-01 12:00:00-00:00', 'utc');
select datetime('2025-01-04 05:32:19+00:00', 'weekday 0');
select strftime('%Y-%m-%d %H:%M:%f %j %w %s', '0045-04-06 12:28:34+00:00', '+1 hour');
select unixepoch('1922-10-05 03:40:47+00:00', 'utc', 'localtime', 'subsec');
select strftime('%Y-%m-%d %H:%M:%f %j %w %s', '4217-06-11 18:07:22+00:00', '+3600 seconds', 'start of month', '+0 days', 'utc');
