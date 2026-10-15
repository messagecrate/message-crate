//! The pieces of the health check: finding the server's process, reading
//! where it listens from `/proc`, and asking `/health` there.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;

use super::*;

/// An address as `/proc/net/tcp` writes it: each 32-bit word of `bytes`, in
/// network order, printed in the host's byte order.
fn proc_hex(bytes: &[u8]) -> String {
    bytes
        .chunks(4)
        .map(|word| format!("{:08X}", u32::from_ne_bytes(word.try_into().unwrap())))
        .collect()
}

#[test]
fn the_ipv4_wildcard_is_asked_on_loopback() {
    assert_eq!(
        hosts_of(&proc_hex(&[0, 0, 0, 0])),
        Some(vec!["127.0.0.1".to_owned()])
    );
}

#[test]
fn an_ipv4_address_is_asked_where_it_is() {
    assert_eq!(
        hosts_of(&proc_hex(&[172, 17, 0, 2])),
        Some(vec!["172.17.0.2".to_owned()])
    );
}

#[test]
fn the_ipv6_wildcard_is_asked_on_ipv4_loopback_first() {
    assert_eq!(
        hosts_of(&proc_hex(&[0; 16])),
        Some(vec!["127.0.0.1".to_owned(), "[::1]".to_owned()])
    );
}

#[test]
fn an_ipv4_mapped_address_is_asked_as_ipv4() {
    let mut mapped = [0u8; 16];
    mapped[10] = 0xff;
    mapped[11] = 0xff;
    mapped[12..].copy_from_slice(&[10, 0, 0, 5]);
    assert_eq!(
        hosts_of(&proc_hex(&mapped)),
        Some(vec!["10.0.0.5".to_owned()])
    );
}

#[test]
fn an_ipv6_address_is_asked_in_brackets() {
    let mut loopback = [0u8; 16];
    loopback[15] = 1;
    assert_eq!(
        hosts_of(&proc_hex(&loopback)),
        Some(vec!["[::1]".to_owned()])
    );
}

#[test]
fn only_a_listening_row_on_one_of_the_process_sockets_counts() {
    let wildcard = proc_hex(&[0, 0, 0, 0]);
    let header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";
    let row = |port: u16, state: &str, inode: &str| {
        format!(
            "   0: {wildcard}:{port:04X} 00000000:0000 {state} 00000000:00000000 00:00000000 00000000  1000        0 {inode} 1 0000000000000000 100 0 0 10 0"
        )
    };
    let table = [
        header.to_owned(),
        // Another process's listener in the same network namespace.
        row(9000, "0A", "111"),
        // A connection of the server's, not a listener.
        row(8081, "01", "222"),
        row(8080, "0A", "333"),
    ]
    .join("\n");
    let inodes: HashSet<String> = ["222", "333"].into_iter().map(str::to_owned).collect();
    assert_eq!(
        listening_in(&table, &inodes),
        vec![vec!["127.0.0.1:8080".to_owned()]]
    );
}

/// A process directory under `proc` whose command line is `args`.
fn fake_process(proc: &Path, pid: u32, args: &[&str]) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    let mut cmdline = args.join("\0");
    cmdline.push('\0');
    fs::write(dir.join("cmdline"), cmdline).unwrap();
}

#[test]
fn the_oldest_server_process_other_than_the_check_is_the_container_command() {
    let proc = tempfile::tempdir().unwrap();
    fake_process(
        proc.path(),
        1,
        &["tini", "--", "/usr/local/bin/docker-entrypoint-release.sh"],
    );
    fake_process(proc.path(), 3, &["message-crate-server", "healthcheck"]);
    fake_process(
        proc.path(),
        7,
        &[
            "/usr/local/bin/message-crate-server",
            "serve",
            "--config",
            "config/config.toml",
        ],
    );
    fake_process(proc.path(), 12, &["message-crate-server", "reset-demo"]);
    fs::create_dir_all(proc.path().join("net")).unwrap();

    let (pid, args) = server_process(proc.path(), 3).unwrap().unwrap();
    assert_eq!(pid, 7);
    assert_eq!(args[1], "serve");
}

#[test]
fn the_check_alone_is_no_server() {
    let proc = tempfile::tempdir().unwrap();
    fake_process(proc.path(), 1, &["bash"]);
    fake_process(proc.path(), 40, &["message-crate-server", "healthcheck"]);

    assert_eq!(server_process(proc.path(), 40).unwrap(), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_listening_socket_of_this_process_is_found_in_proc() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let process = Path::new("/proc").join(std::process::id().to_string());
    let inodes = socket_inodes(&process);
    let table = fs::read_to_string(process.join("net/tcp")).unwrap();

    assert!(
        listening_in(&table, &inodes).contains(&vec![format!("127.0.0.1:{port}")]),
        "127.0.0.1:{port} is not among this process's listeners"
    );
}

/// A server on a free loopback port whose `/health` answers `status`, and the
/// address to ask it at.
async fn health_server(status: StatusCode) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let app = Router::new().route("/health", get(move || async move { (status, "answer\n") }));
    crate::request_id::spawn_in_current_span(async move { axum::serve(listener, app).await });
    address
}

#[tokio::test]
async fn a_server_that_answers_ok_is_healthy() {
    let address = health_server(StatusCode::OK).await;
    let outcome = probe(std::slice::from_ref(&address)).await;
    assert_eq!(
        outcome,
        Outcome::healthy(format!("http://{address}/health answered 200"))
    );
}

#[tokio::test]
async fn a_server_that_cannot_reach_its_database_is_unhealthy() {
    let address = health_server(StatusCode::INTERNAL_SERVER_ERROR).await;
    let outcome = probe(std::slice::from_ref(&address)).await;
    assert_eq!(
        outcome,
        Outcome::unhealthy(format!("http://{address}/health answered 500"))
    );
}

#[tokio::test]
async fn the_next_address_is_asked_when_one_fails() {
    let failing = health_server(StatusCode::INTERNAL_SERVER_ERROR).await;
    let answering = health_server(StatusCode::OK).await;
    let outcome = probe(&[failing, answering.clone()]).await;
    assert_eq!(
        outcome,
        Outcome::healthy(format!("http://{answering}/health answered 200"))
    );
}
