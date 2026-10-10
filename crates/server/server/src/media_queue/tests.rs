//! The background pass as an Upload meets it: an Import Run of a photo, an
//! H.264 MP4 and a HEVC `.mov` ends, and the server makes their versions
//! afterwards (`docs/architecture/media.md`, rule 4).

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use axum::http::StatusCode;

use super::*;
use crate::problem::ProblemType;
use crate::server::AppState;
use crate::test_support::{
    RegisteredAccount, TestFixture, attachment, conversation_header, expect_problem,
    fixture_with_account, http_client, message_line,
};

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

/// One batch of one message with `files`, each a name and a media type,
/// attached under the fingerprints `shas`.
fn batch(files: &[(&str, &str)], shas: &[String]) -> String {
    let message = files.iter().zip(shas).fold(
        message_line("g-media", "some files").sender("+15555550123"),
        |message, ((name, mime), sha)| {
            message.attachment(message_ir::IrAttachment {
                digest_sha256: Some(sha.clone()),
                ..attachment(&format!("attachments/{name}"), name, mime)
            })
        },
    );
    let header = conversation_header("imessage", "+15555550123").participant("+15555550123", None);
    format!("{header}\n{message}\n")
}

/// Import the three files as an Upload does: start an Import Run, upload
/// each file, send the batch that names them, and complete the run.
async fn import_three(fixture: &TestFixture, alice: &RegisteredAccount) -> Imported {
    let files: Vec<(&str, &str, Vec<u8>)> = FILES
        .iter()
        .map(|(name, mime)| (*name, *mime, fixture_bytes(name)))
        .collect();
    let (shas, conversation_id) = import_files(fixture, alice, &files).await;
    let [photo, h264, hevc]: [String; 3] = shas.try_into().unwrap();
    Imported {
        photo,
        h264,
        hevc,
        conversation_id,
    }
}

/// Import `files`, each a name, a media type and its bytes, as an Upload
/// does, and answer their fingerprints and the conversation that holds them.
async fn import_files(
    fixture: &TestFixture,
    alice: &RegisteredAccount,
    files: &[(&str, &str, Vec<u8>)],
) -> (Vec<String>, i64) {
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
    for (name, mime, bytes) in files {
        let sha = crate::assets_api::sha256_hex(bytes);
        let (status, text) = crate::test_support::put_raw(
            state,
            &format!("/v1/assets/{sha}"),
            &alice.token,
            mime,
            bytes.clone(),
        )
        .await;
        assert!(status.is_success(), "upload {name}: {status} {text}");
        shas.push(sha);
    }
    let names: Vec<(&str, &str)> = files.iter().map(|(name, mime, _)| (*name, *mime)).collect();
    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{run}/batches"),
        &alice.token,
        "application/jsonl",
        batch(&names, &shas),
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
    (shas, conversation_id)
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
    let mut request = http_client()
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
        assert_eq!(
            attachment.get("thumbnail_mime_type"),
            Some(&serde_json::Value::Null),
            "no Thumbnail is made yet: {attachment}"
        );
    }
    let (status, _, body) = get_bytes(
        state,
        &format!("/v1/assets/{}/thumbnail", imported.photo),
        Some(&alice.token),
    )
    .await;
    expect_problem(
        status,
        &String::from_utf8(body).unwrap(),
        ProblemType::NotFound,
    );
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

        let made = work_through(&state.db, &state.cfg, &AtomicBool::new(false))
            .await
            .unwrap();

        assert_eq!(
            (made.thumbnails, made.derived, made.not_made),
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
                assert_eq!(
                    preview,
                    Some(&serde_json::Value::Null),
                    "browsers show it as it is: {attachment}"
                );
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
        let link: serde_json::Value = http_client()
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

/// Whether the original is shown as it is, as the conversation answers it
/// for the attachment stored under `sha256`.
fn shown_as_is(attachments: &[serde_json::Value], sha256: &str) -> serde_json::Value {
    attachments
        .iter()
        .find(|attachment| attachment["sha256"] == sha256)
        .unwrap_or_else(|| panic!("no attachment {sha256} in {attachments:?}"))["shown_as_is"]
        .clone()
}

/// The pass decides whether every browser shows each original as it is
/// (`docs/architecture/media.md`, rule 2), and the conversation answers it
/// in `shown_as_is`, so the web app never repeats the list of types and a
/// read never runs ffprobe. A PNG and an H.264 MP4 are shown as they are. A
/// HEVC video is not, in an `.mp4` as in a `.mov`, because only the file can
/// tell its codec, and each gets a Preview. Until the pass has looked at a
/// file, nothing says it is shown as it is.
#[test]
fn the_pass_decides_which_originals_are_shown_as_they_are() {
    with_real_ffmpeg(async {
        let (fixture, alice) = fixture_with_account().await;
        let state = &fixture.state;
        let files: Vec<(&str, &str, Vec<u8>)> = [
            ("photo.png", "image/png"),
            ("h264.mp4", "video/mp4"),
            ("hevc.mov", "video/quicktime"),
            ("hevc.mp4", "video/mp4"),
        ]
        .into_iter()
        .map(|(name, mime)| (name, mime, fixture_bytes(name)))
        .collect();
        let (shas, conversation_id) = import_files(&fixture, &alice, &files).await;

        let before = attachments(state, &alice, conversation_id).await;
        for sha in &shas {
            assert_eq!(
                shown_as_is(&before, sha),
                serde_json::json!(false),
                "nothing is decided before the pass: {before:?}"
            );
        }

        work_through(&state.db, &state.cfg, &AtomicBool::new(false))
            .await
            .unwrap();

        let after = attachments(state, &alice, conversation_id).await;
        let decided = shas
            .iter()
            .map(|sha| shown_as_is(&after, sha))
            .collect::<Vec<_>>();
        assert_eq!(
            decided,
            [true, true, false, false].map(serde_json::Value::Bool),
            "the PNG and the H.264 MP4 are shown as they are, the HEVC .mov and .mp4 are not: {after:?}"
        );
        let hevc_mp4 = after
            .iter()
            .find(|attachment| attachment["sha256"] == shas[3].as_str())
            .unwrap();
        assert_eq!(
            hevc_mp4["preview_mime_type"], "video/mp4",
            "a HEVC MP4 gets a Preview: {hevc_mp4}"
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
/// is, for a server that finds ffmpeg later. What needs no ffprobe is
/// decided all the same, so a photo opens as it is on a server that cannot
/// convert. An MP4's codec cannot be read without ffprobe, so nothing says
/// it is shown as it is yet. The tools are hidden outside any `await`,
/// which Clippy's `await_holding_lock` refuses.
#[test]
fn without_ffmpeg_the_queue_waits() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (fixture, alice) = runtime.block_on(fixture_with_account());
    let imported = runtime.block_on(import_three(&fixture, &alice));
    let state = &fixture.state;

    let made = {
        let _hidden = media::testutil::hide_ffmpeg();
        runtime
            .block_on(work_through(&state.db, &state.cfg, &AtomicBool::new(false)))
            .unwrap()
    };

    assert_eq!(made, ProcessAssetsStats::default());
    assert_eq!(runtime.block_on(queued(state)), 3);
    let after = runtime.block_on(attachments(state, &alice, imported.conversation_id));
    assert_eq!(
        shown_as_is(&after, &imported.photo),
        serde_json::json!(true)
    );
    assert_eq!(
        shown_as_is(&after, &imported.h264),
        serde_json::json!(false)
    );
}

/// The sweep the pass runs before each conversion records whether each
/// queued original is shown as it is, converts nothing, and leaves the
/// queue as it is, so a photo queued behind a long video opens at once
/// rather than after the video's Preview is made.
#[test]
fn decide_queued_records_every_queued_original_and_leaves_the_queue() {
    with_real_ffmpeg(async {
        let (fixture, alice) = fixture_with_account().await;
        let state = &fixture.state;
        let imported = import_three(&fixture, &alice).await;

        let mut stats = ProcessAssetsStats::default();
        decide_queued(
            &state.db,
            &state.cfg,
            &AtomicBool::new(false),
            0,
            &mut stats,
        )
        .await
        .unwrap();

        assert_eq!(queued(state).await, 3, "nothing leaves the queue");
        assert_eq!(stats, ProcessAssetsStats::default(), "nothing failed");
        let after = attachments(state, &alice, imported.conversation_id).await;
        assert_eq!(
            [&imported.photo, &imported.h264, &imported.hevc].map(|sha| shown_as_is(&after, sha)),
            [true, true, false].map(serde_json::Value::Bool),
            "{after:?}"
        );
        for attachment in &after {
            assert_eq!(
                attachment["thumbnail_mime_type"],
                serde_json::Value::Null,
                "nothing is converted: {attachment}"
            );
        }
    });
}

/// The pass sweeps again before each conversion, from the last row it
/// decided, so an Asset an Import Run queues while a long video is converted
/// is decided before the next conversion rather than at its own turn.
#[tokio::test]
async fn decide_queued_decides_what_was_queued_after_the_last_sweep() {
    let (fixture, alice) = fixture_with_account().await;
    let state = &fixture.state;
    let imported = import_three(&fixture, &alice).await;
    let stop = AtomicBool::new(false);
    let mut stats = ProcessAssetsStats::default();
    let undecide_photo = || async {
        crate::db::attachment_versions::record_shown_as_is(
            &mut state.db.acquire().await.unwrap(),
            crate::db::attachment_versions::OriginalRows {
                account_id: alice.account_id,
                original_sha: &imported.photo,
            },
            false,
        )
        .await
        .unwrap();
    };
    let last = decide_queued(&state.db, &state.cfg, &stop, 0, &mut stats)
        .await
        .unwrap();
    undecide_photo().await;

    // A later Import Run queues the same Assets again, in new rows.
    let run: i64 = sqlx::query_scalar("SELECT id FROM imports WHERE account_id = $1")
        .bind(alice.account_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    crate::db::media_queue::queue_import_run(
        &mut state.db.acquire().await.unwrap(),
        alice.account_id,
        run,
    )
    .await
    .unwrap();
    let newest = decide_queued(&state.db, &state.cfg, &stop, last, &mut stats)
        .await
        .unwrap();

    let after = attachments(state, &alice, imported.conversation_id).await;
    assert_eq!(
        shown_as_is(&after, &imported.photo),
        serde_json::json!(true)
    );
    assert!(newest > last, "the sweep moves on to the newest row");

    // Nothing was queued since, so the next sweep decides nothing.
    undecide_photo().await;
    decide_queued(&state.db, &state.cfg, &stop, newest, &mut stats)
        .await
        .unwrap();
    let after = attachments(state, &alice, imported.conversation_id).await;
    assert_eq!(
        shown_as_is(&after, &imported.photo),
        serde_json::json!(false)
    );
}

/// Only ffprobe can read an MP4's codec, so a pass that has lost ffmpeg
/// leaves an MP4 as an earlier pass with it decided: an H.264 MP4 queued
/// again keeps opening its original.
#[test]
fn a_pass_without_ffprobe_keeps_what_one_with_it_decided_about_an_mp4() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (fixture, alice) = runtime.block_on(fixture_with_account());
    let state = &fixture.state;
    let imported = runtime.block_on(import_three(&fixture, &alice));
    {
        let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
            return;
        };
        runtime
            .block_on(work_through(&state.db, &state.cfg, &AtomicBool::new(false)))
            .unwrap();
    }
    let run: i64 = runtime
        .block_on(
            sqlx::query_scalar("SELECT id FROM imports WHERE account_id = $1")
                .bind(alice.account_id)
                .fetch_one(&state.db),
        )
        .unwrap();
    runtime
        .block_on(async {
            let mut conn = state.db.acquire().await.unwrap();
            crate::db::media_queue::queue_import_run(&mut conn, alice.account_id, run).await
        })
        .unwrap();

    {
        let _hidden = media::testutil::hide_ffmpeg();
        runtime
            .block_on(work_through(&state.db, &state.cfg, &AtomicBool::new(false)))
            .unwrap();
    }

    let after = runtime.block_on(attachments(state, &alice, imported.conversation_id));
    assert_eq!(shown_as_is(&after, &imported.h264), serde_json::json!(true));
    assert_eq!(
        shown_as_is(&after, &imported.photo),
        serde_json::json!(true)
    );
}

/// A Demo Account built without ffmpeg is never queued, so the build decides
/// every original of the account itself (`decide_shown_as_is` with no
/// fingerprint): its photos open as they are.
#[test]
fn without_ffmpeg_every_original_of_an_account_is_decided() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (fixture, alice) = runtime.block_on(fixture_with_account());
    let state = &fixture.state;
    let imported = runtime.block_on(import_three(&fixture, &alice));

    {
        let _hidden = media::testutil::hide_ffmpeg();
        runtime
            .block_on(crate::process_assets::decide_shown_as_is(
                &state.cfg,
                &state.db,
                alice.account_id,
                None,
            ))
            .unwrap();
    }

    let after = runtime.block_on(attachments(state, &alice, imported.conversation_id));
    assert_eq!(
        [&imported.photo, &imported.h264, &imported.hevc].map(|sha| shown_as_is(&after, sha)),
        [true, false, false].map(serde_json::Value::Bool),
        "{after:?}"
    );
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

/// A video that takes ffmpeg seconds to convert: the HEVC fixture played
/// 8000 times over, copied rather than encoded, so it is made in an instant.
fn long_video() -> Vec<u8> {
    let ffmpeg = media::ffmpeg_path().expect("ffmpeg, which the guard found");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("long.mov");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/hevc.mov");
    let status = std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-stream_loop", "7999", "-i"])
        .arg(&source)
        .args(["-c", "copy"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "make the long video: {status}");
    std::fs::read(&out).unwrap()
}

/// The ids of the processes making a Preview into a work directory under
/// `work`: the ffmpeg runs whose command line names a `Preview-` file there.
#[cfg(target_os = "linux")]
fn making_a_preview_in(work: &Path) -> Vec<u32> {
    let work = work.to_string_lossy();
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let names_preview = cmdline
                .split(|byte| *byte == 0)
                .map(String::from_utf8_lossy)
                .any(|arg| arg.starts_with(work.as_ref()) && arg.contains("/Preview-"));
            names_preview.then_some(pid)
        })
        .collect()
}

/// Every file under `dir`, at any depth.
fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                dirs.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    files
}

/// A server that stops while the pass converts a video stops ffmpeg with
/// it: no ffmpeg is left converting, the Asset stays queued for the next
/// start, and the part-made Preview is removed (#1729).
#[cfg(target_os = "linux")]
#[test]
fn stopping_the_pass_stops_its_conversion_and_leaves_the_asset_queued() {
    with_real_ffmpeg(async {
        let (fixture, alice) = fixture_with_account().await;
        let state = &fixture.state;
        import_files(
            &fixture,
            &alice,
            &[("long.mov", "video/quicktime", long_video())],
        )
        .await;
        assert_eq!(queued(state).await, 1);
        let work = state.cfg.paths.data_dir.join(".media-work");

        state
            .media_queue
            .start(state.db.clone(), Arc::clone(&state.cfg));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        while making_a_preview_in(&work).is_empty() || files_under(&work).is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the pass did not start making the Preview"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        state.media_queue.stop().await;

        assert_eq!(
            making_a_preview_in(&work),
            Vec::<u32>::new(),
            "ffmpeg stops with the pass"
        );
        assert_eq!(queued(state).await, 1, "the Asset stays queued");
        assert_eq!(
            files_under(&work),
            Vec::<std::path::PathBuf>::new(),
            "the part-made Preview is removed"
        );
    });
}

/// ffmpeg and ffprobe in two places are found, so the pass says they are
/// not used and why, not that ffmpeg is missing.
#[test]
fn tools_in_two_places_are_reported_as_not_used_with_the_reason() {
    let reason = "ffmpeg is on PATH at /usr/bin/ffmpeg and ffprobe is in the Tools Directory at /srv/tools/ffprobe. \
                  Both must be on PATH or both in the Tools Directory.";
    let why = tools_unavailable(&Err(anyhow::anyhow!(reason))).expect("a warning");
    assert!(why.starts_with("ffmpeg and ffprobe are not used"), "{why}");
    assert!(why.contains(reason), "{why}");
    assert!(!why.contains("not found"), "{why}");
}

/// A missing program is named, and a complete pair gives no warning.
#[test]
fn a_missing_program_is_named_as_not_found() {
    let only_ffmpeg = media::FfmpegTools {
        ffmpeg: Some("/usr/bin/ffmpeg".into()),
        ffprobe: None,
    };
    let why = tools_unavailable(&Ok(only_ffmpeg)).expect("a warning");
    assert!(why.starts_with("ffprobe was not found"), "{why}");
    let both = media::FfmpegTools {
        ffmpeg: Some("/usr/bin/ffmpeg".into()),
        ffprobe: Some("/usr/bin/ffprobe".into()),
    };
    assert_eq!(tools_unavailable(&Ok(both)), None);
}

/// What a backup holds of the photo of `g-photo` in [`import_photo_run`].
#[derive(Clone, Copy)]
enum Photo {
    /// The message has no attachment.
    NotListed,
    /// The attachment is listed, but the backup did not hold its file.
    Missing,
    /// The attachment and its file, uploaded first.
    Present,
}

/// One Import Run in Append mode of one message, `g-photo`, with the JPEG
/// `photo.jpg` (ffmpeg's test pattern, made small) as `photo` says.
/// Answers the JPEG's fingerprint.
async fn import_photo_run(
    fixture: &TestFixture,
    alice: &RegisteredAccount,
    photo: Photo,
) -> String {
    let state = &fixture.state;
    let bytes = fixture_bytes("photo.jpg");
    let sha = crate::assets_api::sha256_hex(&bytes);
    let (_, run): (String, serde_json::Value) = crate::test_support::post_created_json(
        state,
        "/v1/imports",
        &alice.token,
        serde_json::json!({ "source": "imessage", "mode": "append" }),
    )
    .await;
    let run = run["id"].as_i64().unwrap();
    let listed = attachment("attachments/photo.jpg", "photo.jpg", "image/jpeg");
    let attached = match photo {
        Photo::Present => {
            let (status, text) = crate::test_support::put_raw(
                state,
                &format!("/v1/assets/{sha}"),
                &alice.token,
                "image/jpeg",
                bytes,
            )
            .await;
            assert!(status.is_success(), "upload: {status} {text}");
            Some(message_ir::IrAttachment {
                digest_sha256: Some(sha.clone()),
                ..listed
            })
        }
        Photo::Missing => Some(message_ir::IrAttachment {
            missing_reason: Some("not_exported".into()),
            ..listed
        }),
        Photo::NotListed => None,
    };
    let header = conversation_header("imessage", "+15555550123").participant("+15555550123", None);
    let message = message_line("g-photo", "a photo").sender("+15555550123");
    let message = match attached {
        Some(attached) => message.attachment(attached),
        None => message,
    };
    let (status, text) = crate::test_support::post_raw(
        state,
        &format!("/v1/imports/{run}/batches"),
        &alice.token,
        "application/jsonl",
        format!("{header}\n{message}\n"),
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
    sha
}

/// A backup imported again in Append mode now holds a JPEG that was missing
/// the first time. The second run gives the stored attachment row its file,
/// on a message the first run created, and queues that Asset (#1946).
#[tokio::test]
async fn a_run_that_gives_a_stored_attachment_its_file_queues_the_asset() {
    let (fixture, alice) = fixture_with_account().await;
    let state = &fixture.state;
    import_photo_run(&fixture, &alice, Photo::Missing).await;
    assert_eq!(queued(state).await, 0, "the first run stored no file");

    let sha = import_photo_run(&fixture, &alice, Photo::Present).await;

    let queued_sha: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM media_queue")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(queued_sha, vec![sha]);
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        messages, 1,
        "the second run met the message the first stored"
    );
}

/// A backup imported again in Append mode now lists a JPEG on a message
/// that had no attachment the first time. The second run adds the
/// attachment row under the message the first run created, and queues its
/// Asset (#1946).
#[tokio::test]
async fn a_run_that_adds_an_attachment_to_a_stored_message_queues_the_asset() {
    let (fixture, alice) = fixture_with_account().await;
    let state = &fixture.state;
    import_photo_run(&fixture, &alice, Photo::NotListed).await;

    let sha = import_photo_run(&fixture, &alice, Photo::Present).await;

    let queued_sha: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM media_queue")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(queued_sha, vec![sha]);
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        messages, 1,
        "the second run met the message the first stored"
    );
}

/// The Asset the second run queues in
/// [`a_run_that_gives_a_stored_attachment_its_file_queues_the_asset`] gets
/// its Thumbnail from the pass.
#[test]
fn a_file_a_later_run_fills_in_gets_its_thumbnail() {
    with_real_ffmpeg(async {
        let (fixture, alice) = fixture_with_account().await;
        let state = &fixture.state;
        import_photo_run(&fixture, &alice, Photo::Missing).await;
        let sha = import_photo_run(&fixture, &alice, Photo::Present).await;

        let made = work_through(&state.db, &state.cfg, &AtomicBool::new(false))
            .await
            .unwrap();

        assert_eq!(made.thumbnails, 1, "{made:?}");
        let conversation_id: i64 =
            sqlx::query_scalar("SELECT id FROM conversations WHERE account_id = $1")
                .bind(alice.account_id)
                .fetch_one(&mut *fixture.conn().await)
                .await
                .unwrap();
        let attachments = attachments(state, &alice, conversation_id).await;
        assert_eq!(attachments.len(), 1, "{attachments:?}");
        assert_eq!(attachments[0]["sha256"], sha.as_str());
        assert_eq!(attachments[0]["thumbnail_mime_type"], "image/jpeg");
    });
}
