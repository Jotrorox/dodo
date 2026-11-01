---
title: "PostgreSQL"
description: "Connect to PostgreSQL servers with TLS and SCRAM authentication, run statements with cached server-side plans, map rows to structs, and pool connections."
section: "Standard library"
order: 159.85
---

`std/db/postgres` is a PostgreSQL client written in Dodo. It speaks the
PostgreSQL wire protocol (version 3.0, every server since 7.4) over
[`std/net`](networking.md), encrypts with [`std/tls`](tls.md), and works
inside a workspace you provide, like the rest of the standard library. It
links OpenSSL (for TLS and for the password hashes), and no PostgreSQL client
library.

| Import | Contents |
| --- | --- |
| `std/db/postgres` | The client: `postgres.connect`, `Connection`, `Config`, `Rows`, `Row`, `Statement`, `postgres.cancel`. |
| `std/db/postgres/protocol` | The wire protocol without I/O: message encoding and parsing, value conversion, SQLSTATE mapping, SCRAM-SHA-256. It imports no sockets, TLS or cryptography backend, so it builds for any target. |
| `std/db` | Shared vocabulary: `db.Error`, `db.Value`, `db.IsolationLevel`, and `db.Pool`. |
| `std/db/sql` | `sql.Conn`, `sql.Row`, `sql.RowSink`, shared by every SQL driver (see [SQLite](sqlite.md#code-that-works-with-any-sql-driver)). |

## Quickstart

```dodo
package notes

import "std/console"
import "std/db"
import "std/db/postgres"

static mut WORKSPACE: [postgres.WORKSPACE_BYTES]u8 = [0u8; postgres.WORKSPACE_BYTES]

fn main() -> i32 {
    conn := postgres.connect(b"postgres://app:secret@localhost/app", unsafe { &mut WORKSPACE[..] })!
    conn.exec("create table if not exists notes(id serial primary key, title text not null, stars int)")!

    params := [db.Value.Text("buy milk"), db.Value.Int(3)]
    conn.execute("insert into notes(title, stars) values ($1, $2)", &params)!

    min := [db.Value.Int(2)]
    rows := conn.select("select title, stars from notes where stars >= $1 order by stars desc", &min)!
    for {
        let some(row) = rows.next()! else {
            break
        }
        console.println(row.text(0))!
    }
    return 0
}
```

Values read from a row borrow the workspace and stay valid until the cursor
moves on. Drop the connection or call `conn.close()` to end the session.

## Connecting

`postgres.connect(url, workspace)` parses a libpq connection URL and connects:

```
postgres://user:password@host:port/database?sslmode=verify-full&application_name=api
```

`postgresql://` works too. The host defaults to `localhost`, the port to 5432,
and the database to the user's name. Escape special characters in the user
name or password (`%40` for `@`). Recognized query parameters are `sslmode`,
`application_name`, `connect_timeout` (seconds), and `host`, `port`, `user`,
`password`, `dbname`. Other parameters are ignored, so URLs written for other
tools still work. Unix-domain sockets are not supported.

For more control, build a `postgres.Config` and connect with
`Connection.connect(config, workspace)`:

```dodo
storage := [0u8; 256]
config := postgres.Config.parse(url, &mut storage)?
config.timeout_ms = 5000
config.trust_pem = company_root_pem
conn := postgres.Connection.connect(config, workspace)?
```

| `Config` field | Default | Meaning |
| --- | --- | --- |
| `host`, `port` | `localhost`, 5432 | Server address; names resolve to every address and each is tried. |
| `user`, `password` | — | Credentials. The password is only sent if the server asks for one. |
| `database` | the user name | Database to open. |
| `application_name` | empty | Shown in `pg_stat_activity`. |
| `ssl_mode` | `SslMode.Prefer` | When to use TLS; see below. |
| `trust_pem` | empty | PEM roots for `VerifyCa`/`VerifyFull`; empty uses the system roots. |
| `connect_timeout_ms` | 10000 | Resolution, connection, TLS and authentication together; 0 waits forever (also `connect_timeout=0` in a URL). |
| `timeout_ms` | 0 (none) | Each later wait for the server. |
| `statement_cache` | `true` | Keep prepared statements on the server; see [Statements](#statements). |

`Config.parse(url, storage)` decodes escaped URL parts into `storage` (at most
`url.len` bytes). `config.clone()` copies a configuration, for example one per
pool slot.

### Authentication

The client answers whatever the server's `pg_hba.conf` asks for:
SCRAM-SHA-256 (the default since PostgreSQL 14), md5, a cleartext password,
or no password (`trust`). SCRAM proves the password without revealing it and
also checks that the server knows it; the client refuses a server that ends
SCRAM without that proof, or that switches to another method midway. Kerberos, GSSAPI, SSPI, certificate and OAuth
authentication, and SCRAM channel binding, are not supported.

A rejected login fails with `db.ErrorKind.Auth`; `conn.message()` gives the
server's reason (SQLSTATE `28P01` for a wrong password).

### TLS

`ssl_mode` follows libpq's `sslmode`:

| Mode | URL value | Behavior |
| --- | --- | --- |
| `SslMode.Disable` | `disable` | Never use TLS. |
| `SslMode.Prefer` | `prefer` (default), `allow` | Use TLS if the server offers it, without checking its certificate; otherwise connect unencrypted. |
| `SslMode.Require` | `require` | Require TLS, without checking the certificate (with `trust_pem` set, the chain is checked, as libpq does with a root certificate). |
| `SslMode.VerifyCa` | `verify-ca` | Require TLS and a certificate that chains to a trusted root. |
| `SslMode.VerifyFull` | `verify-full` | Also require the certificate to name the host. |

Only `VerifyFull` protects against an attacker who can intercept the
connection; use it whenever the network is not trusted. `Prefer` and
`Require` only protect against passive eavesdropping, as in libpq. A failed
certificate check fails with `db.ErrorKind.Auth`.

### The workspace

A connection keeps everything in the workspace: TLS buffers, a send buffer,
the receive buffer, the statement cache, the current result's column names,
queued notifications, and the last error message.
`postgres.WORKSPACE_BYTES` (256 KiB) suits most programs; the minimum is
`postgres.MIN_WORKSPACE_BYTES` (64 KiB). Everything beyond the fixed parts
becomes the receive buffer, which bounds the largest row: a row that does not
fit fails with `db.ErrorKind.Capacity`, and the connection stays usable.
Parameters of any size are streamed to the server. Put a large workspace in a
`static mut` array rather than on the stack.

## Running statements

The methods mirror the [SQLite driver](sqlite.md#running-statements):

| Call | Use |
| --- | --- |
| `conn.exec(sql)` | Run one or more statements separated by `;`, without parameters, discarding rows. |
| `conn.execute(sql, &params)` | Run one statement; returns the number of rows it inserted, updated, deleted or returned. |
| `conn.select(sql, &params)` | Run one statement and get a `Rows` cursor. |
| `conn.query(sql, &params, &mut sink)` | Run one statement and pass every row to an `sql.RowSink`. |
| `conn.prepare(sql)` | Prepare now; then `conn.run(&stmt, &params)` or `conn.rows(&stmt, &params)`. |
| `conn.changes()` | Rows affected by the last statement. |

Parameters are written `$1`, `$2`, ... and `params[i]` binds `$(i + 1)`. The
number of parameters must match the statement (`db.ErrorKind.Range`
otherwise). Bind arrays to a local first, and use `none_: [0]db.Value = []`
for statements without parameters.

Each parameter is converted for the type the server inferred for it:

| `db.Value` | Accepted for |
| --- | --- |
| `Int` | Integer columns (the value must fit: `70000` for an `int2` is a `db.ErrorKind.Type` error, reported before the statement runs), `float4`/`float8`, `bool` (nonzero is true), and as decimal text for everything else (`numeric`, `text`, ...). |
| `Real` | `float4`/`float8`, integer columns when the value is integral, and as text for `numeric` and other types (the shortest digits that read back exactly). |
| `Text` | Any type; the server parses the text (`'2024-02-29'` for a `date`, `'{1,2}'` for an array). |
| `Bytes` | `bytea`, and as raw text for other types. |
| `Null` | Any type. |

A parameter whose type the server cannot infer, such as `select $1`, is text.
Add a cast (`$1::int8`) to choose another type.

### Reading rows

`rows.next()` returns `ok(some(row))` for each row and `ok(none)` at the end.
A statement that fails after producing some rows (a division by zero in row 3)
returns its rows first and then the error. One statement runs at a time: the
cursor borrows the connection. Dropping a cursor early is fine; the
connection skips the remaining rows before its next request.

Columns arrive as these values:

| PostgreSQL type | `db.Value` |
| --- | --- |
| `bool` | `Int` 0 or 1 |
| `int2`, `int4`, `int8`, `oid` | `Int` |
| `float4`, `float8` | `Real` |
| `bytea` | `Bytes` |
| everything else: `text`, `varchar`, `numeric`, `date`, `timestamp`, `timestamptz`, `uuid`, `json`, `jsonb`, arrays, ... | `Text`, in PostgreSQL's text format (`12.50`, `2024-02-29 12:34:56`) |
| `NULL` | `Null` |

`numeric` stays text so no digits are lost; parse it, or cast in SQL
(`amount::float8`). Timestamps can be read with
[`std/time/iso8601`](time.md).

| `Row` method | Result |
| --- | --- |
| `len()`, `name(i)` | Column count and name. |
| `get(i)` | The `db.Value`. |
| `is_null(i)`, `value_type(i)` | Whether the value is NULL, and its class. |
| `int(i)`, `real(i)` | Integers and reals, converted between each other; 0 for other values. |
| `text(i)`, `bytes(i)` | Text, or bytea / the bytes of text; empty for other values. |

`@derive(Row)` [maps rows to structs](sqlite.md#mapping-rows-to-structs) with
every SQL driver, PostgreSQL included:

```dodo
@derive(Row)
struct Note {
    id: i64
    title: &str
    stars: Option<i32>
}

none_: [0]db.Value = []
rows := conn.select("select id, title, stars from notes", &none_)?
for {
    let some(row) = rows.next()? else {
        break
    }
    note := Note.decode_row(&row)?
}
```

## Statements

PostgreSQL plans a statement once when it is prepared, and later executions
skip parsing and planning. The client does this automatically: `execute`,
`select` and `query` keep up to `postgres.STATEMENT_SLOTS` (32) statements
prepared on the server, keyed by their SQL text, and reuse the least recently
used slot when the cache is full. The first execution of a statement costs an
extra round trip to prepare and describe it; later ones cost one round trip
and use PostgreSQL's binary format for the types in the table above.

A cached statement survives schema changes: when the server reports that a
cached plan no longer matches (`select *` after `alter table`), or that the
statement is gone (`discard all`), the client prepares it again and retries
once, outside transactions. `conn.prepare(sql)` puts a statement into the
cache right away, so syntax errors surface early; the returned `Statement`
keeps working even after its slot is reused.

Set `config.statement_cache = false` for connection poolers that do not
support named prepared statements (PgBouncer before 1.21 in transaction
mode). Every statement then costs two round trips.

## Transactions

`conn.begin(level)`, `conn.commit()` and `conn.rollback()` (or `exec("BEGIN")`
and friends) work as in PostgreSQL. `db.IsolationLevel.Default` uses the
server's default (normally read committed); the other levels map to the
matching `BEGIN ISOLATION LEVEL`. `conn.in_transaction()` reports whether a
transaction block is open.

When a statement fails inside a transaction, PostgreSQL aborts the whole
transaction: later statements fail (`db.ErrorKind.Misuse`, SQLSTATE `25P02`)
until it ends. `commit()` of an aborted transaction makes the server roll back
instead, and the client reports that as `db.ErrorKind.Misuse` rather than
success. Serialization failures and deadlocks (`db.ErrorKind.Serialization`)
are worth retrying from `begin`:

```dodo
for attempt in 0..3 {
    match transfer(&mut conn) {
        ok() => {
            break
        },
        err(e) => {
            if !e.is_transient() || attempt == 2 {
                return err(e)
            }
            conn.rollback()?
        },
    }
}
```

## Errors

Failures return `db.Error`. `kind` is portable, `constraint` names the
constraint class, `code` is the SQLSTATE packed into a number
(`protocol.sqlstate_number(b"23505")`), and `position` is the byte offset of
a syntax error in the statement. The connection keeps the server's text:

| Method | Content |
| --- | --- |
| `conn.message()` | The primary message, or the client's reason for a local failure. |
| `conn.detail()`, `conn.hint()` | The server's detail and hint, if any. |
| `conn.sqlstate()` | The five-character SQLSTATE (`"23505"`), empty for client-side failures. |

| SQLSTATE | `db.ErrorKind` |
| --- | --- |
| `23505` / `23503` / `23502` / `23514` / other `23` | `Constraint` with `Unique` / `ForeignKey` / `NotNull` / `Check` / `Other` |
| `40001`, `40P01`, other `40` | `Serialization` |
| `28`, `42501` | `Auth` |
| `42601` and other syntax errors | `Syntax` |
| `42P01`, `42703`, `42883` and other missing or duplicate objects | `Schema` |
| `22`, `42804` | `Type` |
| `25006` / other `25` | `ReadOnly` / `Misuse` |
| `55P03` | `Busy` |
| `57014` (cancelled, statement timeout) | `Interrupted` |
| `08`, `3D000`, `57P01`-`57P03` | `Connect` |
| `53`, `54` | `Capacity` |
| `0A000` | `Unsupported` |

A failed statement leaves the connection usable. Network failures make
`conn.is_open()` false; `conn.open()` reconnects with the same configuration.
`COPY` is refused with `db.ErrorKind.Unsupported`.

## Timeouts and cancellation

`config.timeout_ms` bounds each wait for the server. A statement whose
response times out (`db.ErrorKind.Timeout`) keeps running on the server; the
connection's next request first skips its response. To stop it, ask the server
to cancel it from a new connection:

```dodo
key := conn.cancel_key()
postgres.cancel(&config, &key)?
```

The cancelled statement then fails with `db.ErrorKind.Interrupted`. For
limits enforced by the server, set `statement_timeout` (`conn.exec("set
statement_timeout = '5s'")`).

## Notifications

`LISTEN` and `NOTIFY` work through `conn.notification(timeout_ms)`, which
returns a notification that arrived during earlier requests, or waits up to
`timeout_ms` for one (`0` only checks what the server has sent); `none` when
there is none:

```dodo
conn.exec("listen jobs")?
for {
    let some(n) = conn.notification(30000)? else {
        continue
    }
    handle(n.channel, n.payload)?
}
```

Notifications that arrive while other statements run are queued in the
workspace. If the queue is full, new ones are dropped and counted in
`conn.dropped_notifications()`.

## Connection pools

`db.Pool` lends out connections that you create in advance, each with its own
workspace. Connections open when first needed, are reused, and are reset
(an open transaction is rolled back) when they come back:

```dodo
static mut FIRST: [postgres.WORKSPACE_BYTES]u8 = [0u8; postgres.WORKSPACE_BYTES]
static mut SECOND: [postgres.WORKSPACE_BYTES]u8 = [0u8; postgres.WORKSPACE_BYTES]

conns := [postgres.Connection.new(config.clone(), unsafe { &mut FIRST[..] })?,
    postgres.Connection.new(config.clone(), unsafe { &mut SECOND[..] })?]
pool := db.Pool.new(&mut conns)?
pool.check_on_get(true)

{
    lease := pool.get()?
    lease.conn().execute("update stats set hits = hits + 1", &none_)?
}   // the connection returns here
```

`pool.get()` borrows the pool, so it hands out one connection at a time. Code
that needs several at once, such as a reactor interleaving requests, uses
`ticket := pool.acquire()?`, `pool.conn(&ticket)`, and `pool.release(ticket)`.
Release every ticket: a dropped ticket keeps its connection lent out.
`check_on_get(true)` pings an idle connection before handing it out and
reconnects if the server dropped it. A connection type works with the pool
when it has `is_open`, `open`, `ping`, `reset` and `close` methods.

Returning a connection rolls back its open transaction and drops its queued
notifications. A connection returned in the middle of a response (a cursor
dropped early) is closed instead of waiting for the rest, and reopens on its
next use. Other session state, such as `SET` values, `LISTEN` registrations
and temporary tables, passes to the next user; run `discard all` before
returning a connection to clear it.

## The protocol package

`std/db/postgres/protocol` contains everything that does not need I/O, so you
can drive PostgreSQL connections from your own event loop or on targets
without `std/net`. Writers (`protocol.startup`, `query`, `parse`, `bind`,
`execute`, `sync`, ...) append messages to any value with a
`put(&mut self, data: &[u8]) -> void!db.Error` method, such as
`protocol.Buffer`. `protocol.frame` finds the next message in received bytes,
and parsers (`notice`, `Fields`, `Columns`, `decode`, `affected_rows`, ...)
read its body. `protocol.Scram` implements the SCRAM-SHA-256 exchange with
hash functions you supply through `protocol.Hashes`.

## Complete API reference

For every public type and signature, see
[std/db/postgres](api/std/db/postgres.md),
[std/db/postgres/protocol](api/std/db/postgres/protocol.md), and
[std/db](api/std/db.md).
