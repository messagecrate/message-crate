//! What the server's log holds after a real `serve` process has handled an
//! owner, an account, an API token, an Import Run, an attachment, a media
//! link and searches: the requests, and never a password, a session or API
//! token (hashed or not), message text, attachment bytes, or a contact's
//! name or identities (`docs/architecture/server-log.md`, ADR 0008).
//!
//! It runs the binary, as Docker and the desktop app do, so the log is the
//! one the process-wide subscriber writes, at `RUST_LOG=trace`, the most a
//! person can ask the server to write.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use message_crate_serve_protocol::LISTENING_LINE;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// How long a server on an empty database may take to listen.
const WAIT: Duration = Duration::from_secs(60);

/// The owner's password.
const OWNER_PASSWORD: &str = "Owner-Pw-7q2Lx9Vb";
/// The account's first password, and the one it changes to.
const ALICE_PASSWORD: &str = "Alice-Pw-3kN8wZr4";
const ALICE_NEW_PASSWORD: &str = "Alice-Pw-Next-5Hd2Qm";
/// What the imported message says, and the word a search looks for.
const MESSAGE_TEXT: &str = "Meet me by the Tangerine Lighthouse at nine";
const SEARCH_WORD: &str = "Tangerine";
/// The contact the import names, and their identities.
const CONTACT_NAME: &str = "Zebediah Quixote";
const CONTACT_PHONE: &str = "+15555550177";
const CONTACT_EMAIL: &str = "zebediah.quixote@example.com";
/// The attachment's bytes.
const ATTACHMENT: &[u8] = b"Attachment-Bytes-Pelican-Orchard-41";

/// A server process, killed when the test ends however it ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn server() -> Command {
    Command::new(env!("CARGO_BIN_EXE_message-crate-server"))
}

/// An empty database in `data_dir`, so `serve` listens without first
/// building the Demo Account.
fn create_database(root: &Path, data_dir: &Path) {
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[paths]\ndb = {:?}\ndata_dir = {:?}\n",
            data_dir.join("messagecrate.db"),
            data_dir
        ),
    )
    .unwrap();
    let output = server()
        .arg("create-database")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// `serve` as the desktop app starts it, and the address it listens on.
/// The server binds port 0 and its listening line names the port it got, so
/// no other process can take the port between the choice and the bind.
fn serve(data_dir: &Path, static_dir: &Path) -> (Running, String) {
    let mut child = Running(
        server()
            .arg("serve")
            .arg("--data-dir")
            .arg(data_dir)
            .arg("--bind")
            .arg("127.0.0.1:0")
            .arg("--static-dir")
            .arg(static_dir)
            .env("RUST_LOG", "trace")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = child.0.stderr.take().unwrap();
    let (lines, received) = mpsc::channel();
    // Reads until the server's output closes, so the pipe never fills.
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = lines.send(line);
        }
    });
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match received.recv_timeout(left) {
            Ok(line) => match line.strip_prefix(LISTENING_LINE) {
                Some(address) => return (child, address.to_string()),
                None => seen.push(line),
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!(
                "the server closed its output before the listening line ({}):\n{}",
                exit_status(&mut child.0, deadline),
                seen.join("\n")
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => panic!(
                "the server wrote no listening line in {WAIT:?}:\n{}",
                seen.join("\n")
            ),
        }
    }
}

/// How `child` exited, or that it is still running at `deadline`.
fn exit_status(child: &mut Child, deadline: Instant) -> String {
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return status.to_string();
        }
        thread::sleep(Duration::from_millis(50));
    }
    "still running".to_string()
}

/// One call, answered with its status and JSON body (`Null` when it has
/// none).
async fn call(
    base: &str,
    method: reqwest::Method,
    path: &str,
    token: Option<&str>,
    body: Option<(&str, Vec<u8>)>,
) -> (reqwest::StatusCode, Value) {
    let mut request = reqwest::Client::new().request(method, format!("{base}{path}"));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some((content_type, body)) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn json_body(value: &Value) -> Option<(&'static str, Vec<u8>)> {
    Some(("application/json", serde_json::to_vec(value).unwrap()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Everything in the server's log files, and how many files there are.
fn log_text(data_dir: &Path) -> (String, usize) {
    let mut text = String::new();
    let mut files = 0;
    for entry in std::fs::read_dir(data_dir.join("logs")).unwrap() {
        let path = entry.unwrap().path();
        text.push_str(&String::from_utf8_lossy(&std::fs::read(path).unwrap()));
        files += 1;
    }
    (text, files)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_server_log_never_holds_a_secret_message_text_or_a_contact() {
    use reqwest::Method;
    use reqwest::StatusCode as S;

    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let static_dir = root.path().join("static");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&static_dir).unwrap();
    create_database(root.path(), &data_dir);
    let (_server, base) = serve(&data_dir, &static_dir);
    let base = base.as_str();

    // The owner claims the Message Crate and opens registration.
    let (status, claimed) = call(
        base,
        Method::POST,
        "/v1/server/claim",
        None,
        json_body(&json!({ "username": "keeper", "password": OWNER_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{claimed}");
    let owner = claimed["token"].as_str().unwrap().to_string();
    let (status, _) = call(
        base,
        Method::PATCH,
        "/v1/server/settings",
        Some(&owner),
        json_body(&json!({ "public_registration": true })),
    )
    .await;
    assert_eq!(status, S::OK);

    // An account registers, logs in, and makes an API token.
    let (status, registered) = call(
        base,
        Method::POST,
        "/v1/accounts",
        None,
        json_body(&json!({ "username": "alice", "password": ALICE_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{registered}");
    let alice_id = registered["account_id"].as_i64().unwrap();
    let (status, session) = call(
        base,
        Method::POST,
        "/v1/session",
        None,
        json_body(&json!({ "username": "alice", "password": ALICE_PASSWORD })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{session}");
    let alice = session["token"].as_str().unwrap().to_string();
    let (status, made) = call(
        base,
        Method::POST,
        &format!("/v1/accounts/{alice_id}/api-tokens"),
        Some(&alice),
        json_body(&json!({ "label": "phone" })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{made}");
    let api_token = made["token"].as_str().unwrap().to_string();

    // The token imports a conversation with a named contact and uploads an
    // attachment.
    let (status, run) = call(
        base,
        Method::POST,
        "/v1/imports",
        Some(&api_token),
        json_body(&json!({ "source": "whatsapp" })),
    )
    .await;
    assert_eq!(status, S::CREATED, "{run}");
    let run_id = run["id"].as_i64().unwrap();
    let sha256 = sha256_hex(ATTACHMENT);
    let (status, answer) = call(
        base,
        Method::PUT,
        &format!("/v1/assets/{sha256}"),
        Some(&api_token),
        Some(("application/octet-stream", ATTACHMENT.to_vec())),
    )
    .await;
    assert!(status.is_success(), "{status} {answer}");
    let header = json!({
        "schema_version": message_ir::SCHEMA_VERSION,
        "export": { "source": "whatsapp", "tool": "t", "tool_version": "0",
                    "owner_identity": "+15555550106", "owner_display_name": "Me" },
        "conversation": {
            "chat_identifier": CONTACT_PHONE, "conversation_type": "individual", "group_title": null,
            "participants": [
                { "identity": CONTACT_PHONE, "display_name": CONTACT_NAME },
                { "identity": CONTACT_EMAIL, "display_name": CONTACT_NAME }
            ],
            "stats": { "message_count": 1, "attachment_count": 1,
                       "first_timestamp_unix_ms": 1_700_000_000_000_i64,
                       "last_timestamp_unix_ms": 1_700_000_000_000_i64 }
        }
    });
    let message = json!({
        "guid": "g-1", "timestamp_unix_ms": 1_700_000_000_000_i64, "direction": "incoming",
        "service": "whatsapp", "message_kind": "sms", "sender_identity": CONTACT_PHONE,
        "sender_display_name": CONTACT_NAME, "subject": null, "text": MESSAGE_TEXT,
        "attachments": [{ "path": "attachments/photo.bin", "original_name": "photo.bin",
                          "mime_type": "application/octet-stream", "digest_sha256": sha256,
                          "is_sticker": false, "transcription": null, "sticker_effect": null }],
        "imessage": null, "source": null
    });
    let batch = format!("{header}\n{message}\n").into_bytes();
    let (status, answer) = call(
        base,
        Method::POST,
        &format!("/v1/imports/{run_id}/batches"),
        Some(&api_token),
        Some(("application/jsonl", batch)),
    )
    .await;
    assert_eq!(status, S::OK, "{answer}");
    let (status, answer) = call(
        base,
        Method::POST,
        &format!("/v1/imports/{run_id}/complete"),
        Some(&api_token),
        json_body(&json!({ "status": "completed" })),
    )
    .await;
    assert_eq!(status, S::OK, "{answer}");

    // The account searches, opens the attachment through a media link, and
    // changes its password.
    let (status, found) = call(
        base,
        Method::GET,
        &format!("/v1/messages?q={SEARCH_WORD}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, S::OK, "{found}");
    assert_eq!(found["total"], 1, "the search found the message: {found}");
    let (status, _) = call(
        base,
        Method::GET,
        "/v1/contacts?q=Zebediah",
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, S::OK);
    let (status, link) = call(
        base,
        Method::POST,
        &format!("/v1/assets/{sha256}/media-links"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, S::CREATED, "{link}");
    let media_link = link["url"].as_str().unwrap().to_string();
    let bytes = reqwest::get(format!("{base}{media_link}"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(bytes.as_ref(), ATTACHMENT);
    let (status, changed) = call(
        base,
        Method::PUT,
        &format!("/v1/accounts/{alice_id}/password"),
        Some(&alice),
        json_body(&json!({
            "password": ALICE_NEW_PASSWORD,
            "password_confirmation": ALICE_NEW_PASSWORD
        })),
    )
    .await;
    assert_eq!(status, S::OK, "{changed}");
    let alice_next = changed["token"].as_str().unwrap().to_string();

    // The owner reads the log back through its route.
    let (status, lines) = call(
        base,
        Method::GET,
        "/v1/server/log-lines?limit=500",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, S::OK, "{lines}");
    assert!(
        !lines["items"].as_array().unwrap().is_empty(),
        "the owner reads lines: {lines}"
    );

    let (log, files) = log_text(&data_dir);
    assert!(files >= 1);
    assert!(
        log.lines().any(|line| line.contains(" TRACE ")),
        "RUST_LOG=trace reached the server's log"
    );
    // The log is about these requests, so a clean result means something.
    for expected in [
        "/v1/server/claim",
        "/v1/imports",
        "/v1/messages?q=",
        "media_link=",
    ] {
        assert!(log.contains(expected), "the log names {expected}");
    }
    let media_link_value = media_link.rsplit_once("media_link=").unwrap().1.to_string();
    let link_signature = media_link_value.rsplit_once('.').unwrap().1.to_string();
    let attachment_text = String::from_utf8_lossy(ATTACHMENT).to_string();
    let mut never: Vec<(&str, String)> = vec![
        ("the owner's password", OWNER_PASSWORD.to_string()),
        ("the account's password", ALICE_PASSWORD.to_string()),
        ("the account's new password", ALICE_NEW_PASSWORD.to_string()),
        ("a password hash", "$argon2".to_string()),
        ("the message text", MESSAGE_TEXT.to_string()),
        ("a word of the message", SEARCH_WORD.to_string()),
        ("the contact's name", "Zebediah".to_string()),
        ("the contact's name", "Quixote".to_string()),
        ("the contact's phone", "5555550177".to_string()),
        ("the contact's email", "zebediah.quixote".to_string()),
        ("the attachment's bytes", attachment_text),
        ("the media link", link_signature),
    ];
    for (what, token) in [
        ("the owner's session token", &owner),
        ("the account's session token", &alice),
        ("the account's next session token", &alice_next),
        ("the API token", &api_token),
    ] {
        never.push((what, token.clone()));
        never.push((what, sha256_hex(token.as_bytes())));
    }
    let mut found = Vec::new();
    for (what, needle) in &never {
        for line in log.lines().filter(|line| line.contains(needle.as_str())) {
            found.push(format!("{what} ({needle}): {line}"));
        }
    }
    assert!(
        found.is_empty(),
        "the server's log holds what it never may:\n{}",
        found.join("\n")
    );
}
