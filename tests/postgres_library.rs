//! std/db/postgres: server-free protocol checks, and checks against a real
//! PostgreSQL started in Docker with TLS and one role per authentication
//! method. The live part is skipped when Docker, the `postgres:17-alpine`
//! image (or network access to pull it), or the `openssl` command is
//! unavailable, or when DODO_SKIP_DOCKER is set.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const IMAGE: &str = "postgres:17-alpine";

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-postgres-{}-{}",
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

struct Container(String);
impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn quiet(command: &mut Command) -> bool {
    command.output().is_ok_and(|output| output.status.success())
}

#[test]
fn protocol_checks_pass() {
    let workspace = Workspace::new();
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("test")
            .arg(root().join("tests/postgres/protocol_checks.dodo"))
            .current_dir(&workspace.0)
            .output()
            .unwrap(),
        "dodo test tests/postgres/protocol_checks.dodo",
    );
}

fn build_and_run(source: &Path, workspace: &Path, level: &str, context: &str) {
    let executable = workspace.join(format!(
        "{}-O{level}",
        source.file_stem().unwrap().to_string_lossy()
    ));
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", level, "-o"])
            .arg(&executable)
            .output()
            .unwrap(),
        &format!("build {context}"),
    );
    success(
        Command::new(&executable)
            .current_dir(workspace)
            .output()
            .unwrap(),
        &format!("run {context} at -O{level}"),
    );
}

/// SCRAM-SHA-256 and md5 with OpenSSL's hashes against reference values.
#[test]
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn authentication_matches_reference_values() {
    let workspace = Workspace::new();
    for level in ["0", "3"] {
        build_and_run(
            &root().join("tests/postgres/auth_checks.dodo"),
            &workspace.0,
            level,
            "the authentication checks",
        );
    }
}

// A private CA and a server certificate that names only `localhost`, so the
// checks can tell verify-ca from verify-full by connecting to 127.0.0.1.
fn credentials(directory: &Path) -> bool {
    fs::write(
        directory.join("extensions.cnf"),
        "subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\n",
    )
    .unwrap();
    let commands: [&[&str]; 3] = [
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-subj",
            "/CN=Dodo PostgreSQL test CA",
            "-days",
            "2",
        ],
        &[
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "server.key",
            "-out",
            "server.csr",
            "-subj",
            "/CN=localhost",
        ],
        &[
            "x509",
            "-req",
            "-in",
            "server.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-out",
            "server.crt",
            "-days",
            "2",
            "-extfile",
            "extensions.cnf",
        ],
    ];
    for arguments in commands {
        if !quiet(
            Command::new("openssl")
                .args(arguments)
                .current_dir(directory),
        ) {
            return false;
        }
    }
    true
}

const INIT: &str = r#"#!/bin/sh
set -e
cp /certs/server.crt /certs/server.key "$PGDATA/"
chmod 600 "$PGDATA/server.key"
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<'SQL'
ALTER SYSTEM SET ssl = on;
SET password_encryption = 'md5';
CREATE ROLE md5user LOGIN PASSWORD 'md5pass';
SET password_encryption = 'scram-sha-256';
CREATE ROLE clearuser LOGIN PASSWORD 'clear@pass';
CREATE ROLE trustuser LOGIN;
CREATE ROLE tlsuser LOGIN PASSWORD 'tlspass';
SQL
# The image trusts loopback connections; require passwords from every
# address so the checks mean the same wherever the client runs.
sed -i 's/^\(host.*\)trust$/\1scram-sha-256/' "$PGDATA/pg_hba.conf"
{
  echo "host all md5user all md5"
  echo "host all clearuser all password"
  echo "host all trustuser all trust"
  echo "hostnossl all tlsuser all reject"
  cat "$PGDATA/pg_hba.conf"
} > /tmp/hba
cp /tmp/hba "$PGDATA/pg_hba.conf"
"#;

fn start_server(directory: &Path) -> Option<(Container, u16)> {
    if std::env::var_os("DODO_SKIP_DOCKER").is_some() || !quiet(Command::new("docker").arg("info"))
    {
        return None;
    }
    if !quiet(Command::new("docker").args(["image", "inspect", IMAGE]))
        && !quiet(Command::new("docker").args(["pull", IMAGE]))
    {
        return None;
    }
    if !credentials(directory) {
        return None;
    }
    let init = directory.join("init.sh");
    fs::write(&init, INIT).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The container's postgres user copies the key; the copy is 0600.
        for (file, mode) in [("init.sh", 0o755), ("server.key", 0o644)] {
            fs::set_permissions(directory.join(file), fs::Permissions::from_mode(mode)).unwrap();
        }
    }
    let name = format!(
        "dodo-postgres-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let started = Command::new("docker")
        .args(["run", "-d", "--name", &name, "-p", "127.0.0.1::5432"])
        .args([
            "-e",
            "POSTGRES_USER=dodo",
            "-e",
            "POSTGRES_PASSWORD=secret",
            "-e",
            "POSTGRES_DB=dodo",
        ])
        .arg("-v")
        .arg(format!("{}:/certs:ro", directory.display()))
        .arg("-v")
        .arg(format!(
            "{}:/docker-entrypoint-initdb.d/init.sh:ro",
            init.display()
        ))
        .arg(IMAGE)
        .output()
        .unwrap();
    success(started, "start the PostgreSQL container");
    let container = Container(name.clone());
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        // The entrypoint runs initialization against a socket-only server;
        // TCP answers once the final server is up.
        let logs = Command::new("docker")
            .args(["logs", &name])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&logs.stderr).into_owned()
            + &String::from_utf8_lossy(&logs.stdout);
        if text.contains("init process complete")
            && quiet(Command::new("docker").args([
                "exec",
                &name,
                "pg_isready",
                "-h",
                "127.0.0.1",
                "-U",
                "dodo",
            ]))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "PostgreSQL did not start:\n{text}"
        );
        sleep(Duration::from_millis(250));
    }
    let port = Command::new("docker")
        .args(["port", &name, "5432/tcp"])
        .output()
        .unwrap();
    let mapping = String::from_utf8_lossy(&port.stdout).into_owned();
    let port = mapping
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .and_then(|port| port.trim().parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no published port in {mapping:?}"));
    Some((container, port))
}

#[test]
fn live_server_checks_pass() {
    let workspace = Workspace::new();
    let Some((_container, port)) = start_server(&workspace.0) else {
        eprintln!("skipped: Docker, the {IMAGE} image, or the openssl command is unavailable");
        return;
    };
    let ca = fs::read_to_string(workspace.0.join("ca.pem")).unwrap();
    let source = fs::read_to_string(root().join("tests/postgres/live_checks.dodo"))
        .unwrap()
        .replace(":55432/", &format!(":{port}/"))
        .replace(
            "@@CA@@",
            &ca.replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('"', "\\\""),
        );
    let checks = workspace.0.join("live_checks.dodo");
    fs::write(&checks, source).unwrap();
    for level in ["0", "3"] {
        build_and_run(&checks, &workspace.0, level, "the live PostgreSQL checks");
    }
}
