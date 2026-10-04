//! The server's log through the router: the owner reads its lines newest
//! first, filters and pages them, lists and downloads its files, and nobody
//! else reads any of it (`docs/architecture/server-log.md`).

use axum::http::StatusCode;
use serde_json::Value;

use crate::logging::{LogFiles, LogLimits, log_dir};
use crate::problem::ProblemType;
use crate::server::AppState;
use crate::test_support::{
    PASSWORD, claim_as_owner, expect_problem, get_json, get_raw, register_via_api, serve,
    test_fixture,
};

/// The server's log in the fixture's Data Directory, in small files so a
/// page crosses from one file into the next.
fn log_files(state: &AppState) -> LogFiles {
    LogFiles::open(
        &log_dir(&state.cfg.paths.data_dir),
        LogLimits {
            file_bytes: 200,
            files: 50,
        },
    )
    .unwrap()
}

/// Write line `n` at `level` saying `text`, as the server's subscriber does.
fn write(files: &LogFiles, n: usize, level: &str, text: &str) {
    let line = format!(
        "2026-10-04T12:00:{:02}.000000Z {level:>5} {text} n={n}\n",
        n % 60
    );
    files.write_event(line.as_bytes()).unwrap();
}

/// The `n` each line of a page carries, in the page's order.
fn numbers(page: &Value) -> Vec<u64> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            let text = line["text"].as_str().unwrap();
            text.rsplit_once("n=").unwrap().1.parse().unwrap()
        })
        .collect()
}

/// The owner reads the newest lines first, each with its id, time, level and
/// text, and pages older ones from the last line's id until none is left.
#[tokio::test]
async fn the_owner_reads_the_newest_lines_first_and_pages_older_ones() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let files = log_files(state);
    for n in 0..10 {
        write(&files, n, "INFO", "finished processing request");
    }

    let first: Value = get_json(state, "/v1/server/log-lines?limit=4", &owner.token).await;
    assert_eq!(numbers(&first), [9, 8, 7, 6]);
    assert_eq!(first["limit"], 4);
    assert_eq!(first["has_more"], true);
    let line = &first["items"][0];
    assert_eq!(line["level"], "info");
    assert_eq!(line["time"], "2026-10-04T12:00:09.000000Z");
    assert_eq!(line["text"], "finished processing request n=9");

    let mut seen = numbers(&first);
    let mut after = first["items"][3]["id"].as_i64().unwrap();
    loop {
        let page: Value = get_json(
            state,
            &format!("/v1/server/log-lines?limit=4&after={after}"),
            &owner.token,
        )
        .await;
        seen.extend(numbers(&page));
        if page["has_more"] == false {
            break;
        }
        after = page["items"][3]["id"].as_i64().unwrap();
    }
    assert_eq!(seen, (0..10).rev().collect::<Vec<_>>());
}

/// `level` keeps that level and the more severe; `text` keeps the lines
/// holding it, ignoring case; the two together keep lines that pass both.
#[tokio::test]
async fn the_owner_filters_lines_by_level_and_text() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let files = log_files(state);
    write(&files, 0, "ERROR", "internal server error");
    write(&files, 1, "WARN", "the import could not finish");
    write(&files, 2, "INFO", "Import Run started");
    write(&files, 3, "DEBUG", "import detail");

    let read = |query: &'static str| {
        let token = owner.token.clone();
        async move {
            let page: Value =
                get_json(state, &format!("/v1/server/log-lines?{query}"), &token).await;
            numbers(&page)
        }
    };

    assert_eq!(read("level=warn").await, [1, 0]);
    assert_eq!(read("level=error").await, [0]);
    assert_eq!(read("text=IMPORT").await, [3, 2, 1]);
    assert_eq!(read("level=info&text=import").await, [2, 1]);
    assert_eq!(read("").await, [3, 2, 1, 0]);
}

/// A level the log does not have, a negative `after` and a `limit` out of
/// range are each `422 Unprocessable Entity`.
#[tokio::test]
async fn a_level_or_cursor_that_breaks_a_rule_is_a_422() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;

    for query in ["level=loud", "after=-1", "limit=0", "limit=501"] {
        let (status, text) = get_raw(
            state,
            &format!("/v1/server/log-lines?{query}"),
            &owner.token,
        )
        .await;
        expect_problem(status, &text, ProblemType::ValidationFailed);
    }
}

/// The owner lists the files newest first and downloads one whole, as a
/// `text/plain` attachment named for the file.
#[tokio::test]
async fn the_owner_lists_the_files_and_downloads_one_whole() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let files = log_files(state);
    for n in 0..6 {
        write(&files, n, "INFO", "finished processing request");
    }

    let page: Value = get_json(state, "/v1/server/log-files", &owner.token).await;
    let items = page["items"].as_array().unwrap();
    assert!(items.len() >= 2, "{page}");
    assert_eq!(page["total"], items.len());
    assert_eq!(page["offset"], 0);
    let ids: Vec<i64> = items.iter().map(|f| f["id"].as_i64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "newest first: {ids:?}");
    let newest = &items[0];
    assert_eq!(newest["name"], format!("server-{:06}.log", ids[0]));
    let on_disk =
        std::fs::read(log_dir(&state.cfg.paths.data_dir).join(newest["name"].as_str().unwrap()))
            .unwrap();
    assert_eq!(newest["bytes"], on_disk.len());

    let server = serve(state).await;
    let response = reqwest::Client::new()
        .get(format!("{}/v1/server/log-files/{}", server.base(), ids[0]))
        .bearer_auth(&owner.token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers["content-type"], "text/plain; charset=utf-8");
    assert_eq!(
        headers["content-disposition"],
        format!("attachment; filename=\"server-{:06}.log\"", ids[0]).as_str()
    );
    assert_eq!(response.bytes().await.unwrap(), on_disk);
}

/// A file the log does not hold is `404 Not Found`, whether it was never
/// written or has been deleted to make room.
#[tokio::test]
async fn a_file_the_log_does_not_hold_is_a_404() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    write(&log_files(state), 0, "INFO", "one line");

    for id in ["99", "0"] {
        let (status, text) =
            get_raw(state, &format!("/v1/server/log-files/{id}"), &owner.token).await;
        expect_problem(status, &text, ProblemType::NotFound);
    }
}

/// The server's log is the owner's alone: an account's session is refused
/// with `403 Forbidden` on every route of it.
#[tokio::test]
async fn an_account_is_refused_the_server_log() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let _owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let alice = register_via_api(state, "alice", PASSWORD).await;
    write(&log_files(state), 0, "INFO", "one line");

    for path in [
        "/v1/server/log-lines",
        "/v1/server/log-files",
        "/v1/server/log-files/1",
    ] {
        let (status, text) = get_raw(state, path, &alice.token).await;
        expect_problem(status, &text, ProblemType::NotTheOwner);
    }
}
