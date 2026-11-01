-- requires: 3.53.0
-- Conversions SQLite 3.53 changed with its new decimal conversion: quote()
-- of infinities and %.0f of halves (rounded away from zero).
select quote(1e308 * 10), quote(-1e308 * 10);
select printf('%.0f %.0f %.0f %.0f', 0.5, 1.5, 2.5, -0.5);
