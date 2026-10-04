//! `GET /v1/server/log-files` and `GET /v1/server/log-files/{id}` through
//! the router: the owner lists the files and downloads one whole
//! (`docs/architecture/server-log.md`). Who may call them is the credential
//! matrix's to check.

use axum::http::StatusCode;
use serde_json::Value;

use crate::logging::log_dir;
use crate::problem::ProblemType;
use crate::test_support::{
    PASSWORD, claim_as_owner, expect_problem, get_json, get_raw, serve, small_log_files,
    test_fixture, write_log_line as write,
};

/// The owner lists the files newest first and downloads one whole, as a
/// `text/plain` attachment named for the file.
#[tokio::test]
async fn the_owner_lists_the_files_and_downloads_one_whole() {
    let fixture = test_fixture().await;
    let state = &fixture.state;
    let owner = claim_as_owner(state, "keeper", PASSWORD).await;
    let files = small_log_files(state);
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
    write(&small_log_files(state), 0, "INFO", "one line");

    for id in ["99", "0"] {
        let (status, text) =
            get_raw(state, &format!("/v1/server/log-files/{id}"), &owner.token).await;
        expect_problem(status, &text, ProblemType::NotFound);
    }
}
