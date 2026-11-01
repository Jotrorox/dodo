---
title: "SQLite databases"
description: "Store data in SQL database files that SQLite can read and write too, with a database engine written entirely in Dodo."
section: "Standard library"
order: 159.8
---

`std/db/sqlite` is a database engine for SQLite's file format and SQL
dialect, written entirely in Dodo. It links no native library: the
same `.db` file can be opened by Dodo programs, by the `sqlite3` command-line
tool, and by any other program built on SQLite, even at the same time. Like
the rest of the standard library, it works inside storage you provide and
never allocates.

Two small packages sit next to it:

| Import | Contents |
| --- | --- |
| `std/db` | Shared vocabulary for every database driver: `db.Error` (portable `db.ErrorKind`, `db.Constraint`, and the driver's native code), the borrowed `db.Value`, `db.IsolationLevel`, and the connection pool `db.Pool` (see [PostgreSQL](postgres.md#connection-pools)). |
| `std/db/sql` | Interfaces that every SQL driver matches: `sql.Conn`, `sql.Row`, and `sql.RowSink`, the ready-made sinks `sql.Count` and `sql.ScalarInt`, and the value conversions behind `@derive(Row)`. |
| `std/db/sqlite` | The SQLite driver: `sqlite.Connection`, prepared statements, row cursors. |

## Quickstart

```dodo test
package notes

import "std/db"
import "std/db/sqlite"

static mut WORKSPACE: [sqlite.WORKSPACE_BYTES]u8 = [0u8; sqlite.WORKSPACE_BYTES]

fn main() -> i32 {
    // ":memory:" keeps the database in the workspace; pass a file path to
    // keep it on disk.
    conn := sqlite.Connection.open_memory(unsafe { &mut WORKSPACE[..] })!
    conn.exec("create table notes(id integer primary key, title text, stars int)")!

    params := [db.Value.Text("buy milk"), db.Value.Int(3)]
    conn.execute("insert into notes(title, stars) values (?, ?)", &params)!
    conn.exec("insert into notes(title, stars) values ('call home', 5)")!

    none_: [0]db.Value = []
    rows := conn.select("select title, stars from notes order by stars desc", &none_)!
    let some(first) = rows.next()! else {
        return 1
    }
    assert_eq(first.text(0), "call home")
    assert_eq(first.int(1), 5i64)
    return 0
}
```

`Connection.open(path, workspace)` opens or creates a database file instead;
`conn.close()` closes it (dropping the connection does the same, ignoring
errors). Every value read from a row borrows the workspace and stays valid
until the cursor moves on.

## The workspace

A connection keeps everything in the byte slice you pass to `open`: the page
cache, the parsed statements, the values of the current row, and the space for
sorting and grouping. `sqlite.WORKSPACE_BYTES` (2 MiB) suits most programs;
the minimum is `sqlite.MIN_WORKSPACE_BYTES`. Larger workspaces cache more
pages and can sort, group, and materialize larger results. Put a large
workspace in a `static mut` array rather than on the stack.

When a fixed region is too small for a request, for example a single value
larger than the value area or an `ORDER BY` over more rows than the sorter
holds, the statement fails with `db.ErrorKind.Capacity`, rolls back, and
`conn.message()` names the region. Nothing is ever spilled to temporary
files.

## Running statements

| Call | Use |
| --- | --- |
| `conn.exec(sql)` | Run one or more statements, discarding any rows. |
| `conn.execute(sql, &params)` | Run one statement; returns the number of changed rows. |
| `conn.select(sql, &params)` | Run one query and get a `Rows` cursor. |
| `conn.query(sql, &params, &mut sink)` | Run one query and pass every row to an `sql.RowSink`. |
| `conn.prepare(sql)` | Parse once; then `conn.run(&stmt, &params)` or `conn.rows(&stmt, &params)` repeatedly. |
| `conn.finalize(stmt)` | Release a prepared statement's slot. |
| `conn.statement_length(sql)` | Length of the first statement of a script, for splitting scripts yourself. |

Parameters use SQLite's syntax: `?` (numbered left to right), `?NNN`,
`:name`, `@name`, and `$name`. `params[i]` binds parameter `i + 1`, unbound
parameters are `NULL`, and `conn.parameter_index(&stmt, ":name")` finds a
named parameter's number. Text and blob parameters are copied, so the slice
only has to live for the call.

Arrays and slices you pass must be bound to a local first: write
`params := [db.Value.Int(1)]` and pass `&params`, and use an empty
`none_: [0]db.Value = []` for statements without parameters.

Prepared statements live in `sqlite.STATEMENT_SLOTS` slots; preparing more
reuses the least recently used slot and invalidates that handle (`run` and
`rows` then fail with `db.ErrorKind.Misuse`). A statement is recompiled
automatically when the schema changes.

### Reading rows

`rows.next()` returns `ok(some(row))` for each row and `ok(none)` at the end.
Only one statement runs at a time: the `Rows` cursor borrows the connection,
and the statement completes when the cursor reaches the end or is dropped.

| `Row` method | Result |
| --- | --- |
| `len()`, `name(i)` | Column count and the column's name (alias, column name, or expression text). |
| `get(i)` | `db.Value`: `Null`, `Int`, `Real`, `Text`, or `Bytes` (text that is not valid UTF-8 comes back as `Bytes`). |
| `value_type(i)`, `is_null(i)` | The column's storage class. |
| `int(i)`, `real(i)` | The value converted with SQLite's rules (`NULL` is 0). |
| `text(i)`, `bytes(i)` | Borrowed text or blob contents (empty for other types). |
| `text_into(i, &mut buf)` | Any value formatted as SQLite would print it, including reals. |

`conn.changes()`, `conn.total_changes()`, and `conn.last_insert_rowid()`
report the effects of the last statements, as their SQLite counterparts do.

## Mapping rows to structs

`@derive(Row)` generates the code that turns a result row into a struct and
a struct into statement parameters. It works with every SQL driver, because
the generated methods only use `std/db/sql`:

```dodo test
package notes

import "std/db"
import "std/db/sqlite"

static mut WORKSPACE: [sqlite.WORKSPACE_BYTES]u8 = [0u8; sqlite.WORKSPACE_BYTES]

@derive(Row)
struct Note {
    id: i64
    title: &str
    @db_name("stars")
    rating: u8
    // NULL becomes none.
    due: Option<&str>
}

fn main() -> i32 {
    conn := sqlite.Connection.open_memory(unsafe { &mut WORKSPACE[..] })!
    conn.exec("create table notes(id integer primary key, title text, stars int, due text)")!

    note := Note { id: 1, title: "buy milk", rating: 3, due: none }
    params := note.params()
    conn.execute("insert into notes(id, title, stars, due) values (?, ?, ?, ?)", &params)!

    none_: [0]db.Value = []
    rows := conn.select("select * from notes", &none_)!
    for {
        let some(row) = rows.next()! else {
            break
        }
        found := Note.decode_row(&row)!
        assert_eq(found.title, "buy milk")
        assert_eq(found.rating, 3u8)
    }
    return 0
}
```

The derive adds two ordinary, type- and borrow-checked methods:

- `decode_row<R: sql.Row>(row: &R) -> Self!db.Error` finds each field's
  column by name (exact, case-sensitive; `@db_name("column")` overrides the
  field name) and converts its value. Column order does not matter, and extra
  columns are ignored. When the statement lists columns in field order, each
  field costs a single name comparison. `R` can be a driver's row type or
  `dyn sql.Row`, so a `sql.RowSink` can decode its rows too.
- `params(&self) -> [N]db.Value` returns the fields in declaration order,
  ready to pass to `execute`, `run`, or `select`.

`&str` and `&[u8]` fields borrow the row: decoding copies nothing, and the
compiler rejects code that keeps a decoded struct after the cursor moves on
or after the workspace goes away. Copy what you need to keep into storage of
your own.

| Field type | Column value | Parameter |
| --- | --- | --- |
| `i64`, `isize`, `i8` to `i32`, `u8` to `u32` | Integer within the field's range | `Int` |
| `bool` | Integer; nonzero is `true` | `Int` 0 or 1 |
| `f64`, `f32` | Real or integer (for `f32`, finite and in range) | `Real` |
| `&str` | Text | `Text` |
| `&[u8]` | Blob, or the bytes of a text value | `Bytes` |
| `Option<T>` | `NULL` gives `none`, anything else decodes as `T` | `Null` for `none` |
| A struct with `decode_value`/`encode_value` | Whatever that struct accepts | `encode_value()` |

`u64` and `usize` are rejected at compile time, because SQL integers are
signed 64-bit values. A missing column is a `db.ErrorKind.Schema` error. A
`NULL` in a non-optional field, a value of the wrong type, or a number
outside the field's range is a `db.ErrorKind.Type` error, never a trap.

For column types of your own, such as money or timestamps, write a struct
with these two methods and use it as a field type:

```dodo
pub struct Cents {
    pub amount: i64

    pub fn decode_value(value: db.Value) -> Self!db.Error {
        ok(Cents { amount: sql.int(value)? })
    }

    pub fn encode_value(&self) -> db.Value {
        db.Value.Int(self.amount)
    }
}
```

Mark a field `@db_flatten` to read a nested `@derive(Row)` struct from the
same row. This is useful for joins: `select users.*, teams.name as team ...`
can fill `struct Member { @db_flatten user: User, team: &str }`. A struct with
flattened fields only gets `decode_row`, because the nested struct's
parameter count is not known where the outer struct is declared.

The conversions are plain functions in `std/db/sql` (`sql.column`,
`sql.int`, `sql.int_in`, `sql.real`, `sql.real32`, `sql.boolean`, `sql.text`,
`sql.bytes`, `sql.is_null`), so a hand-written `decode_row` follows the same
rules as a derived one:

```dodo
fn decode_row<R: sql.Row>(row: &R) -> Self!db.Error {
    count := sql.int(row.get(sql.column(row, "count", 0)?)?)?
    ok(Totals { count })
}
```

## Transactions

Outside a transaction every statement commits on its own. Wrap several
statements in `BEGIN`/`COMMIT` (or `conn.begin(level)`, `conn.commit()`,
`conn.rollback()`) to make them atomic; that is also much faster for many
inserts, because each commit synchronizes the file.

```dodo
conn.exec("begin")?
insert := conn.prepare("insert into notes(title) values (?)")?
for &title in titles {
    p := [db.Value.Text(title)]
    conn.run(&insert, &p)?
}
conn.exec("commit")?
```

As in SQLite, a statement that fails inside a transaction undoes only its own
changes, and the transaction stays open. (A statement that changes more pages
than the statement-undo area of the workspace holds rolls back the whole
transaction instead; `conn.in_transaction()` then reports `false`.) `BEGIN IMMEDIATE` (and
`conn.begin(db.IsolationLevel.Serializable)`) takes the write lock at once,
so a later write cannot fail because another process holds it.

When another process holds a conflicting lock, the operation fails with
`db.ErrorKind.Busy` ("database is locked"). `Options.busy_timeout_ms`,
`conn.set_busy_timeout(ms)`, or `PRAGMA busy_timeout = ms` make the
connection wait and retry for up to that long first.

## Errors

Failures return `db.Error`. Its `kind` is portable (`Syntax`, `Constraint`,
`Busy`, `ReadOnly`, `Capacity`, `Corrupt`, `Io`, ...), `constraint` says which
constraint rejected a change (`Unique`, `PrimaryKey`, `NotNull`, `Check`),
`code` is SQLite's extended result code, and `position` is the byte offset of
a syntax error. `conn.message()` returns SQLite's wording:

```dodo
match conn.exec("insert into users(email) values ('a@example.com')") {
    ok() => {},
    err(e) => {
        if e.constraint == db.Constraint.Unique {
            // "UNIQUE constraint failed: users.email"
            console.println(conn.message())?
        }
    },
}
```

## Code that works with any SQL driver

`std/db/sql` describes connections and rows with interfaces, so code written
against them works with this driver and with future ones. Generic code is
specialized per driver at compile time; `&mut dyn sql.Conn` selects the driver
at run time.

```dodo
fn count<C: sql.Conn>(conn: &mut C, table_sql: &str) -> i64!db.Error {
    result := sql.ScalarInt.new()
    none_: [0]db.Value = []
    conn.query(table_sql, &none_, &mut result)?
    ok(result.value)
}
```

A sink is any value with `pub fn row(&mut self, row: &dyn sql.Row) ->
void!db.Error`; the row and its borrowed values are valid during that call.

## Opening options

`sqlite.Options` configures `Connection.open_with(path, workspace, &options)`:

| Field | Default | Meaning |
| --- | --- | --- |
| `read_only` | `false` | Open without write access; writes fail with `db.ErrorKind.ReadOnly`. |
| `create` | `true` | Create the file if it does not exist. |
| `busy_timeout_ms` | `0` | How long to wait for other processes' locks before reporting `Busy`. |
| `page_size` | `4096` | Page size of a new database file (512 to 65536). |

## Compatibility

The engine implements SQLite's documented file format and semantics. The
test suite runs the same SQL scripts through SQLite and through this driver,
compares every result row, checks the files Dodo writes with SQLite's
`PRAGMA integrity_check`, and reads SQLite-written files back through Dodo.
It also covers crash recovery in both directions (each recovers the other's
interrupted transaction from its hot journal) and the lock protocol between
the two.

**Files.** All page sizes; overflow pages; the freelist; rollback-journal
transactions with hot-journal recovery; SQLite's lock bytes through
[`fs.File.try_lock_range`](filesystem.md); UTF-8 databases; schema formats 1
to 4.

**SQL.** `CREATE`/`DROP` `TABLE`, `INDEX` (including `UNIQUE` and the
automatic indexes behind `PRIMARY KEY` and `UNIQUE` constraints), and `VIEW`;
`ALTER TABLE` `RENAME TO`, `RENAME COLUMN`, and `ADD COLUMN`; `INSERT`
(including `INSERT ... SELECT`, `DEFAULT VALUES`, `OR IGNORE`/`REPLACE`/...,
and UPSERT), `UPDATE`, `DELETE`, each with `RETURNING`; `SELECT` with joins
(inner, `LEFT`, `CROSS`, `USING`), subqueries (scalar, `IN`, `EXISTS`, in
`FROM`), compound selects (`UNION [ALL]`, `INTERSECT`, `EXCEPT`), `WITH` and
`WITH RECURSIVE`, `DISTINCT`, aggregates, `GROUP BY`/`HAVING`,
`ORDER BY ... NULLS FIRST/LAST`, and `LIMIT`/`OFFSET`; `BEGIN`
(`DEFERRED`/`IMMEDIATE`/`EXCLUSIVE`), `COMMIT`, `ROLLBACK`; constraints
`NOT NULL`, `UNIQUE`, `PRIMARY KEY`, `CHECK`, `DEFAULT`, `COLLATE`,
`AUTOINCREMENT`; `INTEGER PRIMARY KEY` rowid aliases; collations `BINARY`,
`NOCASE`, and `RTRIM`; type affinity and comparison rules as in SQLite.

**Functions.** The core scalar functions (`abs`, `coalesce`, `ifnull`, `iif`,
`instr`, `length`, `like`, `glob`, `lower`, `upper`, `ltrim`, `rtrim`, `trim`,
`max`, `min`, `nullif`, `printf`/`format`, `quote`, `random`, `randomblob`,
`replace`, `round`, `sign`, `substr`, `typeof`, `hex`, `unhex`, `unicode`,
`char`, `zeroblob`, `concat`, `concat_ws`, `octet_length`, `changes`,
`last_insert_rowid`, `total_changes`, `sqlite_version`, ...); the aggregates
`count`, `sum`, `total`, `avg`, `min`, `max`, `group_concat`/`string_agg`
(with `DISTINCT`); and the date and time functions `date`, `time`,
`datetime`, `julianday`, `unixepoch`, `strftime` with their modifiers.

**Pragmas.** `user_version`, `application_id`, `schema_version`,
`page_size`, `page_count`, `freelist_count`, `encoding`, `journal_mode`,
`table_info`, `table_list`, `index_list`, `index_info`, `integrity_check`,
`quick_check`, `busy_timeout`, `database_list`; others are accepted and
ignored, as SQLite does for unknown pragmas.

**Not supported.** WAL-mode databases (convert them with `PRAGMA
journal_mode=DELETE` in SQLite first), triggers (tables with triggers can be
read but not written), window functions, `WITHOUT ROWID` and virtual tables,
`ATTACH`, savepoints, `ALTER TABLE DROP COLUMN`, writes to tables with
expression or partial indexes, UTF-16 databases, temporary tables, and
time zones (`localtime` and `utc` leave times in UTC). Reals are printed
with SQLite 3.47's rule (the shortest of 15 or 17 significant digits that
round-trips); the 17th digit can occasionally differ from SQLite's own
conversion.

**Concurrency.** Locks follow SQLite's protocol, so Dodo and SQLite processes
can use one file safely. As with SQLite's POSIX locking, two connections to
the same file inside one process interfere: closing either releases the
other's locks. Open one connection per file per process.

## Complete API reference

For every public type and signature, see [std/db](api/std/db.md),
[std/db/sql](api/std/db/sql.md), and [std/db/sqlite](api/std/db/sqlite.md).
