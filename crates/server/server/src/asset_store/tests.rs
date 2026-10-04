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

#[test]
fn an_upload_session_is_stale_after_a_day_by_its_manifest_or_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let with_manifest = dir.path().join("with-manifest");
    fs::create_dir_all(&with_manifest).unwrap();
    fs::write(with_manifest.join("manifest.json"), b"{}").unwrap();
    let without_manifest = dir.path().join("without-manifest");
    fs::create_dir_all(&without_manifest).unwrap();
    let now = SystemTime::now();
    let limit = Duration::from_secs(STALE_UPLOAD_SECS);

    assert!(!upload_session_is_stale(&with_manifest, now).unwrap());
    assert!(!upload_session_is_stale(&without_manifest, now).unwrap());
    assert!(upload_session_is_stale(&with_manifest, now + limit).unwrap());
    assert!(upload_session_is_stale(&without_manifest, now + limit).unwrap());
    assert!(!upload_session_is_stale(&with_manifest, now + limit / 2).unwrap());
}

/// The limit is a day on the clock: an upload left alone for 23 hours may
/// still be resumed, and one left for 25 hours is abandoned.
#[test]
fn an_upload_session_idle_for_23_hours_is_kept_and_one_idle_for_25_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("upload");
    fs::create_dir_all(&session).unwrap();
    fs::write(session.join("manifest.json"), b"{}").unwrap();
    let now = SystemTime::now();
    let hours = |n: u64| Duration::from_secs(n * 3600);

    assert!(!upload_session_is_stale(&session, now + hours(23)).unwrap());
    assert!(upload_session_is_stale(&session, now + hours(25)).unwrap());
}

/// Set `path`'s modified time to two days ago, twice the upload age limit,
/// so the `.incoming/` sweep treats it as abandoned.
pub(crate) fn make_abandoned(path: &Path) {
    let two_days_ago = SystemTime::now() - Duration::from_secs(2 * STALE_UPLOAD_SECS);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(two_days_ago)
        .unwrap();
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

    let removed = sweep_store_dir(1, dir.path(), &HashSet::new(), PREVIEW_GRACE_SECS, None);

    assert_eq!(removed, 1);
    assert!(fresh.is_file(), "a Preview its row may not name yet stays");
    assert!(!old.exists(), "an old unnamed Preview goes");
}

/// The account the database-backed tests below remove files for. No row
/// needs it: the removals read the import runs and attachments of an id.
const ACCOUNT: i64 = 7;

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
    let fixture = crate::test_support::test_fixture().await;
    let pool = fixture.state.db.clone();
    let paths = fixture.state.cfg.paths.clone();
    let original = stored_original(&paths);
    let (delete, deletion_started, finish) = paused_deletion();

    let removal = tokio::spawn({
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
    let fixture = crate::test_support::test_fixture().await;
    let pool = fixture.state.db.clone();
    let paths = fixture.state.cfg.paths.clone();
    let original = stored_original(&paths);
    let (delete, deletion_started, finish) = paused_deletion();

    let sweep = tokio::spawn({
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

/// A removal that finds nothing to move makes no `.removing/` directory,
/// and so cannot bring back the directory of an account deleted meanwhile.
#[tokio::test]
async fn a_removal_with_nothing_to_move_leaves_nothing_on_disk() {
    let fixture = crate::test_support::test_fixture().await;
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
        "a missing account directory stays missing"
    );
}

/// #1544: a crash between moving files into `.removing/` and deleting them
/// leaves them there, and the next sweep deletes them.
#[tokio::test]
async fn the_sweep_deletes_a_removing_directory_a_crash_left() {
    let fixture = crate::test_support::test_fixture().await;
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

/// An assets folder whose `.incoming/` holds one of each thing the sweep
/// meets: a `.part` temp two days old, a `.part` temp a live upload is
/// still writing, a file that is not a `.part`, a multipart session two
/// days old, and one still being uploaded.
struct Incoming {
    _dir: tempfile::TempDir,
    assets: PathBuf,
    stale_part: PathBuf,
    live_part: PathBuf,
    other_file: PathBuf,
    stale_session: PathBuf,
    fresh_session: PathBuf,
}

fn incoming_with_leftovers() -> Incoming {
    let dir = tempfile::tempdir().unwrap();
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

    Incoming {
        _dir: dir,
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

    let removed = sweep_incoming(&incoming.assets, false);

    assert_eq!(removed, 2, "the abandoned .part temp and the stale session");
    assert!(!incoming.stale_part.exists());
    assert!(!incoming.stale_session.exists());
    assert!(
        !incoming.stale_session.parent().unwrap().exists(),
        "the stale session's emptied sha folder goes too"
    );
    assert!(incoming.other_file.exists(), "only .part files are temps");
    assert!(
        incoming.live_part.exists(),
        "a .part file younger than a day may be a live upload's, so it is kept"
    );
    assert!(
        incoming.fresh_session.join("manifest.json").exists(),
        "an upload still in progress is kept"
    );
}

/// A `.part` file is abandoned after a day on the clock, the same limit a
/// multipart session has.
#[test]
fn a_part_file_idle_for_23_hours_is_kept_and_one_idle_for_25_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join(format!("{SHA}-1.part"));
    fs::write(&part, b"half").unwrap();
    let listed = [part.clone()];
    let now = SystemTime::now();
    let hours = |n: u64| Duration::from_secs(n * 3600);

    assert_eq!(
        remove_stale_files(&listed, now + hours(23), STALE_UPLOAD_SECS, false),
        0
    );
    assert!(part.exists(), "a .part file 23 hours old is kept");
    assert_eq!(
        remove_stale_files(&listed, now + hours(25), STALE_UPLOAD_SECS, false),
        1
    );
    assert!(!part.exists(), "a .part file 25 hours old is removed");
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

    let removed = remove_stale_files(&listed, SystemTime::now(), STALE_UPLOAD_SECS, false);

    assert_eq!(removed, 1, "only the file still there counts");
    assert!(!left.exists(), "the sweep went on past the missing file");
}

#[test]
fn a_dry_run_of_the_incoming_sweep_counts_what_it_would_remove_and_removes_nothing() {
    let incoming = incoming_with_leftovers();

    let removed = sweep_incoming(&incoming.assets, true);

    assert_eq!(removed, 2, "the abandoned .part temp and the stale session");
    for kept in [
        &incoming.stale_part,
        &incoming.live_part,
        &incoming.other_file,
        &incoming.stale_session.join("manifest.json"),
        &incoming.fresh_session.join("manifest.json"),
    ] {
        assert!(kept.exists(), "a dry run removed {}", kept.display());
    }
}
