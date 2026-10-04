//! The background pass as an Upload meets it: an Import Run of a photo, an
//! H.264 MP4 and a HEVC `.mov` ends, and the server makes their versions
//! afterwards (`docs/architecture/media.md`, rule 4).

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use axum::http::StatusCode;

use super::*;
use crate::server::AppState;
use crate::test_support::{RegisteredAccount, TestFixture, fixture_with_account};

/// The three synthetic files the tests import: an 800x600 PNG, a half-second
/// H.264 MP4 and a half-second HEVC `.mov`, each made with ffmpeg's test
/// pattern.
const FILES: [(&str, &str); 3] = [
    ("photo.png", "image/png"),
    ("h264.mp4", "video/mp4"),
    ("hevc.mov", "video/quicktime"),
];

/// The fingerprints of what [`import_three`] stored, in the order of
/// [`FILES`], and the conversation that holds them.
struct Imported {
    photo: String,
    h264: String,
    hevc: String,
    conversation_id: i64,
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// One batch of one message with the three files attached.
fn batch(shas: &[String]) -> String {
    let attachments: Vec<String> = FILES
        .iter()
        .zip(shas)
        .map(|((name, mime), sha)| {
            format!(
                r#"{{"path":"attachments/{name}","original_name":"{name}","mime_type":"{mime}","digest_sha256":"{sha}","is_sticker":false,"transcription":null,"sticker_effect":null}}"#
            )
        })
        .collect();
    let message = format!(
        r#"{{"guid":"g-media","timestamp_unix_ms":1426183462000,"direction":"incoming","service":"imessage","message_kind":"imessage","sender_identity":"+15555550123","sender_display_name":null,"subject":null,"text":"three files","attachments":[{}],"imessage":null,"source":null}}"#,
        attachments.join(",")
    );
    format!(
        "{}\n{message}\n",
        r#"{"schema_version":6,"export":{"source":"imessage","tool":"test","tool_version":"0","owner_identity":null,"owner_display_name":null},"conversation":{"chat_identifier":"+15555550123","conversation_type":"individual","group_title":null,"participants":[{"identity":"+15555550123","display_name":null}],"stats":{"message_count":1,"attachment_count":3,"first_timestamp_unix_ms":1426183462000,"last_timestamp_unix_ms":1426183462000}}}"#,
    )
}

/// Import the three files as an Upload does: start an Import Run, upload
/// each file, send the batch that names them, and complete the run.
async fn import_three(fixture: &TestFixture, alice: &RegisteredAccount) -> Imported {
    let state = &fixture.state;
    let (_, run): (String, serde_json::Value) = crate::test_support::post_created_json(
        state,
        "/v1/imports",
        &alice.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let run = run["id"].as_i64().unwrap();
    let mut shas = Vec::new();
    for (name, mime) in FILES {
        let bytes = fixture_bytes(name);
        let sha = crate::assets_api::sha256_hex(&bytes);
        let (status, text) = crate::test_support::put_raw(
            state,
            &format!("/v1/assets/{sha}"),
            &alice.token,
            mime,
            bytes,
        )
        .await;
        assert!(status.is_success(), "upload {name}: {status} {text}");
        shas.push(sha);
    }
    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{run}/batches"),
        &alice.token,
        "application/jsonl",
        batch(&shas),
    )
    .await;
    assert!(status.is_success(), "batch: {status} {text}");
    let completed: serde_json::Value = crate::test_support::post_json(
        state,
        &format!("/v1/imports/{run}/complete"),
        &alice.token,
        serde_json::json!({ "status": "completed" }),
    )
    .await;
    assert_eq!(completed["status"], "completed", "{completed}");

    let conversation_id: i64 =
        sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
            .bind(alice.account_id)
            .fetch_one(&mut *fixture.conn().await)
            .await
            .unwrap();
    let [photo, h264, hevc]: [String; 3] = shas.try_into().unwrap();
    Imported {
        photo,
        h264,
        hevc,
        conversation_id,
    }
}

/// The attachments of the one message, as the conversation answers them.
async fn attachments(
    state: &AppState,
    alice: &RegisteredAccount,
    conversation_id: i64,
) -> Vec<serde_json::Value> {
    let page: serde_json::Value = crate::test_support::get_json(
        state,
        &format!("/v1/conversations/{conversation_id}/messages"),
        &alice.token,
    )
    .await;
    page["items"][0]["attachments"].as_array().unwrap().clone()
}

/// GET `path` as an `<img>` does, with `token` or with none, and answer the
/// status, the `Content-Type` and the body.
async fn get_bytes(
    state: &AppState,
    path: &str,
    token: Option<&str>,
) -> (StatusCode, String, Vec<u8>) {
    let server = crate::test_support::serve(state).await;
    let mut request = reqwest::Client::new()
        .get(format!("{}{path}", server.base()))
        .header(reqwest::header::ACCEPT, "image/*");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    (
        status,
        content_type,
        response.bytes().await.unwrap().to_vec(),
    )
}

async fn queued(state: &AppState) -> i64 {
    crate::db::media_queue::count(&mut state.db.acquire().await.unwrap())
        .await
        .unwrap()
}

/// Run `test` on its own runtime with the real ffmpeg held available, or skip
/// it the way every ffmpeg test in the workspace skips (and fail under CI).
fn with_real_ffmpeg(test: impl Future<Output = ()>) {
    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}

/// The Upload never waits for a conversion: completing the run answers
/// while every Asset it brought is still queued, with no Thumbnail made and
/// none claimed. No pass runs in a test unless the test starts one.
#[tokio::test]
async fn an_import_run_completes_before_its_thumbnails_are_made() {
    let (fixture, alice) = fixture_with_account().await;
    let state = &fixture.state;

    let imported = import_three(&fixture, &alice).await;

    assert_eq!(
        queued(state).await,
        3,
        "each Asset the run brought is queued"
    );
    for attachment in attachments(state, &alice, imported.conversation_id).await {
        assert!(
            attachment.get("thumbnail_mime_type").is_none(),
            "no Thumbnail is made yet: {attachment}"
        );
    }
    let (status, _, _) = get_bytes(
        state,
        &format!("/v1/assets/{}/thumbnail", imported.photo),
        Some(&alice.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// After the pass, every image and video has a Thumbnail, and only the HEVC
/// `.mov`, which browsers often cannot play, has a Preview. The Thumbnail is
/// read like the original: with the Session, or with a Media Link.
#[test]
fn the_pass_makes_a_thumbnail_of_each_image_and_video_and_a_preview_only_for_hevc() {
    with_real_ffmpeg(async {
        let (fixture, alice) = fixture_with_account().await;
        let state = &fixture.state;
        let imported = import_three(&fixture, &alice).await;

        let made = work_through(&state.db, &state.cfg).await.unwrap();

        assert_eq!(
            (made.thumbnails, made.derived, made.errors),
            (3, 1, 0),
            "{made:?}"
        );
        assert_eq!(queued(state).await, 0, "the queue is worked through");
        let attachments = attachments(state, &alice, imported.conversation_id).await;
        for attachment in &attachments {
            assert_eq!(
                attachment["thumbnail_mime_type"], "image/jpeg",
                "{attachment}"
            );
            let preview = attachment.get("preview_mime_type");
            if attachment["sha256"] == imported.hevc.as_str() {
                assert_eq!(preview, Some(&serde_json::json!("video/mp4")));
            } else {
                let sha = attachment["sha256"].as_str().unwrap();
                assert!(
                    sha == imported.photo || sha == imported.h264,
                    "{attachment}"
                );
                assert_eq!(preview, None, "browsers show it as it is: {attachment}");
            }
        }

        let (status, content_type, bytes) = get_bytes(
            state,
            &format!("/v1/assets/{}/thumbnail", imported.photo),
            Some(&alice.token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "image/jpeg");
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG starts with SOI");

        let server = crate::test_support::serve(state).await;
        let link: serde_json::Value = reqwest::Client::new()
            .post(format!(
                "{}/v1/assets/{}/media-links",
                server.base(),
                imported.h264
            ))
            .bearer_auth(&alice.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let (status, content_type, _) =
            get_bytes(state, link["thumbnail_url"].as_str().unwrap(), None).await;
        assert_eq!(
            (status, content_type.as_str()),
            (StatusCode::OK, "image/jpeg"),
            "a Media Link reads the Thumbnail with no header"
        );
    });
}

/// The queue is a table, so what a stopped server left in it is worked on
/// when the server starts again: a new state over the same database, whose
/// pass starts with the queue as it was.
#[test]
fn a_server_started_again_works_through_what_was_queued() {
    with_real_ffmpeg(async {
        let (fixture, alice) = fixture_with_account().await;
        let imported = import_three(&fixture, &alice).await;
        assert_eq!(queued(&fixture.state).await, 3);

        let restarted = crate::server::test_app_state(fixture.state.db.clone(), fixture.dir());
        restarted
            .media_queue
            .start(restarted.db.clone(), Arc::clone(&restarted.cfg));

        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        while queued(&restarted).await > 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the pass did not work through the queue"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let attachments = attachments(&restarted, &alice, imported.conversation_id).await;
        assert_eq!(attachments.len(), 3);
        for attachment in &attachments {
            assert_eq!(
                attachment["thumbnail_mime_type"], "image/jpeg",
                "{attachment}"
            );
        }
    });
}

/// Without ffmpeg nothing can be made, so the pass leaves the queue as it
/// is, for a server that finds ffmpeg later. The tools are hidden outside
/// any `await`, which Clippy's `await_holding_lock` refuses.
#[test]
fn without_ffmpeg_the_queue_waits() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (fixture, alice) = runtime.block_on(fixture_with_account());
    runtime.block_on(import_three(&fixture, &alice));
    let state = &fixture.state;

    let made = {
        let _hidden = media::testutil::hide_ffmpeg();
        runtime
            .block_on(work_through(&state.db, &state.cfg))
            .unwrap()
    };

    assert_eq!(made, ProcessAssetsStats::default());
    assert_eq!(runtime.block_on(queued(state)), 3);
}

/// An Import Run that ends while the pass works on one of its Assets queues
/// it again, and the pass, when it is done with the old row, leaves the new
/// one: the run's new rows still get the versions.
#[tokio::test]
async fn an_asset_queued_again_while_it_is_worked_on_stays_queued() {
    let (fixture, alice) = fixture_with_account().await;
    let state = &fixture.state;
    import_three(&fixture, &alice).await;
    let run: i64 = sqlx::query_scalar("SELECT id FROM imports WHERE account_id = $1")
        .bind(alice.account_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let mut conn = state.db.acquire().await.unwrap();
    let worked_on = crate::db::media_queue::first(&mut conn)
        .await
        .unwrap()
        .unwrap();

    crate::db::media_queue::queue_import_run(&mut conn, alice.account_id, run)
        .await
        .unwrap();
    crate::db::media_queue::remove(&mut conn, &worked_on)
        .await
        .unwrap();

    assert_eq!(
        queued(state).await,
        3,
        "the Asset queued again is still queued"
    );
}
