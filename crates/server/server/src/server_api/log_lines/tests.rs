//! `GET /v1/server/log-lines` through the router: the owner reads the
//! newest lines first, filters them by level and text, and pages older ones
//! from a line's id (`docs/architecture/server-log.md`). Who may call it is
//! the credential matrix's to check.

use serde_json::Value;

use crate::problem::ProblemType;
use crate::test_support::{
    PASSWORD, claim_as_owner, expect_problem, get_json, get_raw, small_log_files, test_fixture,
    write_log_line as write,
};

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
    let files = small_log_files(state);
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

/// `level` keeps that level and the more severe. `text` keeps the lines
/// whose text holds it, ignoring case. The two together keep the lines that
/// pass both.
#[tokio::test]
async fn the_owner_filters_lines_by_level_and_text() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let files = small_log_files(state);
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
async fn a_level_or_line_id_that_breaks_a_rule_is_a_422() {
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
