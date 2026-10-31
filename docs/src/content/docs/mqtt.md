---
title: "MQTT"
description: "Run an MQTT broker, write publish/subscribe apps with topic handlers and timers, and test them without a network."
section: "Standard library"
order: 159.7
---

MQTT is a publish/subscribe protocol that is popular for home automation and
small devices. Programs never talk to each other directly. A **broker** sits in
the middle: a client **publishes** a message to a **topic** such as
`home/kitchen/temperature`, and the broker forwards it to every client that has
**subscribed** to a matching topic filter such as `home/+/temperature`.

Dodo has everything for both sides, much like the [web](web.md) packages:

| You want to | Use | Start with |
| --- | --- | --- |
| Run a broker on your machine or network | `std/mqtt/broker` | `broker.new().run(b"0.0.0.0:1883")` |
| React to messages, publish on a timer, report status | `std/mqtt/app` | `app.new(b"my-app").on(filter, handler).run(url)` |
| Control every step yourself | `std/mqtt/hosted` | `hosted.Client.connect(url, client_id, &mut workspace)` |
| Encode packets for your own transport | `std/mqtt` | `mqtt.encode_publish`, `mqtt.decode` |

`std/mqtt` also holds the types every layer shares: `mqtt.Message`,
`mqtt.Outbox`, `mqtt.QoS`, and `mqtt.Error`. The broker, `app` and `hosted`
run on Linux GNU, macOS, and Windows x64; `std/mqtt` also builds for
WebAssembly and bare-metal targets.

## Quickstart

You need two programs: a broker and an app. Save this as `broker.dodo`:

```dodo
package main
import "std/mqtt/broker"

fn main() -> i32 {
    return broker.new()
        .on(b"#", broker.print())
        .run(b"127.0.0.1:1883")
}
```

Start it with `dodo run broker.dodo`. It accepts clients on port 1883, the
standard MQTT port, and prints every message it sees. Ctrl+C stops it.

In a second terminal, save this app as `main.dodo`:

```dodo
package main
import "std/mqtt"
import "std/mqtt/app"

fn main() -> i32 {
    return app.new(b"dodo-pinger")
        .on(b"ping", mqtt.reply(b"pong", b"hello from Dodo"))
        .run(b"mqtt://127.0.0.1")
}
```

Start it with `dodo run`. The app connects, subscribes to `ping`, and answers
every message on `ping` with a message on `pong`. Any MQTT client can talk to
it. With the [Mosquitto](https://mosquitto.org/) command-line tools installed,
send a ping:

```text
mosquitto_pub -h 127.0.0.1 -t ping -m hi
```

The broker terminal now shows `ping hi` and then `pong hello from Dodo`.

`run` returns an exit code for `main`: 0 after a clean stop, 1 if starting
failed (it prints why, for example a wrong address or a refused login), and 2
if any handler returned an error along the way. `run` keeps going until the
process stops. The app reconnects by itself if the broker restarts.

## Handlers

`mqtt.reply` is a ready-made handler. Your own handlers are structs with a
`handle` method. The app calls it for every message whose topic matches the
filter you registered it with:

```dodo
package lamp
import "core/bytes"
import "std/mqtt"
import "std/mqtt/app"

pub struct Lamp {
    on: bool
    pub fn handle(&mut self, message: &mqtt.Message, outbox: &mut mqtt.Outbox) -> void!mqtt.Error {
        if bytes.equal(message.payload, b"on") {
            self.on = true
        } else if bytes.equal(message.payload, b"off") {
            self.on = false
        } else {
            return err(mqtt.failure(mqtt.ErrorKind.InvalidInput, 0))
        }
        return outbox.publish_with(b"home/lamp/state", message.payload, mqtt.QoS.AtLeastOnce, true)
    }
}
fn main() -> i32 {
    return app.new(b"dodo-lamp")
        .on(b"home/lamp/set", Lamp { on: false })
        .on(b"home/#", app.print())
        .run(b"mqtt://127.0.0.1")
}
```

Handlers are ordinary values, so they keep state between messages, like
`Lamp.on` here. The handler receives:

- `message`, with `topic` and `payload` bytes, its `qos`, `retain` (a stored
  value the broker sent on subscription), and `duplicate` (possibly sent
  before). `message.level(1)` returns one level of the topic: for
  `home/kitchen/light` it is `kitchen`. `message.matches(filter)` tests
  another filter.
- `outbox`, for messages to publish in response. `outbox.publish(topic,
  payload)` sends at QoS 0; `outbox.publish_with(topic, payload, qos, retain)`
  chooses the [quality of service](#publish) and whether the broker keeps the
  message as the topic's current value. The app sends them right after the
  handler returns.

Every handler whose filter matches runs, in the order you registered them.
Above, a message on `home/lamp/set` goes to `Lamp` and is printed. If a
handler returns an error, the app prints it to stderr, counts it, and carries
on with the next message.

| Ready-made handler | Effect |
| --- | --- |
| `mqtt.reply(topic, payload)` | Publishes a fixed payload to a fixed topic. |
| `mqtt.forward(topic)` | Republishes each payload to another topic. |
| `app.print()`, `broker.print()` | Prints `topic payload` lines to stdout. |

Be careful when a handler publishes to a topic the same app subscribes to: the
broker sends that message back, which can loop forever.

## Timers and settings

`.every(milliseconds, task)` runs a task right after connecting and then
repeatedly. A task is a struct with a `tick` method that publishes through its
outbox, for example a sensor reading:

```dodo
pub struct Thermometer {
    pub fn tick(&mut self, outbox: &mut mqtt.Outbox) -> void!mqtt.Error {
        return outbox.publish(b"home/kitchen/temperature", b"21.5")
    }
}
fn main() -> i32 {
    return app.new(b"kitchen-sensor")
        .every(10000, Thermometer {})
        .status(b"home/kitchen/status")
        .run(b"mqtt://127.0.0.1")
}
```

To publish a number you compute, format it into a byte buffer first, as shown
in [format into caller-owned bytes](formatting.md#format-into-caller-owned-bytes).

| Builder method | Effect |
| --- | --- |
| `.on(filter, handler)` | Subscribe and call `handler` for matching messages. |
| `.every(milliseconds, task)` | Call `task.tick(outbox)` after connecting and then periodically. |
| `.status(topic)` | Keep a retained `online` or `offline` on `topic`, even after a crash. |
| `.login(username, password)` | Credentials for brokers that require them. |
| `.will(topic, payload)` | Message the broker publishes if the app disappears without disconnecting. |
| `.qos(qos)` | Subscription quality of service; `mqtt.QoS.AtLeastOnce` by default. |
| `.keep_alive(seconds)` | How often an idle app pings the broker; 60 by default. |
| `.max_messages(count)` | Disconnect after this many messages. Useful for tests and scripts. |
| `.build()` | Check the setup and get an `App` for [tests](#test-without-a-broker). |
| `.run(url)` | Connect and run, with printed errors and an exit code. |
| `.serve(url)`, `.serve_until(url, cancel)` | Run and return a `Report`, or stop when `cancel.cancelled()` returns true. |

An invalid filter, such as `home/#/light`, is reported by `build` and `run`
before connecting:

```text
mqtt route #2 (home/#/light): InvalidFilter; use levels separated by /, + for one whole level, or # as the last level
```

`serve` returns an `app.Report` with the `received`, `published`, and `failed`
counts, the number of `reconnects`, and the `last_error`. It fails only if the
first connection fails or the broker refuses the login or a subscription. After
that, a lost connection is printed and retried, waiting 1 second at first and
up to 30 seconds between attempts.

## Test without a broker

`build()` returns the configured app without connecting. `deliver(topic,
payload)` runs the matching handlers in process, and `tick()` runs every
task once. Both return an `mqtt.Delivery` that records what was published:

```dodo test
package lamp_test
import "core/bytes"
import "std/mqtt"
import "std/mqtt/app"

pub struct Lamp {
    pub fn handle(&mut self, message: &mqtt.Message, outbox: &mut mqtt.Outbox) -> void!mqtt.Error {
        if !bytes.equal(message.payload, b"on") && !bytes.equal(message.payload, b"off") {
            return err(mqtt.failure(mqtt.ErrorKind.InvalidInput, 0))
        }
        return outbox.publish(b"home/lamp/state", message.payload)
    }
}
pub struct Heartbeat {
    pub fn tick(&mut self, outbox: &mut mqtt.Outbox) -> void!mqtt.Error {
        return outbox.publish(b"home/lamp/heartbeat", b"alive")
    }
}

@test
fn lamp_reports_its_state() {
    lamp := app.new(b"lamp")
        .on(b"home/lamp/set", Lamp {})
        .every(10000, Heartbeat {})
        .build()!
    lamp.deliver(b"home/lamp/set", b"on")
        .expect_ok()
        .expect_handled(1)
        .expect_publish(b"home/lamp/state", b"on")
    lamp.deliver(b"home/lamp/set", b"purple").expect_failure().expect_published(0)
    lamp.deliver(b"home/garage/set", b"on").expect_handled(0)
    lamp.tick().expect_publish(b"home/lamp/heartbeat", b"alive")
}
```

Run `dodo test`. The `expect_*` methods assert and return the delivery, so
they chain. `expect_handled(n)` counts matching handlers, `expect_published(n)`
counts published messages, and `expect_publish(topic, payload)` looks for one.
To inspect results yourself, use `handled`, `failure`, `len()`, `published(index)`,
or `contains(topic, payload)`. A delivery keeps copies of up to
`mqtt.DELIVERY_BYTES` of published messages.

## The broker

`broker.new().run(address)` is a complete broker for a home network, a lab, or
tests. Use `b"127.0.0.1:1883"` to accept only programs on the same computer,
or `b"0.0.0.0:1883"` to accept other devices on your network too.

The broker supports what MQTT 3.1.1 clients expect: QoS 0, 1, and 2,
wildcard subscriptions, retained messages, wills, keep-alive, and a new
connection with the same client ID taking over the old one. Any MQTT 3.1.1
client works with it, including `std/mqtt/app`, Mosquitto's tools, and phone
apps.

Broker handlers work like app handlers. They see every message a client
publishes to a matching topic, and what they put in the outbox goes to
subscribers like any other message:

```dodo
package doorbell
import "std/mqtt"
import "std/mqtt/broker"

pub struct Doorbell {
    rings: u32
    pub fn handle(&mut self, message: &mqtt.Message, outbox: &mut mqtt.Outbox) -> void!mqtt.Error {
        self.rings += 1
        return outbox.publish(b"home/announce", b"someone is at the door")
    }
}
fn main() -> i32 {
    return broker.new()
        .on(b"home/doorbell", Doorbell { rings: 0 })
        .password(b"family", b"secret")
        .run(b"0.0.0.0:1883")
}
```

Messages published by handlers are not passed to other broker handlers, so
`broker.print()` does not show them.

| Builder method | Effect |
| --- | --- |
| `.on(filter, handler)` | Call `handler` for messages clients publish to matching topics. |
| `.password(username, password)` | Accept only clients that log in with these credentials. |
| `.authenticate(auth)` | Decide yourself: `auth.allow(client_id, username, password) -> bool`. |
| `.max_connections(count)` | Stop once this many connections have come and gone. |
| `.build()` | Check the setup and get a `Broker` for tests. |
| `.run(address)`, `.serve(address)`, `.serve_until(address, cancel)` | As for apps. |

Refused clients receive CONNACK return code 5 (not authorized). Test handlers
and authenticators without a network, too:

```dodo test
package broker_test
import "std/mqtt"
import "std/mqtt/broker"

@test
fn doorbell_announces_visitors() {
    server := broker.new()
        .on(b"home/doorbell", mqtt.reply(b"home/announce", b"visitor"))
        .password(b"family", b"secret")
        .build()!
    server.publish(b"home/doorbell", b"ring").expect_handled(1).expect_publish(b"home/announce", b"visitor")
    assert(server.allows(b"phone", b"family", b"secret"))
    assert(!server.allows(b"phone", b"family", b"guess"))
}
```

The broker is one thread that keeps everything in fixed memory, so it has
limits. The defaults suit a household of devices:

| Limit | Default | What happens beyond it |
| --- | --- | --- |
| Simultaneous clients | `broker.CLIENTS` (16) | New connections wait until one leaves. |
| Packet size | `broker.PACKET_BYTES` (4096) | The client is disconnected. |
| Subscriptions per client | `broker.FILTER_BYTES` (1024 bytes of filters) | SUBACK refuses the filter (128). |
| Client ID length | `broker.ID_BYTES` (128) | CONNACK refuses the client (2). |
| Will topic plus payload | `broker.WILL_BYTES` (512) | CONNACK refuses the client (3). |
| All retained messages | `broker.RETAINED_BYTES` (16384) | The new message is delivered but not retained. |

Everything lives in memory: subscriptions, retained messages, and sessions
are lost when the broker stops, and a client asking for a persistent session
always starts a new one. A client that does not read for
`config.write_timeout_ms` (5 seconds) is disconnected so it cannot stall
everyone else. `serve` returns a `broker.Report` with counts of `accepted`,
`connected`, and `refused` connections, `messages` received, copies
`delivered`, handler `failed`, and `dropped` connections.

## The step-by-step client

`std/mqtt/hosted` is the layer under `app`: you call `connect`, `publish`,
`subscribe`, and `receive` yourself, in your own loop. Use it for short
scripts, or when a program decides itself when to talk to the broker. This
program subscribes to `lights/#`, publishes to `lights/hall`, and prints the
message the broker sends back:

```dodo
package lights
import "std/console"
import "std/mqtt"
import "std/mqtt/hosted"

fn main() {
    workspace := [0u8; hosted.WORKSPACE_BYTES]
    client := hosted.Client.connect(b"mqtt://localhost", b"dodo-lights", &mut workspace)!
    client.subscribe(b"lights/#", mqtt.QoS.AtLeastOnce)!
    client.publish(b"lights/hall", b"on")!
    message := client.receive()!
    out := console.stdout()
    out.write(message.topic)!
    out.write(b" = ")!
    out.write(message.payload)!
    out.write(b"\n")!
    client.disconnect()!
}
```

It prints `lights/hall = on`.

URLs have the form `mqtt://host[:port]`; the port defaults to 1883 and IPv6
literals use brackets (`mqtt://[::1]:1883`). The client ID names the session on
the broker. Two clients connected with the same ID disconnect each other.

### Storage

The client keeps no heap memory. Every packet lives in the workspace you pass to
`connect`, which the client borrows until it is dropped:

- The first quarter stages outgoing packets. A larger payload is sent straight
  from your slice after the packet header.
- The remaining three quarters buffer incoming packets. **The largest message
  you can receive is the size of this buffer**, minus a few header bytes. A
  larger one fails with `PacketTooLarge` and ends the connection.

`hosted.WORKSPACE_BYTES` (16 KiB) receives messages up to about 12 KiB. Pass a
larger array for bigger messages. The minimum is `hosted.MIN_WORKSPACE_BYTES`.

### Publish

`publish(topic, payload)` sends a QoS 0 message. `publish_with(topic, payload,
qos, retain)` chooses the quality of service. It returns when the broker has
done what that QoS promises:

| QoS | `mqtt.QoS` | Delivery | `publish_with` returns after |
| --- | --- | --- | --- |
| 0 | `AtMostOnce` | Fire and forget; may be lost | the packet is written |
| 1 | `AtLeastOnce` | Retried until acknowledged; may repeat | PUBACK |
| 2 | `ExactlyOnce` | Four-step handshake | PUBREC, PUBREL, PUBCOMP |

With `retain` set, the broker stores the message as the topic's last known
value and gives it to future subscribers immediately. Publishing an empty
retained payload clears it.

Topic names cannot be empty or contain the `+` or `#` wildcards. An invalid
topic fails with `InvalidTopic` before anything is sent, and the client stays
usable.

### Subscribe and receive

`subscribe(filter, qos)` returns the QoS the broker granted, which can be lower
than the one you asked for. Messages on a subscription arrive at no more than
the granted QoS. A broker that refuses the filter makes `subscribe` fail with
`Rejected`. The connection stays open. `unsubscribe(filter)` removes a
subscription.

`receive()` blocks until the next message arrives. `poll(timeout_ms)` waits at
most `timeout_ms` and returns `none` if nothing arrived; `poll(0)` only takes a
message that is already buffered.

```dodo
fn drain(client: &mut hosted.Client) -> void!mqtt.Error {
    for true {
        match client.poll(1000)? {
            some(message) => {
                handle(message.topic, message.payload)
            }
            none => {
                return ok()
            }
        }
    }
    return ok()
}
```

An `mqtt.Message` has `topic`, `payload`, `qos`, `retain` (a stored value sent on
subscription), and `duplicate` (the broker may have sent it before). Its views
borrow the client's workspace, so copy anything you need to keep before calling
the client again.

**Acknowledgement happens on your next call.** A QoS 1 or 2 message is
acknowledged when you next call `receive`, `poll`, `publish`, `subscribe`, or
another client method. If your program stops while handling a message, the
broker still considers it undelivered.

Messages that arrive while `publish_with`, `subscribe` or `unsubscribe` waits
for its acknowledgement are kept in order for the next `receive`. They count
against the incoming buffer. If too many pile up, the call fails with
`BufferFull`.

### Options and keep-alive

`hosted.Options.new(client_id)` gives the defaults; change fields and pass the
value to `Client.connect_with`:

```dodo
options := hosted.Options.new(b"thermostat-1")
options.username = b"thermostat"
options.password = password
options.keep_alive_s = 30
options.will_topic = b"devices/thermostat-1/status"
options.will_payload = b"offline"
options.will_retain = true
client := hosted.Client.connect_with(b"mqtt://broker.local", &options, &mut workspace)?
```

| Field | Default | Meaning |
| --- | --- | --- |
| `username`, `password` | empty (absent) | Credentials. A password needs a username. |
| `keep_alive_s` | 60 | Idle seconds before the client pings the broker. 0 disables pings. |
| `clean_session` | `true` | `false` asks the broker to keep subscriptions and queued messages for this client ID between connections. `session_present()` reports whether it did. |
| `will_topic`, `will_payload`, `will_qos`, `will_retain` | no will | Message the broker publishes if the connection ends without `disconnect()`. |
| `connect_timeout_ms` | 10000 | Resolution, TCP connect, and CONNACK together. |
| `timeout_ms` | 10000 | Each write, and each wait for an acknowledgement. |

The client sends keep-alive pings itself, but only while one of its methods is
running. While waiting for messages, call `poll` or `receive` at least once per
keep-alive interval. If the broker does not answer a ping within the interval,
the call fails with `TimedOut`.

`disconnect()` ends the session cleanly, so the broker discards the will
message. Dropping the client closes the socket without DISCONNECT, which
publishes the will.

### Errors

Every operation returns `mqtt.Error`, with a `kind` and a numeric `code`:

| `kind` | Cause | `code` |
| --- | --- | --- |
| `InvalidInput`, `InvalidTopic`, `InvalidFilter` | Rejected before sending; the client stays usable. | 0 |
| `Refused` | The broker refused CONNECT. | CONNACK return code, such as `mqtt.BAD_CREDENTIALS` (4) or `mqtt.NOT_AUTHORIZED` (5) |
| `Rejected` | The broker refused a subscription; the client stays usable. | `mqtt.SUBSCRIBE_FAILURE` (128) |
| `Resolve`, `Connect`, `Network` | DNS, TCP connection, or socket failure. | Operating-system code |
| `TimedOut` | A deadline passed, or the broker stopped answering pings. | 0 or system code |
| `Closed` | The broker closed the connection, or the client was disconnected. | 0 or system code |
| `Malformed`, `Protocol`, `PacketTooLarge`, `BufferFull` | The broker sent invalid or unexpected data, or more than the workspace holds. | 0 |

Apart from the cases marked usable, an error ends the session, and later calls
fail with `Closed`. The client does not reconnect by itself. To recover, drop it
and connect again with the same workspace.

## Topics

Topic levels are separated by `/`. In filters, `+` matches exactly one level,
and `#`, which must be the last level, matches that level and everything below
it. Wildcards in the first level never match topics starting with `$`, which
brokers use for their own statistics.

```dodo test
package topic_example
import "std/mqtt"

@test
fn filters_select_topics() {
    assert(mqtt.matches(b"sensors/+/temperature", b"sensors/kitchen/temperature"))
    assert(mqtt.matches(b"sensors/#", b"sensors"))
    assert(!mqtt.matches(b"sensors/+", b"sensors/kitchen/temperature"))
    assert(!mqtt.matches(b"#", b"$SYS/broker/uptime"))
    assert(mqtt.valid_filter(b"sensors/+/temperature"))
    assert(!mqtt.valid_filter(b"sensors/#/temperature"))
    assert(!mqtt.valid_topic(b"sensors/+"))
}
```

`valid_topic`, `valid_filter`, and `valid_string` check the MQTT rules: UTF-8
without U+0000, at most 65535 bytes, and wildcards only where allowed.

## Packets for other transports

On targets without the hosted client, use `std/mqtt` directly over any byte
stream. Each `encode_*` function writes one complete packet into your buffer
and returns its length: `encode_connect` (from an `mqtt.Connect` value),
`encode_publish`, `encode_publish_head` (everything except the payload),
`encode_subscribe`, `encode_unsubscribe`, `encode_ack` (PUBACK, PUBREC, PUBREL,
PUBCOMP, UNSUBACK), `encode_pingreq`, and `encode_disconnect`. The server side has `encode_connack`, `encode_suback`,
and `encode_pingresp`, plus `decode_connect`, `subscribe_entry`, and
`unsubscribe_entry` for reading client requests.

For input, `frame_length(bytes)` returns `none` until the fixed header is
complete, then the total packet length. Once that many bytes have arrived,
`decode` returns a `Packet` whose `topic` and `payload` borrow the buffer.

```dodo test
package codec_example
import "core/bytes"
import "std/mqtt"

@test
fn round_trips_a_publish() {
    frame := [0u8; 64]
    length := mqtt.encode_publish(&mut frame, b"lights/hall", b"on", mqtt.QoS.AtLeastOnce, false, false, 1)!
    packet := mqtt.decode(&frame[..length])!
    assert(packet.kind == mqtt.PacketKind.Publish)
    assert_eq(packet.length, length)
    assert_eq(packet.packet_id, 1u16)
    assert(bytes.equal(packet.topic, b"lights/hall"))
    assert(bytes.equal(packet.payload, b"on"))
}
```

## Limits

- MQTT 3.1.1 only, for clients and the broker. MQTT 5 properties and reason
  codes are not supported; every common broker and client also speaks 3.1.1.
- Plain TCP only. `mqtts://` (TLS) and WebSocket transports are not built in
  yet, so keep the broker on a trusted network.
- `hosted.Client` has one acknowledged operation in flight at a time and does
  not reconnect; `app` reconnects for you.
- In-flight QoS 1 and 2 state is kept only for the current connection. After a
  reconnect a message may arrive again, so make handlers safe to repeat.
