//! MQTT 3.1.1 codec vectors, portability, and the hosted client against a
//! scripted loopback broker.
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-mqtt-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn compile(source: &Path, output: &Path, optimization: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", optimization, "-o"])
            .arg(output)
            .output()
            .unwrap(),
        &format!("compile {} at O{optimization}", source.display()),
    );
}

#[test]
fn codec_vectors_pass_and_cross_compile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/stdlib/mqtt_checks.dodo");
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let binary = workspace.0.join(format!("checks-{optimization}"));
        compile(&source, &binary, optimization);
        success(
            Command::new(&binary).output().unwrap(),
            &format!("run mqtt_checks at O{optimization}"),
        );
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(&source)
                    .args([
                        "-O",
                        optimization,
                        "--target",
                        target,
                        "--emit",
                        "obj",
                        "-o",
                    ])
                    .arg(workspace.0.join(format!("{target}-{optimization}.o")))
                    .output()
                    .unwrap(),
                &format!("cross-compile mqtt_checks for {target} at O{optimization}"),
            );
        }
    }
}

fn read_packet(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut first = [0_u8; 1];
    stream.read_exact(&mut first).unwrap();
    let mut remaining = 0_usize;
    let mut scale = 1_usize;
    loop {
        let mut digit = [0_u8; 1];
        stream.read_exact(&mut digit).unwrap();
        remaining += (digit[0] & 127) as usize * scale;
        if digit[0] & 128 == 0 {
            break;
        }
        scale *= 128;
    }
    let mut body = vec![0_u8; remaining];
    stream.read_exact(&mut body).unwrap();
    (first[0], body)
}
// Keep-alive pings reset read timeouts, so the script also has an overall deadline.
const SCRIPT_LIMIT: Duration = Duration::from_secs(30);
// The next packet that is not a keep-alive ping; pings are answered and counted.
fn next(stream: &mut TcpStream, pings: &mut (usize, Instant)) -> (u8, Vec<u8>) {
    loop {
        assert!(pings.1.elapsed() < SCRIPT_LIMIT, "broker script timed out");
        let (header, body) = read_packet(stream);
        if header == 0xc0 {
            assert!(body.is_empty());
            stream.write_all(&[0xd0, 0]).unwrap();
            pings.0 += 1;
            continue;
        }
        return (header, body);
    }
}
fn string(data: &[u8]) -> Vec<u8> {
    let mut out = vec![(data.len() >> 8) as u8, data.len() as u8];
    out.extend_from_slice(data);
    out
}
fn publish(header: u8, topic: &[u8], id: Option<u16>, payload: &[u8]) -> Vec<u8> {
    let mut body = string(topic);
    if let Some(id) = id {
        body.extend_from_slice(&id.to_be_bytes());
    }
    body.extend_from_slice(payload);
    assert!(body.len() < 128);
    let mut frame = vec![header, body.len() as u8];
    frame.extend_from_slice(&body);
    frame
}
fn accept(listener: &TcpListener) -> TcpStream {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
}
fn broker(refusing: TcpListener, listener: TcpListener) {
    let mut pings = (0, Instant::now());
    let mut stream = accept(&refusing);
    assert_eq!(next(&mut stream, &mut pings).0, 0x10);
    stream.write_all(&[0x20, 2, 0, 5]).unwrap();
    drop(stream);

    let mut stream = accept(&listener);
    let mut connect = string(b"MQTT");
    connect.extend_from_slice(&[4, 0xc2, 0, 1]);
    connect.extend(string(b"dodo-test"));
    connect.extend(string(b"user"));
    connect.extend(string(b"pass"));
    assert_eq!(next(&mut stream, &mut pings), (0x10, connect));
    stream.write_all(&[0x20, 2, 0, 0]).unwrap();

    let mut subscribe = vec![0, 1];
    subscribe.extend(string(b"sensors/#"));
    subscribe.push(2);
    assert_eq!(next(&mut stream, &mut pings), (0x82, subscribe));
    stream.write_all(&[0x90, 3, 0, 1, 1]).unwrap();

    let mut zero = string(b"sensors/a");
    zero.extend_from_slice(b"zero");
    assert_eq!(next(&mut stream, &mut pings), (0x30, zero));

    let mut one = string(b"sensors/b");
    one.extend_from_slice(&[0, 2]);
    one.extend_from_slice(b"one");
    assert_eq!(next(&mut stream, &mut pings), (0x32, one));
    // A delivery overtakes the acknowledgement the client is waiting for.
    stream
        .write_all(&publish(0x32, b"sensors/x", Some(7), b"held"))
        .unwrap();
    stream.write_all(&[0x40, 2, 0, 2]).unwrap();

    let mut three = string(b"sensors/c");
    three.extend_from_slice(&[0, 3]);
    three.extend_from_slice(b"three");
    assert_eq!(next(&mut stream, &mut pings), (0x35, three));
    stream.write_all(&[0x50, 2, 0, 3]).unwrap();
    assert_eq!(next(&mut stream, &mut pings), (0x62, vec![0, 3]));
    stream.write_all(&[0x70, 2, 0, 3]).unwrap();

    // sensors/x is acknowledged once the client moves on from it.
    assert_eq!(next(&mut stream, &mut pings), (0x40, vec![0, 7]));
    stream
        .write_all(&publish(0x34, b"sensors/y", Some(8), b"two"))
        .unwrap();
    assert_eq!(next(&mut stream, &mut pings), (0x50, vec![0, 8]));
    stream.write_all(&[0x62, 2, 0, 8]).unwrap();
    assert_eq!(next(&mut stream, &mut pings), (0x70, vec![0, 8]));

    let mut unsubscribe = vec![0, 4];
    unsubscribe.extend(string(b"sensors/#"));
    assert_eq!(next(&mut stream, &mut pings), (0xa2, unsubscribe));
    assert!(pings.0 >= 1, "an idle client must ping");
    stream.write_all(&[0xb0, 2, 0, 4]).unwrap();

    let mut denied = vec![0, 5];
    denied.extend(string(b"denied/#"));
    denied.push(0);
    assert_eq!(next(&mut stream, &mut pings), (0x82, denied));
    stream.write_all(&[0x90, 3, 0, 5, 0x80]).unwrap();

    let mut big = string(b"big");
    big.extend(std::iter::repeat_n(b'x', 6000));
    assert_eq!(next(&mut stream, &mut pings), (0x30, big));
    assert_eq!(next(&mut stream, &mut pings), (0xe0, vec![]));
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
}

#[test]
fn hosted_client_runs_a_session_against_a_loopback_broker() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let template = fs::read_to_string(root.join("tests/mqtt/client.dodo")).unwrap();
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let refusing = TcpListener::bind("127.0.0.1:0").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let source = workspace.0.join(format!("client-{optimization}.dodo"));
        fs::write(
            &source,
            template
                .replace(
                    "mqtt://REFUSING",
                    &format!("mqtt://{}", refusing.local_addr().unwrap()),
                )
                .replace(
                    "mqtt://BROKER",
                    &format!("mqtt://{}", listener.local_addr().unwrap()),
                ),
        )
        .unwrap();
        let binary = workspace.0.join(format!("client-{optimization}"));
        compile(&source, &binary, optimization);
        let peer = std::thread::spawn(move || broker(refusing, listener));
        let mut child = Command::new(&binary)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > SCRIPT_LIMIT + Duration::from_secs(10) {
                child.kill().unwrap();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        success(
            child.wait_with_output().unwrap(),
            &format!("MQTT session at O{optimization}"),
        );
        peer.join().unwrap();
    }
}

#[test]
fn hosted_packages_cross_compile_for_windows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for fixture in [
        "tests/mqtt/client",
        "tests/mqtt/broker",
        "tests/mqtt/echo",
        "tests/mqtt/in_process",
        "examples/mqtt_broker",
        "examples/mqtt_lamp",
    ] {
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join(format!("{fixture}.dodo")))
                .args(["--target", "x86_64-pc-windows-msvc", "--emit", "obj", "-o"])
                .arg(
                    workspace
                        .0
                        .join(format!("{}.obj", fixture.replace('/', "-"))),
                )
                .output()
                .unwrap(),
            &format!("cross-compile {fixture}.dodo for Windows"),
        );
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn remaining(mut length: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut digit = (length % 128) as u8;
        length /= 128;
        if length != 0 {
            digit |= 128;
        }
        out.push(digit);
        if length == 0 {
            return out;
        }
    }
}
// A raw MQTT client for asserting exact broker behavior.
struct Peer(TcpStream);
impl Peer {
    fn open(port: u16) -> Peer {
        let started = Instant::now();
        loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    return Peer(stream);
                }
                Err(_) if started.elapsed() < Duration::from_secs(20) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("connect to broker: {error}"),
            }
        }
    }
    fn send(&mut self, header: u8, body: &[u8]) {
        let mut frame = vec![header];
        frame.extend(remaining(body.len()));
        frame.extend_from_slice(body);
        self.0.write_all(&frame).unwrap();
    }
    fn read(&mut self) -> (u8, Vec<u8>) {
        read_packet(&mut self.0)
    }
    fn closed(&mut self) -> bool {
        let mut byte = [0_u8; 1];
        matches!(self.0.read(&mut byte), Ok(0) | Err(_))
    }
    fn connect(
        port: u16,
        id: &[u8],
        login: bool,
        will: Option<(&[u8], &[u8])>,
        level: u8,
    ) -> (Peer, u8) {
        let mut peer = Peer::open(port);
        let mut body = string(b"MQTT");
        let mut flags = 2;
        if will.is_some() {
            flags |= 4;
        }
        if login {
            flags |= 192;
        }
        body.extend_from_slice(&[level, flags, 0, 60]);
        body.extend(string(id));
        if let Some((topic, payload)) = will {
            body.extend(string(topic));
            body.extend(string(payload));
        }
        if login {
            body.extend(string(b"family"));
            body.extend(string(b"secret"));
        }
        peer.send(0x10, &body);
        let (header, reply) = peer.read();
        assert_eq!(header, 0x20);
        assert_eq!(reply[0], 0);
        (peer, reply[1])
    }
    fn login(port: u16, id: &[u8]) -> Peer {
        let (peer, code) = Peer::connect(port, id, true, None, 4);
        assert_eq!(code, 0);
        peer
    }
    fn subscribe(&mut self, id: u16, filters: &[(&[u8], u8)]) -> Vec<u8> {
        let mut body = id.to_be_bytes().to_vec();
        for (filter, qos) in filters {
            body.extend(string(filter));
            body.push(*qos);
        }
        self.send(0x82, &body);
        let (header, reply) = self.read();
        assert_eq!(header, 0x90);
        assert_eq!(&reply[..2], &id.to_be_bytes());
        reply[2..].to_vec()
    }
    fn publish(&mut self, header: u8, topic: &[u8], id: Option<u16>, payload: &[u8]) {
        let mut body = string(topic);
        if let Some(id) = id {
            body.extend_from_slice(&id.to_be_bytes());
        }
        body.extend_from_slice(payload);
        self.send(header, &body);
    }
}
fn delivery(topic: &[u8], id: Option<u16>, payload: &[u8]) -> Vec<u8> {
    let mut body = string(topic);
    if let Some(id) = id {
        body.extend_from_slice(&id.to_be_bytes());
    }
    body.extend_from_slice(payload);
    body
}
// Kills the child if a failed assertion unwinds before finish().
struct Running(Option<std::process::Child>);
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn spawn_dodo(binary: &Path) -> Running {
    Running(Some(
        Command::new(binary)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ))
}
fn finish(mut running: Running, context: &str) {
    let mut child = running.0.take().unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > SCRIPT_LIMIT {
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    success(child.wait_with_output().unwrap(), context);
}
fn build_broker(
    workspace: &Workspace,
    port: u16,
    connections: usize,
    optimization: &str,
) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = workspace.0.join(format!("broker-{port}.dodo"));
    fs::write(
        &source,
        fs::read_to_string(root.join("tests/mqtt/broker.dodo"))
            .unwrap()
            .replace(
                ".max_connections(7)",
                &format!(".max_connections({connections})"),
            )
            .replace("127.0.0.1:1883", &format!("127.0.0.1:{port}")),
    )
    .unwrap();
    let binary = workspace.0.join(format!("broker-{port}"));
    compile(&source, &binary, optimization);
    binary
}

#[test]
fn app_and_broker_handlers_test_in_process() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let binary = workspace.0.join(format!("in-process-{optimization}"));
        compile(
            &root.join("tests/mqtt/in_process.dodo"),
            &binary,
            optimization,
        );
        success(
            Command::new(&binary).output().unwrap(),
            &format!("in-process app and broker tests at O{optimization}"),
        );
    }
}

#[test]
fn broker_routes_retains_and_cleans_up_sessions() {
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let port = free_port();
        let binary = build_broker(&workspace, port, 7, optimization);
        let broker = spawn_dodo(&binary);

        let (mut refused, code) = Peer::connect(port, b"intruder", false, None, 4);
        assert_eq!(
            code, 5,
            "a broker with a password refuses anonymous clients"
        );
        assert!(refused.closed());

        let (mut s, code) = Peer::connect(port, b"s", true, Some((b"status/s", b"gone")), 4);
        assert_eq!(code, 0);
        assert_eq!(
            s.subscribe(1, &[(b"house/#", 1), (b"bad/#/x", 0)]),
            vec![1, 0x80]
        );
        let mut p = Peer::login(port, b"p");

        p.publish(0x31, b"house/temp", None, b"21");
        assert_eq!(s.read(), (0x30, delivery(b"house/temp", None, b"21")));

        // QoS 1 in, then the server handler's announcement.
        p.publish(0x32, b"house/doorbell", Some(5), b"ring");
        assert_eq!(p.read(), (0x40, vec![0, 5]));
        assert_eq!(
            s.read(),
            (0x32, delivery(b"house/doorbell", Some(1), b"ring"))
        );
        s.send(0x40, &[0, 1]);
        assert_eq!(s.read(), (0x30, delivery(b"house/announce", None, b"ring")));

        // QoS 2 in; delivered at the granted QoS 1.
        p.publish(0x34, b"house/light", Some(6), b"on");
        assert_eq!(p.read(), (0x50, vec![0, 6]));
        p.send(0x62, &[0, 6]);
        assert_eq!(p.read(), (0x70, vec![0, 6]));
        assert_eq!(s.read(), (0x32, delivery(b"house/light", Some(2), b"on")));
        s.send(0x40, &[0, 2]);

        // A new subscriber receives the retained value with the retain flag.
        let mut s2 = Peer::login(port, b"s2");
        assert_eq!(s2.subscribe(3, &[(b"house/temp", 0)]), vec![0]);
        assert_eq!(s2.read(), (0x31, delivery(b"house/temp", None, b"21")));

        // An abrupt disconnect publishes the will.
        assert_eq!(s2.subscribe(4, &[(b"status/#", 1)]), vec![1]);
        drop(s);
        assert_eq!(s2.read(), (0x30, delivery(b"status/s", None, b"gone")));

        // The same client ID takes the session over.
        let mut p2 = Peer::login(port, b"p");
        assert!(p.closed());

        s2.send(0xc0, &[]);
        assert_eq!(s2.read(), (0xd0, vec![]));
        s2.send(0xa2, &[&[0_u8, 9][..], &string(b"status/#")].concat());
        assert_eq!(s2.read(), (0xb0, vec![0, 9]));
        p2.publish(0x30, b"status/x", None, b"unseen");
        p2.publish(0x31, b"house/temp", None, b"22");
        assert_eq!(s2.read(), (0x30, delivery(b"house/temp", None, b"22")));

        let (mut old, code) = Peer::connect(port, b"old", false, None, 3);
        assert_eq!(code, 1, "only MQTT 3.1.1 is accepted");
        assert!(old.closed());
        let (mut anonymous, code) = Peer::connect(port, b"", true, None, 4);
        assert_eq!(code, 0, "an empty client ID gets a generated one");

        for peer in [&mut p2, &mut s2, &mut anonymous] {
            peer.send(0xe0, &[]);
        }
        drop((p2, s2, anonymous));
        finish(broker, &format!("broker at O{optimization}"));
    }
}

#[test]
fn app_runs_against_the_dodo_broker() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let port = free_port();
        let broker = spawn_dodo(&build_broker(&workspace, port, 3, optimization));
        let source = workspace.0.join(format!("echo-{optimization}.dodo"));
        fs::write(
            &source,
            fs::read_to_string(root.join("tests/mqtt/echo.dodo"))
                .unwrap()
                .replace("BROKER", &format!("mqtt://127.0.0.1:{port}")),
        )
        .unwrap();
        let binary = workspace.0.join(format!("echo-{optimization}"));
        compile(&source, &binary, optimization);

        let mut watcher = Peer::login(port, b"watcher");
        assert_eq!(
            watcher.subscribe(1, &[(b"apps/#", 1), (b"pong", 0)]),
            vec![1, 0]
        );
        let app = spawn_dodo(&binary);
        let (header, body) = watcher.read();
        assert_eq!(
            (header & 0xf6, &body[..2 + 16]),
            (0x32, &string(b"apps/echo/status")[..])
        );
        assert!(body.ends_with(b"online"));
        assert_eq!(
            watcher.read(),
            (0x30, delivery(b"apps/echo/tick", None, b"1"))
        );
        watcher.publish(0x30, b"ping", None, b"");
        assert_eq!(watcher.read(), (0x30, delivery(b"pong", None, b"hello")));
        let (header, body) = watcher.read();
        assert_eq!(header & 0xf6, 0x32);
        assert!(body.ends_with(b"offline"));
        finish(app, &format!("echo app at O{optimization}"));

        // The status stays retained for later subscribers.
        let mut later = Peer::login(port, b"later");
        assert_eq!(later.subscribe(1, &[(b"apps/echo/status", 0)]), vec![0]);
        assert_eq!(
            later.read(),
            (0x31, delivery(b"apps/echo/status", None, b"offline"))
        );
        watcher.send(0xe0, &[]);
        later.send(0xe0, &[]);
        drop((watcher, later));
        finish(broker, &format!("broker at O{optimization}"));
    }
}
