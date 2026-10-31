---
title: "Signals"
description: "Interrupt, terminate, hangup and console control events delivered into a poll loop."
section: "Standard library"
order: 154.5
---

`std/signal` turns the requests an operator or service manager sends a running
program into values the program handles in its own loop: Ctrl-C, a request to
terminate, and a request to reload. This hosted API supports Linux GNU, macOS,
and Windows x64.

| `Signal` | POSIX | Windows console |
| --- | --- | --- |
| `Interrupt` | `SIGINT` | Ctrl-C |
| `Terminate` | `SIGTERM` | console closed, logoff, shutdown |
| `Hangup` | `SIGHUP` | never arrives |
| `Break` | never arrives | Ctrl-Break |

Services conventionally reload their configuration on `Hangup`; `Break` is its
Windows counterpart. Selecting a signal the platform lacks is accepted, so one
selection serves every target.

## Quickstart

```dodo test
package main

import "std/signal"

fn main() {
    selection := [signal.Signal.Interrupt, signal.Signal.Terminate]
    listener := signal.listen(&selection)!
    match listener.wait(10)! {
        some(_) => {}
        none => {}
    }
    listener.close()!
}
```

`signal.listen(&selection)` replaces the default action of every selected
signal: instead of ending the process, each arrival is queued.
`listener.next()` returns the oldest pending signal or `none` without waiting;
`listener.wait(milliseconds)` waits up to that long for one, for programs
without a poll loop. Signals are reported in arrival order, one value per
arrival. The operating system may merge a signal that arrives while the same
signal is still being delivered, so count arrivals only when the sender waits
between them.

## In a poll loop

The listener owns a wake channel that is readable while signals are pending.
Register it in the [`PollSet`](networking.md) next to the program's sockets,
and call `next()` until it returns `none` when the channel is ready:

```dodo
package main

import "std/net"
import "std/net/native"
import "std/signal"

fn main() -> i32 {
    selection := [signal.Signal.Interrupt, signal.Signal.Terminate, signal.Signal.Hangup, signal.Signal.Break]
    listener := signal.listen(&selection)!
    set := native.PollSet.new()
    for true {
        set.clear()
        wake := listener.register(&mut set)!
        // Register the program's listeners and connections here.
        match set.wait(1000) {
            ok(_) => {}
            err(reason) => {
                if reason.kind != net.ErrorKind.Interrupted {
                    return 1
                }
            }
        }
        if set.ready(wake) {
            for true {
                match listener.next() {
                    some(value) => {
                        if value == signal.Signal.Interrupt || value == signal.Signal.Terminate {
                            return 0
                        }
                        // Hangup or Break: reload the configuration.
                    }
                    none => {
                        break
                    }
                }
            }
        }
    }
    return 0
}
```

On POSIX, a signal that arrives during `set.wait` also ends the wait early with
`net.ErrorKind.Interrupted`; treat it like a ready wake channel. Signals that
arrive while the program is busy wait in the channel until the next turn.

`listener.cancelled()` reports whether `Interrupt` or `Terminate` has arrived
since `listen`, whether or not `next()` has returned it. That makes a listener
the cancellation source of APIs that take one, such as
[`reactor.serve_with`](web.md), which then returns its report with
`cancelled` set instead of the process ending mid-request:

```dodo
stop := signal.listen(&selection)!
report := reactor.serve_with(b"0.0.0.0:8080", &routes, &mut handler, &mut slots, &mut workspace,
&mut request, &mut response, reactor.Config.defaults(), &mut clock, &stop)!
```

## Lifetime

Signal actions belong to the whole process, so only one listener exists at a
time: a second `listen` fails with `platform/error` `AlreadyExists`.
`close()` restores the actions that were in place before `listen`, as dropping
the listener does, and discards pending signals. Closing twice does nothing.
A closed listener's `next()` returns `none`, and `register` and `wait` fail.

On POSIX the handler sets `SA_RESTART`, so blocking reads and writes elsewhere in
the program continue across a signal. Child processes start with the default
actions. The wake channel is a pipe, kept open for the rest of the process once
created, so that a handler running on another thread never writes to a reused
descriptor; `listen` after `close` reuses it.

On Windows the wake channel is a connected loopback socket pair, because
`WSAPoll` waits only on sockets. The system ends a process as soon as its
handler for a console close, logoff or shutdown returns, so the handler waits
after queuing `Terminate`: the program can shut down and exit on its own until
the system's deadline, about five seconds for a closed console. A process
started without a console, or in a new process group, receives no Ctrl-C.
