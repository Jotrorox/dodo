-- requires: 3.54.1
-- From SQLite 3.54.1 on, U+FFFE and U+FFFF survive conversion to UTF-16
-- (older versions replace them with U+FFFD); surrogates still do not.
pragma encoding = 'UTF-16le';
create table t(s);
insert into t values (char(97, 65534, 98, 65535, 99)), (char(55296)), ('a' || char(65534));
select hex(s), length(s), unicode(substr(s, 2)) from t order by rowid;
select hex(char(65534)), hex(char(65535)), hex(char(55296)), hex(char(1114111));
