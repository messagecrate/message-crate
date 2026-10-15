//! `message-crate-server healthcheck`: the release image's Docker
//! `HEALTHCHECK` (`docker/Dockerfile`).
//!
//! It asks `/health` at the address the container's server listens on, read
//! from `/proc`, so the check follows the server wherever `[server] bind` or
//! `serve --bind` puts it and needs no edit when the port changes. `/health`
//! answers ok only while the server can take a database connection and run a
//! statement on it, so a server that runs but cannot reach its database reads
//! unhealthy. It is part of the server binary, so the image needs neither
//! Node.js nor curl (#2065).
//!
//! The container's command is the oldest `message-crate-server` process other
//! than this one. When that command is not `serve` (`docker compose run --rm
//! server reset-demo`, `create-owner`), or the container runs no other
//! `message-crate-server` at all (a shell started with `--entrypoint bash`),
//! nothing is meant to answer, so the check passes instead of reading
//! unhealthy once the start period ends. A serving container whose server has
//! died has already exited, because tini is PID 1 and the entrypoint execs the
//! server, so a missing server never means one that stopped answering. A
//! `serve` that is not listening yet, while it builds the Demo Account on a
//! first start, fails the check. A failure inside `--start-period` does not
//! count.
//!
//! It reads Linux's `/proc`, so it works only on Linux, where the image runs;
//! elsewhere it fails because `/proc` cannot be read.

use std::collections::HashSet;
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::time::Duration;

use http_body_util::Empty;
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;

const SERVER: &str = "message-crate-server";

/// The `st` column of `/proc/net/tcp` and `tcp6` holds the kernel's TCP state
/// as hex. `0A` is `TCP_LISTEN`.
const TCP_STATE_LISTEN: &str = "0A";

/// The hosts asked share this time between them. It stays under `HEALTHCHECK
/// --timeout=5s` in `docker/Dockerfile`, with a second left for the process to
/// start and read `/proc`. The server gives up on its database within
/// `HEALTH_DATABASE_LIMIT` (`server.rs`), 1.5 seconds, which stays under the
/// two seconds each host gets when two are asked, so a failure arrives as the
/// server's answer rather than as this check's own time running out.
const PROBE_BUDGET: Duration = Duration::from_secs(4);

/// What the check found: healthy or not, and the line it prints. Docker shows
/// that line in `docker inspect --format '{{json .State.Health}}'`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) healthy: bool,
    pub(crate) message: String,
}

impl Outcome {
    fn healthy(message: String) -> Self {
        Self {
            healthy: true,
            message,
        }
    }

    fn unhealthy(message: String) -> Self {
        Self {
            healthy: false,
            message,
        }
    }
}

/// Check the server this container runs, as the module documentation says.
pub(crate) async fn check() -> Outcome {
    let proc = Path::new("/proc");
    let server = match server_process(proc, std::process::id()) {
        Ok(server) => server,
        Err(e) => return Outcome::unhealthy(format!("cannot read /proc: {e}")),
    };
    let Some((pid, args)) = server else {
        return Outcome::healthy(format!(
            "no {SERVER} process runs, so nothing serves to check"
        ));
    };
    let command = args.get(1).map_or("", String::as_str);
    if command != "serve" {
        return Outcome::healthy(format!(
            "the container runs `{SERVER} {command}`, which serves nothing to check"
        ));
    }
    let addresses = listen_addresses(&proc.join(pid.to_string()));
    if addresses.is_empty() {
        return Outcome::unhealthy(format!("{SERVER} serve is not listening yet"));
    }
    probe(&addresses).await
}

/// Ask `/health` at each address in turn, and pass on the first that answers
/// with a success status. `addresses` is `host:port`, with an IPv6 host in
/// brackets.
pub(crate) async fn probe(addresses: &[String]) -> Outcome {
    let each = PROBE_BUDGET / u32::try_from(addresses.len().max(1)).unwrap_or(u32::MAX);
    let mut failures = Vec::new();
    for address in addresses {
        let url = format!("http://{address}/health");
        match tokio::time::timeout(each, get_health(address)).await {
            Ok(Ok(status)) if status.is_success() => {
                return Outcome::healthy(format!("{url} answered {}", status.as_u16()));
            }
            Ok(Ok(status)) => failures.push(format!("{url} answered {}", status.as_u16())),
            Ok(Err(e)) => failures.push(format!("{url}: {e}")),
            Err(_) => failures.push(format!("{url}: no answer within {} ms", each.as_millis())),
        }
    }
    Outcome::unhealthy(failures.join("; "))
}

/// `GET /health` at `address` over one HTTP/1.1 connection, and the status
/// it answered.
async fn get_health(address: &str) -> anyhow::Result<hyper::StatusCode> {
    let stream = tokio::net::TcpStream::connect(address.replace(['[', ']'], "")).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    let driver = crate::request_id::spawn_in_current_span(connection);
    let request = hyper::Request::get("/health")
        .header(hyper::header::HOST, address)
        .body(Empty::<Bytes>::new())?;
    let response = sender.send_request(request).await;
    driver.abort();
    Ok(response?.status())
}

/// The oldest `message-crate-server` process under `proc` other than `own`,
/// with its arguments. The lowest id is the oldest, since a container's ids
/// never wrap.
fn server_process(proc: &Path, own: u32) -> std::io::Result<Option<(u32, Vec<String>)>> {
    let mut pids: Vec<u32> = fs::read_dir(proc)?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .filter(|&pid| pid != own)
        .collect();
    pids.sort_unstable();
    for pid in pids {
        let args = args_of(&proc.join(pid.to_string()));
        let is_server = args.first().is_some_and(|program| {
            Path::new(program)
                .file_name()
                .and_then(|name| name.to_str())
                == Some(SERVER)
        });
        if is_server {
            return Ok(Some((pid, args)));
        }
    }
    Ok(None)
}

/// The arguments in `<process>/cmdline`, or none when the process has gone or
/// cannot be read.
fn args_of(process: &Path) -> Vec<String> {
    fs::read(process.join("cmdline"))
        .map(|bytes| {
            bytes
                .split(|&b| b == 0)
                .filter(|arg| !arg.is_empty())
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The inodes of the sockets the process at `process` has open.
fn socket_inodes(process: &Path) -> HashSet<String> {
    let Ok(entries) = fs::read_dir(process.join("fd")) else {
        return HashSet::new();
    };
    entries
        .filter_map(|entry| {
            // A descriptor that closed between the listing and the read is
            // skipped.
            let target = fs::read_link(entry.ok()?.path()).ok()?;
            let target = target.to_str()?;
            Some(
                target
                    .strip_prefix("socket:[")?
                    .strip_suffix(']')?
                    .to_owned(),
            )
        })
        .collect()
}

/// The addresses to ask for the first socket the process at `process` listens
/// on, read from its `net/tcp` and `net/tcp6`.
fn listen_addresses(process: &Path) -> Vec<String> {
    let inodes = socket_inodes(process);
    for table in ["tcp", "tcp6"] {
        let Ok(text) = fs::read_to_string(process.join("net").join(table)) else {
            continue;
        };
        if let Some(addresses) = listening_in(&text, &inodes).into_iter().next() {
            return addresses;
        }
    }
    Vec::new()
}

/// For each row of a `/proc/net/tcp` or `tcp6` table that listens on a socket
/// in `inodes`, the addresses to ask, in the table's order.
fn listening_in(table: &str, inodes: &HashSet<String>) -> Vec<Vec<String>> {
    table
        .lines()
        .skip(1)
        .filter_map(|row| {
            let cols: Vec<&str> = row.split_whitespace().collect();
            let (address_hex, port_hex) = cols.get(1)?.split_once(':')?;
            if *cols.get(3)? != TCP_STATE_LISTEN || !inodes.contains(*cols.get(9)?) {
                return None;
            }
            let port = u16::from_str_radix(port_hex, 16).ok()?;
            let hosts = hosts_of(address_hex)?;
            Some(
                hosts
                    .into_iter()
                    .map(|host| format!("{host}:{port}"))
                    .collect(),
            )
        })
        .collect()
}

/// The hosts to ask for a listening address as `/proc/net/tcp` writes it, in
/// order: hex 32-bit words in the host's byte order. A dual-stack socket on
/// the IPv6 wildcard answers on 127.0.0.1 too, and ::1 does not exist when the
/// container has IPv6 turned off, so 127.0.0.1 is asked first.
fn hosts_of(hex: &str) -> Option<Vec<String>> {
    let mut bytes = Vec::with_capacity(16);
    for i in (0..hex.len()).step_by(8) {
        let word = u32::from_str_radix(hex.get(i..i + 8)?, 16).ok()?;
        bytes.extend_from_slice(&word.to_ne_bytes());
    }
    if let Ok(v4) = <[u8; 4]>::try_from(bytes.as_slice()) {
        let v4 = Ipv4Addr::from(v4);
        let v4 = if v4.is_unspecified() {
            Ipv4Addr::LOCALHOST
        } else {
            v4
        };
        return Some(vec![v4.to_string()]);
    }
    let v6 = Ipv6Addr::from(<[u8; 16]>::try_from(bytes.as_slice()).ok()?);
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(vec![v4.to_string()]);
    }
    if v6.is_unspecified() {
        return Some(vec![Ipv4Addr::LOCALHOST.to_string(), "[::1]".to_owned()]);
    }
    Some(vec![format!("[{v6}]")])
}

#[cfg(test)]
mod tests;
