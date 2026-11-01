-- requires: 3.46.0
-- From SQLite 3.46 on, 'localtime' on a time already in local time and 'utc'
-- on one already in UTC (such as 'now') are no-ops; older versions convert
-- again. Run with localtime.sql under several TZ
-- settings by tests/sqlite_library.rs.
select datetime('2024-07-01 12:00:00', 'localtime', 'localtime');
select datetime(-1781073241, 'unixepoch', 'localtime', 'localtime', '+1 hour');
select date('2325-06-27 11:37:24.438', 'localtime', 'start of month', '+1 month', 'localtime');
select strftime('%Y-%m-%d %H:%M:%f %j %w %s', '1937-07-07 12:30:12', 'localtime', 'localtime', 'subsec');
select julianday(-1721036903, 'unixepoch', '+1 hour', 'localtime', 'localtime');
select strftime('%Y-%m-%d %H:%M:%f %j %w %s', '9694-06-05 21:02:19', 'localtime', 'localtime');
select unixepoch('-0016-09-14 11:03:24', 'localtime', '+1 hour', 'localtime');
select datetime('now', 'utc') = datetime('now');
