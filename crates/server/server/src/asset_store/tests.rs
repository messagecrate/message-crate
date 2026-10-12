use std::fs;
use std::time::Duration;

use super::*;

const SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

#[test]
fn a_stored_path_must_stay_under_its_directory() {
    let dir = Path::new("/srv/data/acct/imessage/assets");
    assert_eq!(
        join_under(dir, "ab/abcd.jpg"),
        Some(dir.join("ab/abcd.jpg"))
    );
    for bad in ["../elsewhere.jpg", "/etc/passwd", "ab/../../x", ""] {
        assert!(join_under(dir, bad).is_none(), "{bad:?} must be refused");
    }
}

#[test]
fn removing_a_file_that_is_already_gone_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let present = dir.path().join("present.jpg");
    fs::write(&present, b"x").unwrap();

    remove_file(&present).unwrap();
    remove_file(&dir.path().join("never-existed.jpg")).unwrap();

    assert!(!present.exists());
}

/// A stored value that is not a fingerprint has no sidecar, rather than a
/// panic on the slice or a path outside the shard.
#[test]
fn a_stored_sidecar_needs_a_fingerprint() {
    let dir = Path::new("/srv/assets");
    assert_eq!(
        stored_sidecar_path(dir, SHA),
        Some(dir.join("ab").join(format!(".{SHA}.mime")))
    );
    for bad in ["", "a", "é", "../x"] {
        assert_eq!(stored_sidecar_path(dir, bad), None, "{bad:?}");
    }
}

/// The sweep removes only names the server writes: an original, a Preview
/// with its extension, and a sidecar. A temporary file and a name that is
/// not a fingerprint are left alone.
#[test]
fn a_stored_name_gives_its_fingerprint_and_any_other_name_gives_none() {
    let upper = SHA.to_ascii_uppercase();
    assert_eq!(fingerprint_of(SHA).as_deref(), Some(SHA));
    assert_eq!(fingerprint_of(&format!("{SHA}.jpg")).as_deref(), Some(SHA));
    assert_eq!(
        fingerprint_of(&format!(".{SHA}.mime")).as_deref(),
        Some(SHA)
    );
    assert_eq!(fingerprint_of(&upper).as_deref(), Some(SHA));
    let short = &SHA[..63];
    let bare_dot = format!(".{SHA}");
    for other in [".tmpAbc123", "notes.txt", bare_dot.as_str(), short] {
        assert_eq!(fingerprint_of(other), None, "{other:?}");
    }
}

/// `[server] upload_stall_secs` at its default, the limit the sweep tests
/// hold uploads to.
pub(crate) const STALL: Duration = Duration::from_secs(60);

/// Abandonment at `now` after [`STALL`].
pub(crate) fn abandoned_at(now: SystemTime) -> Abandonment {
    Abandonment { now, stall: STALL }
}

/// Set `path`'s modified time, a file's or a directory's, to `at`.
pub(crate) fn written_at(path: &Path, at: SystemTime) {
    fs::File::options()
        .read(true)
        .write(path.is_file())
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// #2652: an upload nothing was written to for the stall limit is
/// abandoned, by its manifest's time, or its directory's when it has no
/// manifest yet. One written to a second inside the limit is not.
#[test]
fn an_upload_session_is_abandoned_once_nothing_was_written_for_the_stall_limit() {
    let dir = tempfile::tempdir().unwrap();
    let with_manifest = dir.path().join("with-manifest");
    fs::create_dir_all(&with_manifest).unwrap();
    fs::write(with_manifest.join("manifest.json"), b"{}").unwrap();
    let without_manifest = dir.path().join("without-manifest");
    fs::create_dir_all(&without_manifest).unwrap();
    let written = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    written_at(&with_manifest.join("manifest.json"), written);
    written_at(&without_manifest, written);
    let second = Duration::from_secs(1);

    for session in [&with_manifest, &without_manifest] {
        assert!(!upload_session_is_stale(session, abandoned_at(written)).unwrap());
        assert!(!upload_session_is_stale(session, abandoned_at(written + STALL - second)).unwrap());
        assert!(upload_session_is_stale(session, abandoned_at(written + STALL)).unwrap());
    }
}

/// Set `path`'s modified time to two days ago, past every age limit a sweep
/// holds a file to, so every sweep treats it as abandoned.
pub(crate) fn make_abandoned(path: &Path) {
    written_at(
        path,
        SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60),
    );
}

/// `process-assets` writes a Preview before the row that names it. The
/// sweep leaves an unnamed Preview younger than the grace period alone and
/// removes an older one.
#[test]
fn the_sweep_leaves_a_fresh_unnamed_preview_for_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let shard = dir.path().join("ab");
    fs::create_dir_all(&shard).unwrap();
    let fresh = shard.join(format!("{SHA}.jpg"));
    let old_sha = format!("ab{}", "1".repeat(62));
    let old = shard.join(format!("{old_sha}.jpg"));
    for file in [&fresh, &old] {
        fs::write(file, b"preview").unwrap();
    }
    make_abandoned(&old);

    let paths = PathsConfig {
        db: dir.path().join("db"),
        data_dir: dir.path().join("data"),
        assets_dir: "assets".to_string(),
        assets_converted_dir: "assets_converted".to_string(),
    };
    let mut removal = RemovalDir::new(&paths, 1);
    let removed = sweep_store_dir(
        1,
        dir.path(),
        &HashSet::new(),
        PREVIEW_GRACE_SECS,
        &mut removal,
    );

    assert_eq!(removed, 1);
    assert!(fresh.is_file(), "a Preview its row may not name yet stays");
    assert!(!old.exists(), "an old unnamed Preview goes");
}

/// The account the database-backed tests below remove files for.
const ACCOUNT: i64 = 7;

/// A fixture with [`ACCOUNT`]'s row in it.
async fn fixture_with_removal_account() -> crate::test_support::TestFixture {
    let fixture = crate::test_support::test_fixture().await;
    fixture.account_with_id(ACCOUNT, "removals").await;
    fixture
}

/// An unnamed original of [`ACCOUNT`] in its shard, returning its path.
fn stored_original(paths: &PathsConfig) -> PathBuf {
    let shard = paths.assets_dir_for_account(ACCOUNT).join(&SHA[..2]);
    fs::create_dir_all(&shard).unwrap();
    let original = shard.join(format!("{SHA}.jpg"));
    fs::write(&original, b"jpeg bytes").unwrap();
    original
}

/// Every entry under [`ACCOUNT`]'s `.removing/` directory.
fn left_in_removing(paths: &PathsConfig) -> Vec<PathBuf> {
    fs::read_dir(removing_dir(paths, ACCOUNT))
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default()
}

/// A deletion that says on the returned receiver when it has begun, then
/// waits for the returned sender before it deletes anything.
fn paused_deletion() -> (
    impl FnOnce(i64, Vec<PathBuf>) + Send + 'static,
    tokio::sync::oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (started, deletion_started) = tokio::sync::oneshot::channel();
    let (finish, finish_signal) = std::sync::mpsc::channel::<()>();
    let delete = move |account_id, dirs| {
        started.send(()).unwrap();
        finish_signal.recv().unwrap();
        delete_removed(account_id, dirs);
    };
    (delete, deletion_started, finish)
}

/// Whether a write transaction on `pool` begins and commits within two
/// seconds, well inside the 15 s busy timeout.
async fn writes_promptly(
    pool: &SqlitePool,
) -> Result<sqlx::Result<()>, tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut conn = pool.acquire().await?;
        begin_write(&mut conn).await?.commit().await
    })
    .await
}

/// #1544: removing files holds the write lock while it moves them out of
/// the store, and lets it go before it deletes them, so a delete on a slow
/// disk does not make every other writer wait out the busy timeout.
#[tokio::test]
async fn another_writer_is_not_blocked_while_removed_files_are_deleted() {
    let fixture = fixture_with_removal_account().await;
    let pool = fixture.state.db.clone();
    let paths = fixture.state.cfg.paths.clone();
    let original = stored_original(&paths);
    let (delete, deletion_started, finish) = paused_deletion();

    let removal = crate::request_id::spawn_in_current_span({
        let (pool, paths, original) = (pool.clone(), paths.clone(), original.clone());
        async move {
            unless_import_running_then(
                &pool,
                &paths,
                ACCOUNT,
                move |removal| {
                    take_out(ACCOUNT, &original, Some(removal), remove_file);
                },
                delete,
            )
            .await;
        }
    });
    deletion_started.await.unwrap();
    let wrote = writes_promptly(&pool).await;
    finish.send(()).unwrap();
    removal.await.unwrap();

    assert!(
        matches!(wrote, Ok(Ok(()))),
        "another writer waited on the removal: {wrote:?}"
    );
    assert!(!original.exists(), "the original is gone");
    assert_eq!(left_in_removing(&paths), Vec::<PathBuf>::new());
}

/// #1544: the sweep at a run's end, too, deletes what it moved out of the
/// store only after it let go of the write lock.
#[tokio::test]
async fn another_writer_is_not_blocked_while_swept_files_are_deleted() {
    let fixture = fixture_with_removal_account().await;
    let pool = fixture.state.db.clone();
    let paths = fixture.state.cfg.paths.clone();
    let original = stored_original(&paths);
    let (delete, deletion_started, finish) = paused_deletion();

    let sweep = crate::request_id::spawn_in_current_span({
        let (pool, paths) = (pool.clone(), paths.clone());
        async move { sweep_unreferenced_then(&pool, &paths, ACCOUNT, delete).await }
    });
    deletion_started.await.unwrap();
    let wrote = writes_promptly(&pool).await;
    finish.send(()).unwrap();
    let removed = sweep.await.unwrap().unwrap();

    assert!(
        matches!(wrote, Ok(Ok(()))),
        "another writer waited on the sweep: {wrote:?}"
    );
    assert_eq!(removed, 1);
    assert!(!original.exists(), "the original is gone");
    assert_eq!(left_in_removing(&paths), Vec::<PathBuf>::new());
}

/// A removal that finds nothing to move makes no `.removing/` directory.
#[tokio::test]
async fn a_removal_with_nothing_to_move_leaves_nothing_on_disk() {
    let fixture = fixture_with_removal_account().await;
    let paths = fixture.state.cfg.paths.clone();

    let removed = sweep_unreferenced(&fixture.state.db, &paths, ACCOUNT)
        .await
        .unwrap();
    unless_import_running(&fixture.state.db, &paths, ACCOUNT, |removal| {
        take_out(
            ACCOUNT,
            &PathBuf::from("/nowhere/x.jpg"),
            Some(removal),
            remove_file,
        );
    })
    .await;

    assert_eq!(removed, 0);
    assert!(
        !account_dir(&paths, ACCOUNT).exists(),
        "nothing is made on disk"
    );
}

/// #2186: a warning written while a request's removal runs in a task of its
/// own, and on the blocking pool, carries the request's id, so the log line
/// can be tied to the request that caused it.
#[tokio::test]
async fn a_warning_from_a_removal_carries_the_request_id() {
    use crate::logging::{LogFiles, LogLevel, LogLinesQuery, SERVER_LOG_LIMITS, read_lines};
    use tracing::Instrument;

    let fixture = fixture_with_removal_account().await;
    let paths = fixture.state.cfg.paths.clone();
    let log_dir = tempfile::tempdir().unwrap();
    let files = LogFiles::open(log_dir.path(), SERVER_LOG_LIMITS).unwrap();
    // At `info`, as the server logs by default: the request's span is an
    // `info` span, and a filter that drops it drops the id with it.
    let _subscriber =
        tracing::subscriber::set_default(crate::logging::subscriber_for(files, "info"));
    // A thread with no subscriber reaches the warning first, as another
    // test's thread can in a full run. This test still reads its line
    // (#2824).
    {
        let paths = paths.clone();
        std::thread::spawn(move || {
            let mut removal = RemovalDir::new(&paths, ACCOUNT);
            assert!(removal.make_or_reuse().is_none());
        })
        .join()
        .unwrap();
    }

    // The account has no directory on disk, so `.removing/` cannot be made
    // and the removal warns.
    unless_import_running(&fixture.state.db, &paths, ACCOUNT, |removal| {
        assert!(removal.make_or_reuse().is_none());
    })
    .instrument(tracing::info_span!("request", request_id = "the-request"))
    .await;

    let (lines, _) = read_lines(
        log_dir.path(),
        &LogLinesQuery {
            limit: 10,
            level: Some(LogLevel::Warn),
            ..LogLinesQuery::default()
        },
    )
    .unwrap();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0]
            .text
            .contains(r#"request{request_id="the-request"}"#),
        "{}",
        lines[0].text
    );
}

/// Deleting an account removes its directory with no lock held, after the
/// row's delete commits. A removal that gets the lock meanwhile moves
/// nothing, so it cannot put a `.removing/` directory where that removal
/// has already looked and leave the account's directory behind.
#[tokio::test]
async fn a_removal_for_an_account_whose_row_is_gone_moves_nothing() {
    let fixture = crate::test_support::test_fixture().await;
    let paths = fixture.state.cfg.paths.clone();
    let original = stored_original(&paths);

    let removed = sweep_unreferenced(&fixture.state.db, &paths, ACCOUNT)
        .await
        .unwrap();
    let moved = original.clone();
    unless_import_running(&fixture.state.db, &paths, ACCOUNT, move |removal| {
        take_out(ACCOUNT, &moved, Some(removal), remove_file);
    })
    .await;

    assert_eq!(removed, 0);
    assert!(original.is_file(), "the account's own delete removes it");
    assert!(!removing_dir(&paths, ACCOUNT).exists());
}

/// #1544: a crash between moving files into `.removing/` and deleting them
/// leaves them there, and the next sweep deletes them.
#[tokio::test]
async fn the_sweep_deletes_a_removing_directory_a_crash_left() {
    let fixture = fixture_with_removal_account().await;
    let paths = fixture.state.cfg.paths.clone();
    let left = removing_dir(&paths, ACCOUNT).join("left-by-a-crash");
    fs::create_dir_all(left.join("inside")).unwrap();
    fs::write(left.join(format!("{SHA}.jpg")), b"jpeg bytes").unwrap();
    let original = stored_original(&paths);

    let removed = sweep_unreferenced(&fixture.state.db, &paths, ACCOUNT)
        .await
        .unwrap();

    assert_eq!(removed, 1, "the unnamed original is swept");
    assert!(!original.exists());
    assert_eq!(left_in_removing(&paths), Vec::<PathBuf>::new());
}

/// An assets directory whose `.incoming/` holds one of each thing the sweep
/// meets: a `.part` temp two days old, a `.part` temp a live upload is
/// still writing, a file that is not a `.part`, a multipart session two
/// days old, and one still being uploaded. `began` is when the live
/// upload's files were written, for the sweep to run at.
struct Incoming {
    _dir: tempfile::TempDir,
    began: SystemTime,
    assets: PathBuf,
    stale_part: PathBuf,
    live_part: PathBuf,
    other_file: PathBuf,
    stale_session: PathBuf,
    fresh_session: PathBuf,
}

fn incoming_with_leftovers() -> Incoming {
    let dir = tempfile::tempdir().unwrap();
    let began = SystemTime::now();
    let assets = dir.path().join("assets");
    let incoming = assets.join(".incoming");
    fs::create_dir_all(&incoming).unwrap();
    let stale_part = incoming.join(format!("{}-1.part", "a".repeat(64)));
    fs::write(&stale_part, b"half an upload").unwrap();
    make_abandoned(&stale_part);
    let live_part = incoming.join(format!("{}-2.part", "d".repeat(64)));
    fs::write(&live_part, b"an upload in progress").unwrap();
    let other_file = incoming.join("notes.txt");
    fs::write(&other_file, b"not a temp").unwrap();

    let stale_session = incoming.join("b".repeat(64)).join("upload-stale");
    fs::create_dir_all(&stale_session).unwrap();
    let manifest = stale_session.join("manifest.json");
    fs::write(&manifest, b"{}").unwrap();
    make_abandoned(&manifest);

    let fresh_session = incoming.join("c".repeat(64)).join("upload-fresh");
    fs::create_dir_all(&fresh_session).unwrap();
    fs::write(fresh_session.join("manifest.json"), b"{}").unwrap();

    // A live upload's files were written as the sweep runs.
    for live in [&live_part, &fresh_session.join("manifest.json")] {
        written_at(live, began);
    }

    Incoming {
        _dir: dir,
        began,
        assets,
        stale_part,
        live_part,
        other_file,
        stale_session,
        fresh_session,
    }
}

#[test]
fn the_incoming_sweep_removes_part_temps_and_stale_sessions_and_nothing_else() {
    let incoming = incoming_with_leftovers();

    let removed = sweep_incoming(&incoming.assets, abandoned_at(incoming.began));

    assert_eq!(removed, 2, "the abandoned .part temp and the stale session");
    assert!(!incoming.stale_part.exists());
    assert!(!incoming.stale_session.exists());
    assert!(
        !incoming.stale_session.parent().unwrap().exists(),
        "the stale session's emptied sha directory goes too"
    );
    assert!(incoming.other_file.exists(), "only .part files are temps");
    assert!(
        incoming.live_part.exists(),
        "a .part file written inside the stall limit may be a live upload's, so it is kept"
    );
    assert!(
        incoming.fresh_session.join("manifest.json").exists(),
        "an upload still in progress is kept"
    );
}

/// An abandoned `.part` temp and a live one in the `.incoming/` of
/// `account_id`, returned in that order.
fn abandoned_and_live_part(paths: &PathsConfig, account_id: i64) -> (PathBuf, PathBuf) {
    let incoming = paths.assets_dir_for_account(account_id).join(".incoming");
    fs::create_dir_all(&incoming).unwrap();
    let abandoned = incoming.join(format!("{SHA}-1.part"));
    fs::write(&abandoned, b"half an upload").unwrap();
    make_abandoned(&abandoned);
    let live = incoming.join(format!("{SHA}-2.part"));
    fs::write(&live, b"an upload in progress").unwrap();
    // Ahead of any time the sweep reads the clock at, so the file is young
    // however long the test takes to reach the sweep.
    written_at(&live, SystemTime::now() + Duration::from_secs(24 * 60 * 60));
    (abandoned, live)
}

/// #2261: an upload start no longer sweeps `.incoming/`, so the end of an
/// Import Run does, and the uploads it left abandoned go then.
#[tokio::test]
async fn the_sweep_after_a_run_removes_abandoned_uploads() {
    let fixture = fixture_with_removal_account().await;
    let paths = fixture.state.cfg.paths.clone();
    let (abandoned, live) = abandoned_and_live_part(&paths, ACCOUNT);

    sweep_after_run(&fixture.state.db, &paths, ACCOUNT, STALL).await;

    assert!(!abandoned.exists());
    assert!(live.is_file());
}

/// #2261: a server that stopped with uploads open sweeps them as it next
/// starts, for every account, even when no Import Run ends after it.
#[tokio::test]
async fn the_sweep_as_the_server_starts_removes_every_accounts_abandoned_uploads() {
    let fixture = fixture_with_removal_account().await;
    fixture.account_with_id(ACCOUNT + 1, "another").await;
    let paths = fixture.state.cfg.paths.clone();
    let (abandoned, live) = abandoned_and_live_part(&paths, ACCOUNT);
    let (other_abandoned, other_live) = abandoned_and_live_part(&paths, ACCOUNT + 1);

    sweep_incoming_at_start(&fixture.state.db, &paths, STALL).await;

    assert!(!abandoned.exists());
    assert!(!other_abandoned.exists());
    assert!(live.is_file());
    assert!(other_live.is_file());
}

/// #2652: a `.part` file is abandoned once nothing was written to it for
/// the stall limit, the same limit a multipart session has.
#[test]
fn a_part_file_written_inside_the_stall_limit_is_kept_and_one_past_it_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join(format!("{SHA}-1.part"));
    fs::write(&part, b"half").unwrap();
    let written = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    written_at(&part, written);
    let listed = [part.clone()];
    let limit = STALL.as_secs();

    assert_eq!(
        remove_stale_files(
            &listed,
            written + STALL - Duration::from_secs(1),
            limit,
            false
        ),
        0
    );
    assert!(
        part.exists(),
        "a .part file written inside the limit is kept"
    );
    assert_eq!(
        remove_stale_files(&listed, written + STALL, limit, false),
        1
    );
    assert!(!part.exists(), "a .part file idle for the limit is removed");
}

/// The server removes a `.part` file itself when its upload finishes or
/// fails, so a file the sweep listed can be gone by the time the sweep
/// reaches it. That is not an error, and the sweep goes on to the rest.
#[test]
fn a_part_file_gone_between_the_listing_and_the_removal_does_not_stop_the_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join(format!("{}-1.part", "a".repeat(64)));
    let left = dir.path().join(format!("{}-2.part", "b".repeat(64)));
    for part in [&gone, &left] {
        fs::write(part, b"half").unwrap();
        make_abandoned(part);
    }
    let listed = [gone.clone(), left.clone()];
    fs::remove_file(&gone).unwrap();

    let removed = remove_stale_files(&listed, SystemTime::now(), STALL.as_secs(), false);

    assert_eq!(removed, 1, "only the file still there counts");
    assert!(!left.exists(), "the sweep went on past the missing file");
}

/// #2652: a request using an upload keeps it, however long ago its
/// manifest was written: a part arriving writes only its own temp and a
/// completion writes no manifest, so a slow part or a long completion would
/// otherwise look abandoned. Once the request ends, the upload is the sweep's again.
#[test]
fn the_incoming_sweep_leaves_an_upload_a_request_is_using() {
    let incoming = incoming_with_leftovers();
    let session_use = crate::asset_uploads::hold(&incoming.stale_session);
    let part_use = crate::asset_uploads::hold(&incoming.stale_part);

    assert_eq!(
        sweep_incoming(&incoming.assets, abandoned_at(incoming.began)),
        0
    );
    assert!(incoming.stale_session.join("manifest.json").exists());
    assert!(incoming.stale_part.exists());

    drop((session_use, part_use));
    assert_eq!(
        sweep_incoming(&incoming.assets, abandoned_at(incoming.began)),
        2
    );
}

/// #2652: the sweep sets an abandoned upload's directory aside under a
/// `.removing-` name before deleting it. One a stopped sweep left there is
/// deleted by the next.
#[test]
fn the_incoming_sweep_deletes_a_directory_a_stopped_sweep_set_aside() {
    let incoming = incoming_with_leftovers();
    let aside = incoming.assets.join(".incoming").join(".removing-abc123");
    fs::create_dir_all(&aside).unwrap();
    fs::write(aside.join("manifest.json"), b"{}").unwrap();
    fs::write(aside.join("part-0001"), b"half").unwrap();

    sweep_incoming(&incoming.assets, abandoned_at(incoming.began));

    assert!(!aside.exists());
    assert!(incoming.fresh_session.join("manifest.json").exists());
}
