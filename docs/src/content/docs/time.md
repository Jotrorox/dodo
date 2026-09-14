---
title: "Time and clocks"
description: "Exact durations, UTC calendars, explicit clock identity, and deterministic timers."
section: "Using Dodo"
order: 145
---

Use `std/time/hosted.WallClock.new().wall_now()` to read the current UTC timestamp
on Linux GNU x86-64 or Windows x64. Import `std/console` and `std/fmt` to print
Unix seconds. For elapsed time and deadlines use `hosted.MonotonicClock` instead:
a wall clock can jump when the system time changes.

## Quickstart

Save this as `time_start.dodo`:

```dodo test
package time_start
import "std/time/hosted"
import "std/console"
import "std/fmt"

fn main() -> i32 {
    clock := hosted.WallClock.new()
    match clock.wall_now() {
        ok(timestamp) => {
            value := fmt.signed(timestamp.seconds())
            match console.println_value(&value) {
                ok(_) => { return 0 }, err(_) => { return 2 },
            }
        },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run time_start.dodo
```

Expected stdout: an integer such as `1789344000` followed by a newline.
It is seconds since 1970-01-01 00:00:00 UTC, so it changes with the system clock.
Success exits 0. Exit 1 means the clock could not be read; propagate the failure
instead of substituting a believable timestamp. `wall_now_native` preserves the
native error code when diagnostics are needed. Exit 2 means printing failed.
The clock and timestamp are fixed-size values; no buffer or allocator is needed.

`std/time` itself contains portable values, and `std/time/clock` supplies clock
contracts and deterministic fakes. Importing either does not read the OS clock.
Use `clock.FakeClock` / `FakeWallClock` in repeatable tests; choose
`std/time/iso8601` to parse or format UTC text, and `std/time/timer` for the
portable timer contract. Those imports are independent of the hosted provider.

## Hosted clock behavior

`hosted.MonotonicClock.new().now()` returns an `Instant` with the reserved native
clock identity `hosted.MONOTONIC_ID`. All native instances share an origin;
custom and fake clocks must use other IDs. Do not persist or serialize monotonic
observations as portable deadlines. `now_native` retains platform errors;
`now` maps them to `TimeError.ClockFailure`. `now_ms` supports network clock
contracts and returns the maximum u64 on failure, causing deadline checks to
fail closed. `WallClock` returns UTC timestamps, with no local-time conversion.

`Duration.to_millis()` truncates fractional milliseconds. `to_millis_ceil()`
rounds up; `timeout_millis()` additionally rejects the all-ones infinite-wait
sentinel. All overflowing conversions fail. These helpers do not sleep.

## API and contracts

`std/time` contains allocation-free values and arithmetic. Importing it does
not read a clock, sleep, schedule work, start a runtime, or consult a timezone.
Optional packages add capabilities and text conversion independently:

| Import | Purpose |
| --- | --- |
| `std/time` | Duration, Timestamp, Date, TimeOfDay, DateTime, TimeShift, Instant |
| `std/time/clock` | Static clock contracts, FakeClock, FakeWallClock, Deadline |
| `std/time/timer` | Static one-shot timer contract and FakeTimer |
| `std/time/iso8601` | Parsing and formatting over borrowed byte slices, using `std/text` and `std/fmt` |

These portable packages have no OS clock, blocking wait, scheduler, timezone database, locale,
implicit local timezone, or daylight-saving conversion in these packages.
Application adapters can provide clock/timer methods without changing the
portable values. All storage is fixed-size; numeric operations and calendar
conversions use O(1) work and storage. Parsing/formatting scans bounded fields
and uses only caller-provided buffers.

## Duration and timestamp guarantees

`Duration` is nonnegative, with u64 whole seconds and a normalized nanosecond
fraction in 0..999999999. The maximum is 2^64−1 seconds plus 999999999 ns.
`Duration.new(seconds, nanoseconds)` normalizes any u64 nanosecond argument
and returns overflow if the carried seconds do not fit. `zero`, `from_seconds`,
`from_millis`, `from_micros`, and `from_nanos` construct exact values.
`seconds` and `subsec_nanos` expose the normalized components. `compare`,
`is_zero`, and `clone` do not allocate.

`checked_add`, `checked_sub`, `checked_mul(u32)`, and `checked_div(u32)` return
mandatory Results. Subtraction below zero returns `Before`; division by zero
returns `InvalidComponent`. Multiplication/addition overflow returns
`Overflow`, including a carry from the fraction. Integer division discards
fractional nanoseconds toward zero. `to_millis`, `to_micros`, and `to_nanos`
check u64 overflow; conversion to a coarser unit truncates toward zero.

`Timestamp` uses the POSIX/Unix epoch, 1970-01-01T00:00:00Z, with i64 whole
seconds and the same nonnegative nanosecond fraction. Its whole second is
floored toward negative infinity: −1 ns is represented by seconds −1 and
nanoseconds 999999999. `Timestamp.new(seconds, fraction)` rejects fractions
outside the normalized range. `epoch`, `from_unix_seconds`, and
`from_unix_nanos` are exact. `to_unix_nanos` returns a checked i64, including
both extreme endpoints; most representable timestamps exceed this narrower
nanosecond range. Values have nanosecond precision, independently of the
resolution or accuracy of any actual clock.

Timestamps support `compare`, `clone`, `checked_add(&Duration)`,
`checked_sub(&Duration)`, and `duration_since(&Timestamp)`. The last method
returns `Before` for reversed arguments and can represent the entire difference
between the minimum and maximum timestamps. It computes a difference between
wall-time values; use monotonic instants for measuring elapsed time.
`TimeError` distinguishes `Overflow`, `InvalidComponent`, `Before`,
`DifferentClock`, and adapter-reported `ClockFailure`.

## Gregorian calendar and time of day

`Date.new(year, month, day)` validates astronomical years 0000..9999, months
1..12, and the day's range. This is the proleptic Gregorian calendar for the
entire range: divisible by 4 means leap year, except centuries not divisible by
400. Year 0000 is a leap year. Historical calendar adoption dates are not
modeled. `is_leap_year` and `days_in_month` expose these rules.

`days_since_epoch` and `from_days_since_epoch` convert exact calendar dates to
and from days relative to 1970-01-01. The supported day range is
−719528..2932896 inclusive. `weekday` uses ISO numbering Monday=1 through
Sunday=7. `checked_add_days(i64)` rejects calendar-range and integer overflow;
`days_since(&Date)` returns a signed day difference. `compare`, `clone`,
`year`, `month`, and `day` complete the value API.

`TimeOfDay.new(hour, minute, second, nanoseconds)` requires hour 0..23,
minute/second 0..59, and nanosecond 0..999999999. Midnight is 00:00:00;
24:00:00 is rejected. `seconds_since_midnight`,
`from_seconds_since_midnight`, `since_midnight`, and `from_duration` provide
checked conversions, with a day limited to 86400 seconds. `checked_add` and
`checked_sub` take a Duration and return `TimeShift`, exposing both
`day_offset()` and `time_of_day()`. Crossing midnight therefore retains the
number and direction of crossed days. All possible Duration inputs fit this
signed day offset. Time values also have component accessors, `compare`, and
`clone`.

`DateTime.new(Date, TimeOfDay)` combines a UTC date and time. `date` and
`time_of_day` return checked borrows tied to the owner; mutation or destruction
cannot invalidate a live view. `to_timestamp` is exact and infallible within
the calendar range. `from_timestamp`, `checked_add`, and `checked_sub` reject
values outside years 0000..9999. `duration_since` returns a checked nonnegative
duration. Calendar conversion follows Howard Hinnant's publicly donated
[civil date algorithms](https://howardhinnant.github.io/date_algorithms.html),
using 400-year eras and explicit floor division for negative dates.

Every civil day has exactly 86400 POSIX seconds. Leap seconds are not
represented: second 60 is rejected, there is no leap-second table, and no
smearing policy is silently applied. These values are neither TAI nor a model
of physical elapsed SI seconds across a leap-second insertion.

## Monotonic clocks and deterministic timers

`Instant.new(clock_id, ticks)` contains a u64 clock identity and a Duration
measured from that clock's origin. `compare` and `duration_since` reject
identities that differ, even if tick values are equal. Subtracting a later
instant returns `Before`. `checked_add` and `checked_sub` preserve identity;
`clone`, `clock_id`, and the borrowed `elapsed_ticks` expose explicit values.
There is no conversion from Instant to Timestamp.

A monotonic provider implements `now(&self) -> Instant!TimeError`; a wall-clock
provider implements `wall_now(&self) -> Timestamp!TimeError`. Free functions
`clock.now` and `clock.wall_now` dispatch statically. Providers must assign
one stable identity per clock domain, never reuse an identity for an unrelated
live domain, and never move monotonic ticks backward. Caller-supplied identities
are logical domain identifiers, not automatic globally unique IDs. Resolution,
frequency conversion, suspend behavior, and OS failures are adapter contracts;
portable time arithmetic does not invent those properties.

`FakeClock.new(id)` starts at zero; `starting_at(id, duration)` selects a
starting tick. `advance(&duration)` advances exactly, or leaves the fake
unchanged on overflow. Different fake clock domains must use distinct IDs.
`FakeWallClock.new(timestamp)` supports `set(timestamp)` in either direction
and checked `advance`. Changing a fake wall clock never changes any monotonic
clock.

`Deadline.at(instant)` and `clock.deadline_after(source, duration)` construct
nonblocking deadlines. `is_due(&instant)` and
`clock.deadline_poll(&deadline, source)` return true at or after the target,
rejecting a different clock identity. No work is scheduled by these calls.

The separate timer contract is `arm(Instant) -> void!TimeError`,
`cancel() -> bool`, and `poll(&Instant) -> bool!TimeError`, exposed through
monomorphized free functions. `FakeTimer.new(clock_id)` is deterministic and
one-shot. Arming replaces a previous deadline; wrong identity leaves it
unchanged. A due poll returns true once and disarms. Cancellation returns
whether a timer was previously armed. Polling with the wrong identity always
fails, including when disarmed. There is no hidden blocking wait.

```dodo test
package deadline_example
import "std/time"
import "std/time/clock"

fn run() -> bool!time.TimeError {
    source := clock.FakeClock.new(1)
    interval := time.Duration.from_millis(250)
    deadline := clock.deadline_after(&source, &interval)?
    source.advance(&interval)?
    return clock.deadline_poll(&deadline, &source)
}

@test
fn deadline_expires_after_the_interval() {
    match run() {
        ok(expired) => { assert(expired) },
        err(_) => { assert(false, "advancing the fake clock must succeed") },
    }
}
```

## Parsing and formatting

The `iso8601` package accepts ASCII bytes with explicit syntax:

- `parse_date`: `YYYY-MM-DD`, exactly four year digits.
- `parse_time`: `HH:MM:SS`, optionally followed by a dot and 1..9 fractional digits.
- `parse_timestamp`: date, uppercase `T`, time, uppercase `Z`.

No whitespace, signs on years, lowercase suffixes, numeric offsets, leap
seconds, or implicit timezone is accepted. The syntax is a deliberately bounded
ISO 8601 UTC subset, not every ISO 8601 representation. Fractional input is
scaled exactly to nanoseconds; excessive precision is rejected rather than
rounded.

`format_date(output, &date)` writes 10 bytes, `format_time(output, &time)`
writes 18 bytes including exactly nine fractional digits, and
`format_timestamp(output, &stamp)` writes 30 bytes in
`YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ` form. They return the written length, preserve
bytes after that length, and leave all bytes unchanged on failure. No string
allocation or NUL terminator is added. Errors distinguish `InvalidSyntax`,
`InvalidDateTime`, and `BufferTooSmall`. Parsing composes
`std/text.parse_u32_decimal`; formatting composes
`std/fmt.write_u32_padded`. These helpers reuse the full text integer parser and
byte-sink formatter described in the [standard library](standard-library.md),
with fixed field widths and complete-output capacity checks supplied by this
package. Time values themselves import neither formatting nor I/O.

## Verification

`tests/time_library.rs` executes fixtures at `-O0` and `-O3` and emits
WebAssembly and Cortex-M0 objects. The fixture exercises overflow and negative
epoch boundaries, 10000 deterministic duration-model operations, an entire
146097-day Gregorian cycle, every second in a day, timestamp/calendar
round trips around the epoch, 134 independent Python `datetime` calendar
vectors, separate clocks, one-shot timers, exact parsing/formatting, and atomic
buffer failure. Compiler rejection tests preserve private invariants, mandatory
Result handling, and distinct monotonic/wall-time types. Portable object checks
do not execute an OS clock or board startup. See `examples/time.dodo` for a
complete executable example.

## Native providers and elapsed measurements

Import `std/time/hosted` explicitly for OS observations. Its `MonotonicClock.now`
and `WallClock.wall_now` implement the `std/time/clock` structural contracts;
`now_native` and `wall_now_native` preserve `std/platform/error.Error` with the
native error code (with kind `Other`). The portable contracts map failures to `TimeError.ClockFailure`.
There is no allocation, sleeping, scheduler, or timezone lookup.

Linux reads `CLOCK_MONOTONIC` (which excludes suspended time) and `CLOCK_REALTIME`.
Windows uses `QueryPerformanceCounter`/`QueryPerformanceFrequency` for monotonic
time and `GetSystemTimePreciseAsFileTime` for wall time (Windows 8 or later).
Windows performance-counter elapsed time includes sleep/hibernation. Wall times
use the Unix epoch; Windows 100 ns ticks are converted to normalized seconds and
nanoseconds. Precision of the value type does not imply equal clock resolution.
Both monotonic clocks can return equal consecutive values; wall clocks can jump.

`hosted.MonotonicClock.now_ms` and legacy `std/net/native.MonotonicClock.now_ms`
use the same provider and truncation, so their millisecond domains agree. Both
return the maximum timestamp on failure for existing network deadline contracts.
Synchronization's `milliseconds` uses `Duration.timeout_millis` and maps conversion
failures to its `InvalidInput`. Portable clocks/durations do not import these
hosted integrations. Native monotonic IDs are reserved for this adapter;
application clocks must choose a different identity.

[hosted_elapsed.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_elapsed.dodo)
is a complete elapsed-time example: it prints nanoseconds between two observations.
Place work between those observations to measure it; zero is a valid result.
