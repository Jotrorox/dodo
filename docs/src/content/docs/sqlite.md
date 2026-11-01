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
| `std/db` | Shared vocabulary for every database driver: `db.Error` (portable `db.ErrorKind`, `db.Constraint`, and the driver's native code), the borrowed `db.Value`, and `db.IsolationLevel`. |
| `std/db/sql` | Interfaces that every SQL driver matches: `sql.Conn`, `sql.Row`, and `sql.RowSink`, plus the ready-made sinks `sql.Count` and `sql.ScalarInt`. |
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
sorting and grouping. `sqlite.WORKSPACE_BYTES` (2 MiB) suits most programs.
The smallest workspace depends on the page size:
`sqlite.min_workspace_bytes(page_size)` returns it, and
`sqlite.MIN_WORKSPACE_BYTES` (424 KiB) covers SQLite's default 4096-byte
pages. Databases with 65536-byte pages fit in the default 2 MiB workspace.
Larger workspaces cache more pages and can sort, group, and materialize larger
results. Put a large workspace in a `static mut` array rather than on the
stack.

The regions are sized when the connection opens, from the database's page
size. `PRAGMA page_size` on a database that does not exist yet lays the
workspace out again for the new size before the next statement; prepared
statements are recompiled automatically.

When a region is too small for a request, for example a single value
larger than the value area or an `ORDER BY` over more rows than the sorter
holds, the statement fails with `db.ErrorKind.Capacity`, rolls back, and
`conn.message()` names the region. Query results are never spilled to
temporary files.

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
changes, and the transaction stays open. `BEGIN IMMEDIATE` (and
`conn.begin(db.IsolationLevel.Serializable)`) takes the write lock at once,
so a later write cannot fail because another process holds it.

`SAVEPOINT name`, `RELEASE [SAVEPOINT] name`, and `ROLLBACK TO [SAVEPOINT]
name` nest transactions as in SQLite: `ROLLBACK TO` undoes everything since
the savepoint and leaves it open, `RELEASE` keeps the changes, and a
`SAVEPOINT` outside `BEGIN` starts a transaction that the release of the
outermost savepoint commits. The page images that savepoints and failing
statements need for undo stay in the workspace; when a large transaction on
a file database outgrows that area, they continue in a `<database>-stmtjrnl`
file that is deleted when the transaction ends. (Without a savepoint, an
in-memory database that outgrows the area rolls back the whole transaction
when a statement fails; `conn.in_transaction()` then reports `false`.)

When another process holds a conflicting lock, the operation fails with
`db.ErrorKind.Busy` ("database is locked"). `Options.busy_timeout_ms`,
`conn.set_busy_timeout(ms)`, or `PRAGMA busy_timeout = ms` make the
connection wait and retry for up to that long first.

## Attached and temporary databases

`ATTACH 'file' AS name` opens another database file on the same connection
(up to ten, as in SQLite; `':memory:'` or `''` attaches an in-memory
database), and `DETACH name` closes it. Tables, indexes, views, and triggers
are then named `name.object`; an unqualified name means the TEMP database
first, then `main`, then the attached databases in attach order. `PRAGMA
database_list` lists them, and pragmas such as `integrity_check`,
`table_info`, `page_count`, or `journal_mode` accept a `name.` prefix
(unqualified `journal_mode = ...` applies to every database, as in SQLite).

```dodo
conn.exec("attach 'archive.db' as archive")?
conn.exec("insert into archive.orders select * from orders where year < 2020")?
conn.exec("detach archive")?
```

`CREATE TEMP TABLE` (and `TEMP VIEW`, `TEMP TRIGGER`, and indexes on temp
tables) put objects into the `temp` database, which is private to the
connection and disappears when it closes. Its pages stay in the workspace's
page cache, as with `temp_store = MEMORY`, so the workspace bounds its size.
A temp trigger may fire on a table of another database, and a temp object
may shadow one of the same name in `main`.

A transaction covers every database it writes. When it writes more than one
file in rollback-journal mode, the commit goes through a super-journal
(`<main>-mjXXXXXXXX`) exactly as in SQLite, so after a crash either every
file has the transaction or none does, and Dodo and SQLite each recover the
other's interrupted multi-file commits. (In WAL mode, as in SQLite, the
commit is atomic per file only.) Statements lock only the databases they
use. Attached databases must use the main database's text encoding; a new
attached file takes it.

## WAL mode

`PRAGMA journal_mode = WAL` switches a database file to write-ahead logging,
exactly as SQLite does (the setting is stored in the file, so it stays on for
every later connection, Dodo or SQLite). Commits then append the changed
pages to `<file>-wal` instead of rewriting the database, and readers keep
reading a consistent snapshot while a writer commits, so readers and the
single writer no longer block each other.

```dodo
conn := sqlite.Connection.open("app.db", unsafe { &mut WORKSPACE[..] })?
conn.exec("pragma journal_mode = wal")?
```

A checkpoint copies committed pages from the log back into the database
file. One runs automatically after a commit leaves 1000 or more frames in
the log (`PRAGMA wal_autocheckpoint = N` changes the threshold; 0 turns it
off), and when the last connection to the file closes; that connection then
deletes the `-wal` and `-shm` files. `PRAGMA wal_checkpoint` runs one now and
returns SQLite's three columns: `busy` (1 if it could not finish because of
other connections), `log` (frames in the log) and `checkpointed` (frames
copied so far). The argument selects SQLite's modes: `PASSIVE` (default,
never waits), `FULL` (waits for the writer and for readers of older
snapshots, up to the busy timeout), `RESTART` (as `FULL`, then waits until
the next writer can start the log over) and `TRUNCATE` (as `RESTART`, and
truncates the log file). `PRAGMA journal_mode = DELETE` returns to rollback
journaling; it needs every other connection to have closed the file and
fails with `Busy` otherwise. Neither conversion, nor a checkpoint, can run
inside a transaction.

In-memory databases ignore the setting and report `memory`, as in SQLite.

Everything follows SQLite's documented formats, so Dodo and SQLite processes
can work on one WAL database at the same time: the log (frames, salts, and
checksums in either byte order), the shared `-shm` index with its hash
tables and read marks, and SQLite's WAL locks on it. Without memory-mapped
files, the engine reads and writes the index with ordinary file I/O and
relies on its double header copy, checksums, and retry rules. A connection
that finds the index stale or damaged (for example after a crash) rebuilds it
from the log, keeping only committed transactions.

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
interrupted transaction from its hot journal or its write-ahead log), the
lock protocol between the two, and Dodo and SQLite processes reading,
writing, and checkpointing one WAL database concurrently.

**Files.** All page sizes; overflow pages; the freelist; rollback-journal
transactions with hot-journal recovery; WAL mode with its `-wal` log and
`-shm` index; SQLite's lock bytes through
[`fs.File.try_lock_range`](filesystem.md); UTF-8, UTF-16le, and UTF-16be
databases; schema formats 1 to 4.

**SQL.** `CREATE`/`DROP` `TABLE` (including `WITHOUT ROWID` tables),
`INDEX` (including `UNIQUE`, indexes on expressions, partial indexes with
`WHERE`, and the automatic indexes behind `PRIMARY KEY` and `UNIQUE`
constraints), `VIEW`, and `TRIGGER` (`BEFORE`, `AFTER`, and `INSTEAD OF` on
views; `INSERT`, `DELETE`, `UPDATE [OF columns]`; `WHEN`; `RAISE`; nested
triggers, with recursive firing off as in SQLite's default);
`ALTER TABLE` `RENAME TO`, `RENAME COLUMN`, `ADD COLUMN`, and `DROP COLUMN`
(references in views, triggers, and foreign keys follow renames, as with
SQLite's default `legacy_alter_table = OFF`); `INSERT`
(including `INSERT ... SELECT`, `DEFAULT VALUES`, `OR IGNORE`/`REPLACE`/...,
and UPSERT), `UPDATE`, `DELETE`, each with `RETURNING`; `SELECT` with joins
(inner, `LEFT`, `CROSS`, `USING`), subqueries (scalar, `IN`, `EXISTS`, in
`FROM`), compound selects (`UNION [ALL]`, `INTERSECT`, `EXCEPT`), `WITH` and
`WITH RECURSIVE`, `DISTINCT`, aggregates, `GROUP BY`/`HAVING`,
`ORDER BY ... NULLS FIRST/LAST`, and `LIMIT`/`OFFSET`; `BEGIN`
(`DEFERRED`/`IMMEDIATE`/`EXCLUSIVE`), `COMMIT`, `ROLLBACK`, `SAVEPOINT`,
`RELEASE`, `ROLLBACK TO`; constraints
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
(with `DISTINCT` and `FILTER (WHERE ...)`); and the date and time functions `date`, `time`,
`datetime`, `julianday`, `unixepoch`, `strftime` with their modifiers. The
`localtime` and `utc` modifiers use the operating system's time zone rules
(`localtime_r`, or `_localtime64_s` on Windows), as SQLite does, including
its handling of times in a daylight-saving gap or overlap and its mapping of
years outside 1970 to 2037 onto a year with the same leap-year position.

**Reals** print and parse exactly as in SQLite 3.53 and later: `CAST` to
text (`printf('%!.17g')`: 17 significant digits, fewer where they end in a
run of 9s or 0s and the shorter form reads back), `printf`'s `%f`, `%e`, `%g` (with the `!` and
`#` flags), `quote`, `round`, and text-to-real conversion all reproduce
SQLite's own decimal rounding digit for digit, not just correctly rounded
values. Older SQLite versions round the last digit differently in places.

**Text encodings.** `PRAGMA encoding = 'UTF-16le'` (or `'UTF-16be'`, or
`'UTF-16'` for the native byte order) before a new database's first table
creates a UTF-16 database; `PRAGMA encoding` reports the encoding of the
database, and an existing database keeps its own. Text in records and in
the schema is stored in the database's encoding, and as in SQLite the
`BINARY` collation then orders text by its UTF-16 bytes (which differs from
UTF-8 order for non-ASCII text), while `NOCASE` and `RTRIM` compare UTF-8.
`hex(text)`, `CAST(text AS BLOB)`, `octet_length`, and blob-to-text
conversions see UTF-16 bytes; conversions handle invalid sequences as SQLite
does. The API binds and returns text as UTF-8 for every encoding. One
difference remains: SQLite keeps the bytes of a blob cast to text in a UTF-16
database unchanged, while Dodo converts them, so an invalid UTF-16 sequence
stored that way (an unpaired surrogate or an odd byte count) is normalized
rather than kept byte for byte.

**Window functions.** `fn(...) [FILTER (WHERE ...)] OVER (...)` and
`OVER name` with `WINDOW name AS (...)` clauses (a definition may build on an
earlier one); `PARTITION BY`, `ORDER BY` (with `NULLS FIRST/LAST` and
collations), and `ROWS`, `RANGE` (numeric offsets on one `ORDER BY` term), or
`GROUPS` frames with `UNBOUNDED PRECEDING`, `n PRECEDING`, `CURRENT ROW`,
`n FOLLOWING`, `UNBOUNDED FOLLOWING` and `EXCLUDE NO OTHERS`/`CURRENT
ROW`/`GROUP`/`TIES`. The functions are `row_number`, `rank`, `dense_rank`,
`percent_rank`, `cume_dist`, `ntile`, `lag`, `lead`, `first_value`,
`last_value`, `nth_value`, and every aggregate above (without `DISTINCT`).
Windows run over the rows left after `WHERE`, `GROUP BY` and `HAVING`
(aggregates may appear in window arguments), may be used in `ORDER BY`, and
combine with `DISTINCT`, `LIMIT`, compound selects, subqueries, views, and
CTEs. Default frames, error messages, row order, and the results of sliding
frames (including SQLite's incremental `sum` and `group_concat` behaviour)
follow SQLite. A window query materializes its rows in the sorter, so the
workspace bounds its size.

**Pragmas.** `user_version`, `application_id`, `schema_version`,
`page_size`, `page_count`, `freelist_count`, `encoding`, `journal_mode`
(`DELETE` and `WAL`; other rollback modes behave as `DELETE`),
`wal_checkpoint`, `wal_autocheckpoint`, `table_info`, `table_list`, `index_list`, `index_info`, `integrity_check`,
`quick_check`, `busy_timeout`, `database_list`, `legacy_alter_table`; others
are accepted and
ignored, as SQLite does for unknown pragmas.

**Not supported.** Virtual tables.

**Concurrency.** Locks follow SQLite's protocol, so Dodo and SQLite processes
can use one file safely. As with SQLite's POSIX locking, two connections to
the same file inside one process interfere: closing either releases the
other's locks. Open one connection per file per process. (In WAL mode the
engine at least keeps a second connection of the same process from resetting
the `-shm` index or deleting the log under the first.) The WAL index is
written in the host's byte order, as SQLite does, so it is shared only
between processes on the same machine.

## Complete API reference

For every public type and signature, see [std/db](api/std/db.md),
[std/db/sql](api/std/db/sql.md), and [std/db/sqlite](api/std/db/sqlite.md).
