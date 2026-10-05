use std::future::Future;

use super::*;
use crate::asset_store::tests::make_abandoned;
use crate::config::PathsConfig;
use crate::db::engine;
use media::testutil::PNG_1X1_RGB;

/// A stop that is never set, for a pass that runs to its end.
static NOT_STOPPED: AtomicBool = AtomicBool::new(false);

const SHA: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

/// A row for a stored blob named `assets_path`, with nothing else known.
fn row(assets_path: &str) -> StoredOriginal {
    StoredOriginal {
        sha256: SHA.to_string(),
        assets_path: assets_path.to_string(),
        mime_type: None,
        derived_assets_path: None,
        derived_sha256: None,
        derived_mime_type: None,
        rows_without_preview: 1,
        thumbnail_assets_path: None,
        thumbnail_sha256: None,
        thumbnail_mime_type: None,
        rows_without_thumbnail: 1,
        original_name: None,
        source_path: None,
    }
}

/// The original is on disk, of a type browsers often cannot show, and has
/// no versions yet: the state a fresh import of a HEIC photo leaves.
const FRESH: OnDisk = OnDisk {
    original_exists: true,
    preview: PreviewFile::Missing,
    thumbnail: PreviewFile::Missing,
    browser_shows: false,
};

/// [`FRESH`], of a type every browser shows as it is.
const FRESH_SHOWN: OnDisk = OnDisk {
    browser_shows: true,
    ..FRESH
};

/// What the plan says each version of a `kind` original needs.
fn versions(kind: Kind, thumbnail: Need, preview: Need) -> Plan {
    Plan::Versions(Versions {
        kind,
        thumbnail,
        preview,
    })
}

/// A pass over `assets_dir` for account 7.
fn pass<'a>(
    opts: &'a ProcessAssetsOptions,
    work_dir: &'a Path,
    assets_dir: &Path,
    converted_dir: &Path,
) -> AccountPass<'a> {
    AccountPass {
        opts,
        work_dir,
        stop: &NOT_STOPPED,
        account_id: 7,
        assets_dir: assets_dir.to_path_buf(),
        converted_dir: converted_dir.to_path_buf(),
        log: Log::Print,
    }
}

#[test]
fn derived_rel_path_layout() {
    assert_eq!(
        derived_rel_path(&crate::assets_api::Sha256::parse(SHA).unwrap(), ".jpg"),
        format!("ab/{SHA}.jpg")
    );
    assert_eq!(
        derived_rel_path(&crate::assets_api::Sha256::parse(SHA).unwrap(), ".jpeg"),
        format!("ab/{SHA}.jpg")
    );
}

#[test]
fn a_part_path_is_removed_and_never_converted() {
    let opts = ProcessAssetsOptions::default();
    let mut part = row("aa/upload.part");
    part.mime_type = Some("video/mp4".to_string());
    part.original_name = Some("clip.mp4".to_string());
    assert_eq!(plan(&part, &opts, FRESH), Plan::RemoveIncomplete);
    // Even when the file is already gone the plan is the same; the executor
    // deals with an absent file.
    let gone = OnDisk {
        original_exists: false,
        ..FRESH
    };
    assert_eq!(plan(&part, &opts, gone), Plan::RemoveIncomplete);
}

#[test]
fn a_blob_that_is_not_media_is_skipped() {
    let opts = ProcessAssetsOptions::default();
    assert_eq!(
        plan(&row("aa/notes.txt"), &opts, FRESH),
        Plan::Skip(SkipReason::NotMedia)
    );
    let mut pdf = row(&format!("ab/{SHA}"));
    pdf.mime_type = Some("application/pdf".to_string());
    pdf.original_name = Some("clip.mp4".to_string());
    assert_eq!(plan(&pdf, &opts, FRESH), Plan::Skip(SkipReason::NotMedia));
}

#[test]
fn a_gif_gets_a_thumbnail_and_no_preview_because_it_is_an_animation() {
    let opts = ProcessAssetsOptions::default();
    let want = versions(Kind::Image, Need::Make, Need::Nothing);
    assert_eq!(plan(&row("aa/photo.gif"), &opts, FRESH_SHOWN), want);
    let mut declared = row(&format!("ab/{SHA}"));
    declared.mime_type = Some("image/gif".to_string());
    assert_eq!(plan(&declared, &opts, FRESH_SHOWN), want);
}

#[test]
fn an_image_or_a_video_gets_a_thumbnail_and_audio_none() {
    let opts = ProcessAssetsOptions::default();
    assert_eq!(
        plan(&row("aa/photo.heic"), &opts, FRESH),
        versions(Kind::Image, Need::Make, Need::Make)
    );
    assert_eq!(
        plan(&row("aa/clip.mov"), &opts, FRESH),
        versions(Kind::Video, Need::Make, Need::Make)
    );
    assert_eq!(
        plan(&row("aa/memo.amr"), &opts, FRESH),
        versions(Kind::Audio, Need::Nothing, Need::Make)
    );
}

#[test]
fn an_original_every_browser_shows_gets_no_preview() {
    let opts = ProcessAssetsOptions::default();
    assert_eq!(
        plan(&row("aa/photo.jpg"), &opts, FRESH_SHOWN),
        versions(Kind::Image, Need::Make, Need::Nothing)
    );
    assert_eq!(
        plan(&row("aa/clip.mp4"), &opts, FRESH_SHOWN),
        versions(Kind::Video, Need::Make, Need::Nothing)
    );
    assert_eq!(
        plan(&row("aa/song.mp3"), &opts, FRESH_SHOWN),
        versions(Kind::Audio, Need::Nothing, Need::Nothing)
    );
}

#[test]
fn an_extensionless_blob_is_planned_by_its_declared_mime_or_its_attachment_name() {
    let opts = ProcessAssetsOptions::default();
    let mut by_mime = row(&format!("ab/{SHA}"));
    by_mime.mime_type = Some("image/heic".to_string());
    assert_eq!(
        plan(&by_mime, &opts, FRESH),
        versions(Kind::Image, Need::Make, Need::Make)
    );
    let mut by_name = row(&format!("ab/{SHA}"));
    by_name.original_name = Some("voice-note.amr".to_string());
    assert_eq!(
        plan(&by_name, &opts, FRESH),
        versions(Kind::Audio, Need::Nothing, Need::Make)
    );
}

#[test]
fn each_skip_option_turns_off_its_kind_and_nothing_else() {
    let photo = row("aa/photo.heic");
    let clip = row("aa/clip.mov");
    let memo = row("aa/memo.amr");
    let off = Plan::Skip(SkipReason::KindDisabled);
    let image = versions(Kind::Image, Need::Make, Need::Make);
    let video = versions(Kind::Video, Need::Make, Need::Make);
    let audio = versions(Kind::Audio, Need::Nothing, Need::Make);
    for (opts, want) in [
        (
            ProcessAssetsOptions {
                skip_image: true,
                ..Default::default()
            },
            [off, video, audio],
        ),
        (
            ProcessAssetsOptions {
                skip_video: true,
                ..Default::default()
            },
            [image, off, audio],
        ),
        (
            ProcessAssetsOptions {
                skip_audio: true,
                ..Default::default()
            },
            [image, video, off],
        ),
    ] {
        let got = [&photo, &clip, &memo].map(|row| plan(row, &opts, FRESH));
        assert_eq!(got, want, "{opts:?}");
    }
}

#[test]
fn an_existing_version_is_kept_unless_force_is_given() {
    let made = OnDisk {
        preview: PreviewFile::Intact,
        thumbnail: PreviewFile::Intact,
        ..FRESH
    };
    let photo = row("aa/photo.heic");
    assert_eq!(
        plan(&photo, &ProcessAssetsOptions::default(), made),
        versions(Kind::Image, Need::Share, Need::Share)
    );
    let force = ProcessAssetsOptions {
        force: true,
        ..Default::default()
    };
    assert_eq!(
        plan(&photo, &force, made),
        versions(Kind::Image, Need::Make, Need::Make)
    );
    let damaged = OnDisk {
        preview: PreviewFile::Damaged,
        thumbnail: PreviewFile::Damaged,
        ..FRESH
    };
    assert_eq!(
        plan(&photo, &ProcessAssetsOptions::default(), damaged),
        versions(Kind::Image, Need::Make, Need::Make),
        "a damaged version is made again without --force"
    );
}

#[test]
fn a_preview_the_original_would_no_longer_get_is_kept_or_dropped_when_damaged() {
    let photo = row("aa/photo.png");
    let kept = OnDisk {
        preview: PreviewFile::Intact,
        ..FRESH_SHOWN
    };
    let force = ProcessAssetsOptions {
        force: true,
        ..Default::default()
    };
    assert_eq!(
        plan(&photo, &force, kept),
        versions(Kind::Image, Need::Make, Need::Share),
        "--force does not make again a Preview the original no longer gets"
    );
    let damaged = OnDisk {
        preview: PreviewFile::Damaged,
        ..FRESH_SHOWN
    };
    assert_eq!(
        plan(&photo, &ProcessAssetsOptions::default(), damaged),
        versions(Kind::Image, Need::Make, Need::Drop)
    );
}

#[test]
fn a_disabled_kind_is_reported_before_an_existing_version() {
    let opts = ProcessAssetsOptions {
        skip_image: true,
        ..Default::default()
    };
    let made = OnDisk {
        preview: PreviewFile::Intact,
        thumbnail: PreviewFile::Intact,
        ..FRESH
    };
    assert_eq!(
        plan(&row("aa/photo.heic"), &opts, made),
        Plan::Skip(SkipReason::KindDisabled)
    );
}

#[test]
fn a_missing_original_is_an_error_only_when_a_version_is_wanted() {
    let opts = ProcessAssetsOptions::default();
    let missing = OnDisk {
        original_exists: false,
        ..FRESH
    };
    let no_original = Need::NoOriginal { damaged: false };
    assert_eq!(
        plan(&row("aa/photo.heic"), &opts, missing),
        versions(Kind::Image, no_original, no_original)
    );
    // A version already on disk, or one nobody wants, needs no original.
    let missing_but_made = OnDisk {
        original_exists: false,
        preview: PreviewFile::Intact,
        thumbnail: PreviewFile::Intact,
        browser_shows: false,
    };
    assert_eq!(
        plan(&row("aa/photo.heic"), &opts, missing_but_made),
        versions(Kind::Image, Need::Share, Need::Share)
    );
    let missing_shown = OnDisk {
        original_exists: false,
        ..FRESH_SHOWN
    };
    assert_eq!(
        plan(&row("aa/song.mp3"), &opts, missing_shown),
        versions(Kind::Audio, Need::Nothing, Need::Nothing)
    );
    assert_eq!(
        plan(&row("aa/notes.txt"), &opts, missing),
        Plan::Skip(SkipReason::NotMedia)
    );
    // A damaged version with no original to make it from again is dropped.
    let missing_and_damaged = OnDisk {
        original_exists: false,
        preview: PreviewFile::Damaged,
        thumbnail: PreviewFile::Missing,
        browser_shows: false,
    };
    assert_eq!(
        plan(&row("aa/photo.heic"), &opts, missing_and_damaged),
        versions(Kind::Image, no_original, Need::NoOriginal { damaged: true })
    );
}

#[test]
fn preview_file_hashes_the_preview_against_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"jpeg-bytes";
    let sha = crate::assets_api::sha256_hex(bytes);
    let rel = format!("{}/{sha}.jpg", &sha[..2]);
    let dest = dir.path().join(&rel);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(&dest, bytes).unwrap();
    assert_eq!(preview_file(Some(&rel), dir.path()), PreviewFile::Intact);

    fs::write(&dest, &bytes[..4]).unwrap();
    assert_eq!(
        preview_file(Some(&rel), dir.path()),
        PreviewFile::Damaged,
        "a Preview cut short does not hash to its name"
    );

    let unnamed = "ab/preview.jpg";
    fs::create_dir_all(dir.path().join("ab")).unwrap();
    fs::write(dir.path().join(unnamed), bytes).unwrap();
    assert_eq!(
        preview_file(Some(unnamed), dir.path()),
        PreviewFile::Damaged
    );
    assert_eq!(
        preview_file(Some("missing.jpg"), dir.path()),
        PreviewFile::Missing
    );
    assert_eq!(
        preview_file(Some("../escape.jpg"), dir.path()),
        PreviewFile::Missing
    );
    assert_eq!(preview_file(Some(""), dir.path()), PreviewFile::Missing);
    assert_eq!(preview_file(None, dir.path()), PreviewFile::Missing);
}

#[test]
fn part_paths_are_recognised_in_any_case() {
    assert!(is_part_path("aa/aabbcc.part"));
    assert!(is_part_path("upload.PART"));
    assert!(!is_part_path("aa/aabbcc.mp4"));
    assert!(!is_part_path("aa/aabbcc"));
}

#[test]
fn a_label_names_the_account_and_the_stored_path() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), dir.path());
    assert_eq!(pass.label(&row("aa/photo.jpg")), "7/aa/photo.jpg");
}

#[test]
fn removing_an_incomplete_upload_deletes_the_part_file() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    fs::create_dir_all(assets.join("aa")).unwrap();
    let part = assets.join("aa/upload.part");
    fs::write(&part, b"half").unwrap();
    let pass = pass(&opts, dir.path(), &assets, dir.path());

    pass.remove_incomplete(&row("aa/upload.part"), &part)
        .unwrap();

    assert!(!part.exists());
    // A file that is already gone is not an error.
    pass.remove_incomplete(&row("aa/upload.part"), &part)
        .unwrap();
}

#[test]
fn a_dry_run_leaves_the_part_file_in_place() {
    let opts = ProcessAssetsOptions {
        dry_run: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("assets");
    fs::create_dir_all(assets.join("aa")).unwrap();
    let part = assets.join("aa/upload.part");
    fs::write(&part, b"half").unwrap();
    let pass = pass(&opts, dir.path(), &assets, dir.path());

    pass.remove_incomplete(&row("aa/upload.part"), &part)
        .unwrap();

    assert!(part.is_file());
}

#[test]
fn a_work_file_is_stored_content_addressed_and_then_removed() {
    let opts = ProcessAssetsOptions::default();
    let dir = tempfile::tempdir().unwrap();
    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let out = dir.path().join("out-abcdef012345.jpg");
    fs::write(&out, b"jpeg-bytes").unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), &converted);

    let stored = pass
        .store_work_file(&out, Version::Thumbnail, ".jpg", &row("aa/photo.jpg"))
        .unwrap();

    let expected = VersionFile {
        sha256: crate::assets_api::sha256_hex(b"jpeg-bytes"),
        assets_path: derived_rel_path(&crate::assets_api::Sha256::of_bytes(b"jpeg-bytes"), ".jpg"),
        mime_type: "image/jpeg".to_string(),
    };
    assert_eq!(stored, Derived::Stored(expected.clone()));
    assert_eq!(
        fs::read(converted.join(&expected.assets_path)).unwrap(),
        b"jpeg-bytes"
    );
    assert!(!out.exists(), "the work file is removed once stored");
}

#[test]
fn a_dry_run_stores_nothing_and_still_removes_the_work_file() {
    let opts = ProcessAssetsOptions {
        dry_run: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let out = dir.path().join("out-abcdef012345.jpg");
    fs::write(&out, b"jpeg-bytes").unwrap();
    let pass = pass(&opts, dir.path(), dir.path(), &converted);

    let stored = pass
        .store_work_file(&out, Version::Thumbnail, ".jpg", &row("aa/photo.jpg"))
        .unwrap();

    assert_eq!(stored, Derived::DryRun);
    assert!(!out.exists());
    assert_eq!(fs::read_dir(&converted).unwrap().count(), 0);
}

/// The account every database test seeds.
pub(crate) const ACCOUNT: i64 = 7;

/// A 1x1 24-bit BMP: an image browsers often cannot show, so it gets a
/// Preview as well as a Thumbnail.
#[rustfmt::skip]
pub(crate) const BMP_1X1: &[u8] = &[
    0x42, 0x4d, 0x3a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x28, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x18, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x13, 0x0b, 0x00, 0x00, 0x13, 0x0b, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x40, 0x80, 0x00,
];

/// An opened fresh database with the schema applied and its data directory
/// under a temp dir, the shape [`run`] is handed by the command line.
async fn open_db() -> (OpenDb, tempfile::TempDir) {
    let (pool, dir) = engine::test_pool().await;
    schema::ensure_schema(&mut pool.acquire().await.unwrap())
        .await
        .unwrap();
    let cfg = Config {
        paths: PathsConfig {
            db: dir.path().join("messagecrate.db"),
            data_dir: dir.path().join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        },
        server: None,
    };
    (OpenDb { cfg, db: pool }, dir)
}

async fn seed_account(conn: &mut SqliteConnection, id: i64) {
    sqlx::query("INSERT INTO accounts (id, username) VALUES ($1, $2)")
        .bind(id)
        .bind(format!("user{id}"))
        .execute(&mut *conn)
        .await
        .unwrap();
}

/// One conversation with one message under `source` for [`ACCOUNT`],
/// returning the message id an attachment can hang off.
pub(crate) async fn seed_message(conn: &mut SqliteConnection, source: &str) -> i64 {
    let handle_id: i64 = sqlx::query_scalar(
        "INSERT INTO handles (account_id, raw, normalized, handle_type, service)
         VALUES ($1, $2, $2, 'phone', 'phone') RETURNING id",
    )
    .bind(ACCOUNT)
    .bind(format!("+1555{source}"))
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    let conversation_id: i64 = sqlx::query_scalar(
        "INSERT INTO conversations (account_id, chat_handle_id, conversation_type, source_file)
         VALUES ($1, $2, 'individual', 't') RETURNING id",
    )
    .bind(ACCOUNT)
    .bind(handle_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    crate::test_support::MessageRow {
        source,
        ..crate::test_support::MessageRow::new(ACCOUNT, conversation_id)
    }
    .insert(conn)
    .await
}

/// Store `bytes` as the original for an attachment of `message_id`, the way
/// an import leaves it: the blob at `<aa>/<sha><ext>` in the account's
/// assets directory and a row pointing at it. Returns the attachment id.
pub(crate) async fn attach_stored_blob(
    opened: &OpenDb,
    conn: &mut SqliteConnection,
    message_id: i64,
    sha: &str,
    ext: &str,
    bytes: &[u8],
) -> i64 {
    let rel = format!("{}/{sha}{ext}", &sha[..2]);
    let path = opened.cfg.paths.assets_dir_for_account(ACCOUNT).join(&rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    let mut tx = crate::db::begin_write(conn).await.unwrap();
    let id = sqlx::query_scalar(
        "INSERT INTO attachments (message_id, sha256, assets_path) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(message_id)
    .bind(sha)
    .bind(rel)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

/// A database with one account and one attachment on a message of
/// `source`, stored as `<sha><ext>` with `bytes`.
async fn fixture_with(source: &str, ext: &str, bytes: &[u8]) -> (OpenDb, tempfile::TempDir, i64) {
    let (opened, dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, source).await;
    let attachment_id = attach_stored_blob(&opened, &mut conn, message_id, SHA, ext, bytes).await;
    (opened, dir, attachment_id)
}

/// [`fixture_with`] a BMP, which gets a Preview and a Thumbnail.
async fn fixture_with_bmp(source: &str) -> (OpenDb, tempfile::TempDir, i64) {
    fixture_with(source, ".bmp", BMP_1X1).await
}

/// The Thumbnail columns of one attachment row, `None` until one is recorded.
async fn thumbnail_of(
    conn: &mut SqliteConnection,
    attachment_id: i64,
) -> Option<(String, String, String)> {
    let (sha, path, mime): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT thumbnail_sha256, thumbnail_assets_path, thumbnail_mime_type
         FROM attachments WHERE id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    Some((sha?, path?, mime?))
}

/// The derived columns of one attachment row, `None` until a preview is recorded.
async fn derived_of(
    conn: &mut SqliteConnection,
    attachment_id: i64,
) -> Option<(String, String, String)> {
    let (sha, path, mime): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT derived_sha256, derived_assets_path, derived_mime_type FROM attachments WHERE id = $1",
    )
    .bind(attachment_id)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    Some((sha?, path?, mime?))
}

/// Run `test` on its own runtime with the real ffmpeg held available, or
/// skip it the way every ffmpeg test in the workspace skips (and fail under
/// CI). The guard is taken outside the async block: holding it across an
/// await is what Clippy's `await_holding_lock` refuses.
fn with_real_ffmpeg(test: impl Future<Output = ()>) {
    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}

fn stats(
    scanned: u64,
    derived: u64,
    thumbnails: u64,
    skipped: u64,
    errors: u64,
) -> ProcessAssetsStats {
    ProcessAssetsStats {
        scanned,
        derived,
        thumbnails,
        skipped,
        errors,
    }
}

#[tokio::test]
async fn store_and_update_derived_db() {
    let (opened, dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, "imessage").await;
    let attachment_id = attach_stored_blob(&opened, &mut conn, message_id, SHA, ".jpg", b"x").await;

    let converted = dir.path().join("converted");
    fs::create_dir_all(&converted).unwrap();
    let blob = store_derived_bytes(&converted, b"jpeg-bytes", ".jpg").unwrap();
    assert!(converted.join(&blob.assets_path).is_file());

    versions_db::record(&mut conn, Version::Preview, ACCOUNT, SHA, &blob)
        .await
        .unwrap();

    assert_eq!(
        derived_of(&mut conn, attachment_id).await,
        Some((blob.sha256, blob.assets_path, "image/jpeg".to_string()))
    );
}

#[tokio::test]
async fn listed_attachments_carry_name_hints_for_extensionless_blobs() {
    let (opened, _dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, "imessage").await;
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query(
        "INSERT INTO attachments (message_id, sha256, assets_path, mime_type, original_name, path)
         VALUES ($1, $2, $3, NULL, 'voice-note.amr', 'attachments/voice-note.amr')",
    )
    .bind(message_id)
    .bind(SHA)
    .bind(format!("ab/{SHA}"))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let rows = versions_db::stored_originals(&mut conn, ACCOUNT, None)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        plan(&rows[0], &ProcessAssetsOptions::default(), FRESH),
        versions(Kind::Audio, Need::Nothing, Need::Make),
        "an extensionless blob with no declared MIME must classify from its attachment name"
    );
}

#[test]
fn a_run_writes_a_jpeg_preview_under_the_converted_directory_and_records_it() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with_bmp("imessage").await;
        let opts = ProcessAssetsOptions::default();

        let first = run(&opened, &opts, &NOT_STOPPED).await.unwrap();

        assert_eq!(first, stats(1, 1, 1, 0, 0));
        let mut conn = opened.conn().await.unwrap();
        let (sha, rel, mime) = derived_of(&mut conn, attachment_id)
            .await
            .expect("the row points at its preview");
        let preview = opened
            .cfg
            .paths
            .assets_converted_dir_for_account(ACCOUNT)
            .join(&rel);
        let bytes = fs::read(&preview).expect("the preview is under assets_converted/");
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG starts with SOI");
        assert_eq!(sha, crate::assets_api::sha256_hex(&bytes));
        assert_eq!(
            rel,
            derived_rel_path(&crate::assets_api::Sha256::parse(&sha).unwrap(), ".jpg")
        );
        assert_eq!(mime, "image/jpeg");

        // A second run leaves the preview alone; `force` makes it again.
        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 0, 0, 1, 0)
        );
        let force = ProcessAssetsOptions {
            force: true,
            ..Default::default()
        };
        assert_eq!(
            run(&opened, &force, &NOT_STOPPED).await.unwrap(),
            stats(1, 1, 1, 0, 0)
        );
    });
}

#[test]
fn a_dry_run_counts_the_preview_it_would_write_and_writes_nothing() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with_bmp("imessage").await;
        let opts = ProcessAssetsOptions {
            dry_run: true,
            ..Default::default()
        };

        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 1, 1, 0, 0)
        );

        let converted = opened.cfg.paths.assets_converted_dir_for_account(ACCOUNT);
        assert_eq!(fs::read_dir(&converted).unwrap().count(), 0);
        let mut conn = opened.conn().await.unwrap();
        assert_eq!(derived_of(&mut conn, attachment_id).await, None);
    });
}

/// The Preview of the one attachment in `opened`, read from the row that
/// names it: its fingerprint and its path on disk.
async fn preview_on_disk(opened: &OpenDb, attachment_id: i64) -> (String, PathBuf) {
    let mut conn = opened.conn().await.unwrap();
    let (sha, rel, _) = derived_of(&mut conn, attachment_id)
        .await
        .expect("the row points at its preview");
    let path = opened
        .cfg
        .paths
        .assets_converted_dir_for_account(ACCOUNT)
        .join(rel);
    (sha, path)
}

/// A Preview cut short by a killed run no longer hashes to the fingerprint
/// in its name. A plain run, without `--force`, converts it again, so the
/// file served is whole.
#[test]
fn a_plain_run_converts_again_a_preview_cut_short() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with_bmp("imessage").await;
        let opts = ProcessAssetsOptions::default();
        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 1, 1, 0, 0)
        );
        let (sha, preview) = preview_on_disk(&opened, attachment_id).await;
        let whole = fs::read(&preview).unwrap();
        fs::write(&preview, &whole[..whole.len() / 2]).unwrap();

        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 1, 0, 0, 0)
        );

        let (sha_after, preview_after) = preview_on_disk(&opened, attachment_id).await;
        let bytes = fs::read(&preview_after).unwrap();
        assert_eq!(crate::assets_api::sha256_hex(&bytes), sha_after);
        assert_eq!(
            (sha_after, preview_after),
            (sha, preview),
            "the same original converts to the same Preview"
        );
    });
}

/// A Preview whose bytes hash to the fingerprint in its name is left as it
/// is by a plain run: not converted, not rewritten.
#[test]
fn a_plain_run_skips_a_preview_that_hashes_to_its_name() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with_bmp("imessage").await;
        let opts = ProcessAssetsOptions::default();
        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 1, 1, 0, 0)
        );
        let (_, preview) = preview_on_disk(&opened, attachment_id).await;
        let written = fs::metadata(&preview).unwrap().modified().unwrap();

        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 0, 0, 1, 0)
        );

        assert_eq!(
            fs::metadata(&preview).unwrap().modified().unwrap(),
            written,
            "the Preview is not written again"
        );
    });
}

/// A file imported again from a second source after its Preview was made:
/// the new rows name no Preview yet. The next run finds the Preview on disk
/// and does not convert again, but it points the new rows at that Preview,
/// so the web app asks for it for both sources.
#[test]
fn a_second_source_imported_after_the_preview_was_made_gets_the_preview() {
    with_real_ffmpeg(async {
        let (opened, _dir, imessage_attachment) = fixture_with_bmp("imessage").await;
        let opts = ProcessAssetsOptions::default();
        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 1, 1, 0, 0)
        );
        let mut conn = opened.conn().await.unwrap();
        let message_id = seed_message(&mut conn, "whatsapp").await;
        let whatsapp_attachment =
            attach_stored_blob(&opened, &mut conn, message_id, SHA, ".bmp", BMP_1X1).await;
        assert_eq!(derived_of(&mut conn, whatsapp_attachment).await, None);

        assert_eq!(
            run(&opened, &opts, &NOT_STOPPED).await.unwrap(),
            stats(1, 0, 0, 1, 0)
        );

        let preview = derived_of(&mut conn, imessage_attachment).await;
        assert!(preview.is_some());
        assert_eq!(derived_of(&mut conn, whatsapp_attachment).await, preview);
        let thumbnail = thumbnail_of(&mut conn, imessage_attachment).await;
        assert!(thumbnail.is_some());
        assert_eq!(
            thumbnail_of(&mut conn, whatsapp_attachment).await,
            thumbnail
        );
    });
}

/// An image every browser shows, such as a PNG, gets a Thumbnail and no
/// Preview: the Thumbnail is a JPEG under the converted directory, named by
/// the fingerprint of its own bytes, and the row says so.
#[test]
fn a_png_gets_a_thumbnail_and_no_preview() {
    with_real_ffmpeg(async {
        let (opened, _dir, attachment_id) = fixture_with("imessage", ".png", PNG_1X1_RGB).await;

        assert_eq!(
            run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
                .await
                .unwrap(),
            stats(1, 0, 1, 0, 0)
        );

        let mut conn = opened.conn().await.unwrap();
        assert_eq!(derived_of(&mut conn, attachment_id).await, None);
        let (sha, rel, mime) = thumbnail_of(&mut conn, attachment_id)
            .await
            .expect("the row names its Thumbnail");
        assert_eq!(mime, "image/jpeg");
        let bytes = fs::read(
            opened
                .cfg
                .paths
                .assets_converted_dir_for_account(ACCOUNT)
                .join(&rel),
        )
        .unwrap();
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG starts with SOI");
        assert_eq!(sha, crate::assets_api::sha256_hex(&bytes));
    });
}

/// The background pass processes one queued Asset, and no other attachment
/// of the account.
#[test]
fn one_asset_is_processed_alone() {
    with_real_ffmpeg(async {
        let (opened, _dir, queued) = fixture_with("imessage", ".png", PNG_1X1_RGB).await;
        let mut conn = opened.conn().await.unwrap();
        let message_id = seed_message(&mut conn, "sms").await;
        let other_sha = "b".repeat(64);
        let other = attach_stored_blob(
            &opened,
            &mut conn,
            message_id,
            &other_sha,
            ".png",
            PNG_1X1_RGB,
        )
        .await;
        let work = tempfile::tempdir().unwrap();

        let made = process_one_asset(
            &opened.cfg,
            &opened.db,
            work.path(),
            ACCOUNT,
            SHA,
            &NOT_STOPPED,
        )
        .await
        .unwrap();

        assert_eq!(made, stats(1, 0, 1, 0, 0));
        assert!(thumbnail_of(&mut conn, queued).await.is_some());
        assert_eq!(thumbnail_of(&mut conn, other).await, None);
    });
}

/// One file named by messages of two sources is stored once in the
/// account's directory, converted once, and every row that names it, from
/// either source, points at the one preview.
#[test]
fn a_file_two_sources_share_is_converted_once_for_both() {
    with_real_ffmpeg(async {
        let (opened, _dir, imessage_attachment) = fixture_with_bmp("imessage").await;
        let mut conn = opened.conn().await.unwrap();
        let message_id = seed_message(&mut conn, "sms").await;
        let sms_attachment =
            attach_stored_blob(&opened, &mut conn, message_id, SHA, ".bmp", BMP_1X1).await;

        assert_eq!(
            run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
                .await
                .unwrap(),
            stats(1, 1, 1, 0, 0)
        );

        let preview = derived_of(&mut conn, imessage_attachment).await;
        assert!(preview.is_some());
        assert_eq!(derived_of(&mut conn, sms_attachment).await, preview);
    });
}

/// A `process-assets` that Ctrl-C or SIGTERM stops fails, so a script that
/// runs it never reads a stopped run as a finished one, and converts nothing
/// more (#1729).
#[tokio::test]
async fn a_stopped_run_fails_and_converts_nothing_more() {
    let (opened, _dir, attachment) = fixture_with_bmp("imessage").await;

    let err = run(
        &opened,
        &ProcessAssetsOptions::default(),
        &AtomicBool::new(true),
    )
    .await
    .unwrap_err();

    assert!(err.to_string().starts_with("stopped"), "got: {err}");
    let mut conn = opened.conn().await.unwrap();
    assert_eq!(thumbnail_of(&mut conn, attachment).await, None);
    assert_eq!(derived_of(&mut conn, attachment).await, None);
}

#[tokio::test]
async fn a_database_without_accounts_is_an_error() {
    let (opened, _dir) = open_db().await;

    let err = run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
        .await
        .unwrap_err();

    assert!(
        err.to_string().starts_with("no accounts found"),
        "got: {err}"
    );
}

#[tokio::test]
async fn an_account_without_an_assets_directory_is_passed_over() {
    let (opened, _dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
            .await
            .unwrap(),
        stats(0, 0, 0, 0, 0)
    );
}

#[tokio::test]
async fn a_blob_that_is_not_media_is_left_as_is_by_the_run() {
    let (opened, _dir) = open_db().await;
    let mut conn = opened.conn().await.unwrap();
    seed_account(&mut conn, ACCOUNT).await;
    let message_id = seed_message(&mut conn, "imessage").await;
    let attachment_id =
        attach_stored_blob(&opened, &mut conn, message_id, SHA, ".txt", b"notes").await;

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
            .await
            .unwrap(),
        stats(1, 0, 0, 1, 0)
    );
    assert_eq!(derived_of(&mut conn, attachment_id).await, None);
}

#[tokio::test]
async fn a_missing_original_is_counted_as_a_failure_and_the_run_goes_on() {
    let (opened, _dir, attachment_id) = fixture_with_bmp("imessage").await;
    let mut conn = opened.conn().await.unwrap();
    let original = opened
        .cfg
        .paths
        .assets_dir_for_account(ACCOUNT)
        .join(format!("ab/{SHA}.bmp"));
    fs::remove_file(&original).unwrap();
    let message_id = seed_message(&mut conn, "sms").await;
    attach_stored_blob(
        &opened,
        &mut conn,
        message_id,
        &"b".repeat(64),
        ".txt",
        b"notes",
    )
    .await;

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
            .await
            .unwrap(),
        stats(2, 0, 0, 1, 1)
    );
    assert_eq!(derived_of(&mut conn, attachment_id).await, None);
}

/// A damaged Preview whose original is missing cannot be converted again.
/// The run drops it rather than leave the rows naming it, so the server
/// stops serving it as if whole: every row that names it, from every
/// source, is cleared, the file is deleted, and the attachment still counts
/// as a failure. A dry run says so and changes nothing.
#[tokio::test]
async fn a_damaged_preview_whose_original_is_missing_is_dropped_and_still_a_failure() {
    let (opened, _dir, imessage_attachment) = fixture_with_bmp("imessage").await;
    let mut conn = opened.conn().await.unwrap();
    let message_id = seed_message(&mut conn, "sms").await;
    let sms_attachment =
        attach_stored_blob(&opened, &mut conn, message_id, SHA, ".bmp", BMP_1X1).await;
    let preview_sha = "c".repeat(64);
    let rel = format!("cc/{preview_sha}.jpg");
    let preview = opened
        .cfg
        .paths
        .assets_converted_dir_for_account(ACCOUNT)
        .join(&rel);
    fs::create_dir_all(preview.parent().unwrap()).unwrap();
    fs::write(&preview, b"a Preview cut short").unwrap();
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query(
        "UPDATE attachments
         SET derived_sha256 = $1, derived_assets_path = $2, derived_mime_type = 'image/jpeg'",
    )
    .bind(&preview_sha)
    .bind(&rel)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    fs::remove_file(
        opened
            .cfg
            .paths
            .assets_dir_for_account(ACCOUNT)
            .join(format!("ab/{SHA}.bmp")),
    )
    .unwrap();
    let named = Some((preview_sha, rel, "image/jpeg".to_string()));

    let dry_run = ProcessAssetsOptions {
        dry_run: true,
        ..Default::default()
    };
    assert_eq!(
        run(&opened, &dry_run, &NOT_STOPPED).await.unwrap(),
        stats(1, 0, 0, 0, 1)
    );
    assert!(preview.is_file(), "a dry run deletes nothing");
    assert_eq!(derived_of(&mut conn, imessage_attachment).await, named);

    assert_eq!(
        run(&opened, &ProcessAssetsOptions::default(), &NOT_STOPPED)
            .await
            .unwrap(),
        stats(1, 0, 0, 0, 1)
    );
    assert!(!preview.exists(), "the damaged Preview is deleted");
    assert_eq!(derived_of(&mut conn, imessage_attachment).await, None);
    assert_eq!(derived_of(&mut conn, sms_attachment).await, None);
    let named_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attachments
         WHERE derived_sha256 IS NOT NULL OR derived_assets_path IS NOT NULL
            OR derived_mime_type IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(named_columns, 0, "every derived column is cleared");
}

#[tokio::test]
async fn account_ids_come_from_the_table_or_else_from_the_data_directories() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    for directory in ["7", "12", "notes"] {
        fs::create_dir_all(data.join(directory)).unwrap();
    }
    fs::write(data.join("3"), b"a file, not an account").unwrap();

    // No accounts table and no data directory: nothing.
    let (pool, _db_dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        list_account_ids(&mut conn, &dir.path().join("elsewhere"))
            .await
            .unwrap(),
        Vec::<i64>::new()
    );

    // No accounts table yet: the directories named by an id are the accounts.
    assert_eq!(list_account_ids(&mut conn, &data).await.unwrap(), [7, 12]);

    // A table with rows in it is the answer, and the directories are ignored.
    schema::ensure_schema(&mut conn).await.unwrap();
    seed_account(&mut conn, 5).await;
    assert_eq!(list_account_ids(&mut conn, &data).await.unwrap(), [5]);
}

#[tokio::test]
async fn opening_an_account_without_an_assets_directory_gives_nothing_to_process() {
    let (opened, _dir) = open_db().await;
    let opts = ProcessAssetsOptions::default();
    let work = tempfile::tempdir().unwrap();

    let pass = AccountPass::open(&opened.cfg, &opts, work.path(), ACCOUNT, &NOT_STOPPED).unwrap();

    assert!(pass.is_none());
    assert!(
        !opened
            .cfg
            .paths
            .assets_converted_dir_for_account(ACCOUNT)
            .exists(),
        "no converted directory is made for an account with nothing in it"
    );
}

#[tokio::test]
async fn opening_an_account_makes_its_converted_directory_and_cleans_its_incoming_temps() {
    let (opened, _dir) = open_db().await;
    let opts = ProcessAssetsOptions::default();
    let work = tempfile::tempdir().unwrap();
    let assets = opened.cfg.paths.assets_dir_for_account(ACCOUNT);
    let part = assets.join(".incoming").join(format!("{SHA}-1.part"));
    fs::create_dir_all(part.parent().unwrap()).unwrap();
    fs::write(&part, b"half").unwrap();
    make_abandoned(&part);
    let live_part = assets.join(".incoming").join(format!("{SHA}-2.part"));
    fs::write(&live_part, b"an upload in progress").unwrap();

    let pass = AccountPass::open(&opened.cfg, &opts, work.path(), ACCOUNT, &NOT_STOPPED)
        .unwrap()
        .expect("an account with an assets directory is processed");

    let converted = opened.cfg.paths.assets_converted_dir_for_account(ACCOUNT);
    assert_eq!(pass.assets_dir, assets);
    assert_eq!(pass.converted_dir, converted);
    assert_eq!(pass.account_id, ACCOUNT);
    assert!(converted.is_dir());
    assert!(
        !part.exists(),
        "an abandoned upload temp is removed on open"
    );
    assert!(live_part.exists(), "a live upload's temp is kept on open");
}

/// A run killed between creating a temporary file in a shard directory and
/// renaming it over its fingerprint leaves the temporary file behind. The
/// next run removes it from the originals and the Preview directories alike,
/// and leaves a young one, which a running import or run may still be
/// writing, alone.
#[tokio::test]
async fn opening_an_account_removes_temporary_files_a_killed_run_left_in_the_shards() {
    let (opened, _dir) = open_db().await;
    let opts = ProcessAssetsOptions::default();
    let work = tempfile::tempdir().unwrap();
    let assets = opened.cfg.paths.assets_dir_for_account(ACCOUNT);
    let converted = opened.cfg.paths.assets_converted_dir_for_account(ACCOUNT);
    let left_original = assets.join("ab").join(".tmpA1b2C3");
    let left_preview = converted.join("cd").join(".tmpD4e5F6");
    let live_preview = converted.join("cd").join(".tmpG7h8I9");
    let original = assets.join("ab").join(SHA);
    for path in [&left_original, &left_preview, &live_preview, &original] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"half").unwrap();
    }
    for path in [&left_original, &left_preview, &original] {
        make_abandoned(path);
    }

    AccountPass::open(&opened.cfg, &opts, work.path(), ACCOUNT, &NOT_STOPPED)
        .unwrap()
        .expect("an account with an assets directory is processed");

    assert!(
        !left_original.exists(),
        "an originals shard temp is removed"
    );
    assert!(!left_preview.exists(), "a Preview shard temp is removed");
    assert!(live_preview.exists(), "a temp still being written is kept");
    assert!(original.exists(), "a stored original is never a temp");
}

/// A dry run says which temporary files it would remove and removes none.
#[tokio::test]
async fn a_dry_run_leaves_temporary_files_in_the_shards() {
    let (opened, _dir) = open_db().await;
    let opts = ProcessAssetsOptions {
        dry_run: true,
        ..Default::default()
    };
    let work = tempfile::tempdir().unwrap();
    let left = opened
        .cfg
        .paths
        .assets_dir_for_account(ACCOUNT)
        .join("ab")
        .join(".tmpA1b2C3");
    fs::create_dir_all(left.parent().unwrap()).unwrap();
    fs::write(&left, b"half").unwrap();
    make_abandoned(&left);

    AccountPass::open(&opened.cfg, &opts, work.path(), ACCOUNT, &NOT_STOPPED)
        .unwrap()
        .expect("an account with an assets directory is processed");

    assert!(left.exists());
}

/// A preview left cut short by an interrupted run is written again when the
/// same bytes are stored, which is how a run repairs it.
#[test]
fn a_truncated_derived_file_is_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let buf = vec![7u8; 4096];
    let rel = derived_rel_path(&crate::assets_api::Sha256::of_bytes(&buf), ".jpg");
    let dest = dir.path().join(&rel);
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, &buf[..100]).unwrap();
    store_derived_bytes(dir.path(), &buf, ".jpg").unwrap();
    let stored = std::fs::read(&dest).unwrap();
    assert_eq!(stored.len(), buf.len(), "the cut-short file is replaced");
    assert!(stored == buf, "the stored bytes are the preview's bytes");
}

/// Storing a preview leaves no temporary file beside it.
#[test]
fn storing_a_derived_file_leaves_only_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let buf = vec![9u8; 512];
    let blob = store_derived_bytes(dir.path(), &buf, ".jpg").unwrap();
    let dest = dir.path().join(&blob.assets_path);
    let names: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![dest.file_name().unwrap().to_owned()]);
    assert_eq!(std::fs::read(&dest).unwrap(), buf);
}

/// The Trash is emptied while the pass makes a Thumbnail: the rows that
/// named the original are gone before the Thumbnail is recorded. The pass
/// counts nothing made and names nothing, and leaves the file to the sweep
/// at the next Import Run's end, because the same bytes may be the version
/// of another original a concurrent pass is about to record.
#[test]
fn a_version_made_after_its_rows_were_deleted_is_left_for_the_sweep() {
    with_real_ffmpeg(async {
        let (opened, _dir, _) = fixture_with("imessage", ".png", PNG_1X1_RGB).await;
        let rows = versions_db::stored_originals(&mut opened.conn().await.unwrap(), ACCOUNT, None)
            .await
            .unwrap();
        let mut conn = opened.conn().await.unwrap();
        let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
        sqlx::query("DELETE FROM attachments")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        drop(conn);
        let opts = ProcessAssetsOptions::default();
        let work = tempfile::tempdir().unwrap();
        let pass = AccountPass::new(
            &opened.cfg,
            &opts,
            work.path(),
            ACCOUNT,
            &NOT_STOPPED,
            Log::Print,
        )
        .unwrap()
        .unwrap();

        let made = pass.process_rows(&opened.db, &rows).await;

        assert_eq!(made, stats(1, 0, 0, 1, 0));
        let converted = opened.cfg.paths.assets_converted_dir_for_account(ACCOUNT);
        assert_eq!(
            walk(&converted).len(),
            1,
            "the Thumbnail waits for the sweep"
        );
    });
}

/// A deleted account's directory is gone, so the pass stores nothing for it
/// and never makes the directory again.
#[test]
fn nothing_is_stored_once_the_account_directory_is_gone() {
    with_real_ffmpeg(async {
        let (opened, _dir, _) = fixture_with("imessage", ".png", PNG_1X1_RGB).await;
        let rows = versions_db::stored_originals(&mut opened.conn().await.unwrap(), ACCOUNT, None)
            .await
            .unwrap();
        let opts = ProcessAssetsOptions::default();
        let work = tempfile::tempdir().unwrap();
        let pass = AccountPass::new(
            &opened.cfg,
            &opts,
            work.path(),
            ACCOUNT,
            &NOT_STOPPED,
            Log::Print,
        )
        .unwrap()
        .unwrap();
        let account_dir = opened.cfg.paths.data_dir.join(ACCOUNT.to_string());
        let source = pass.assets_dir.join(&rows[0].assets_path);
        let kept = work.path().join("original.png");
        fs::copy(&source, &kept).unwrap();
        fs::remove_dir_all(&account_dir).unwrap();

        let made = pass
            .derive(Version::Thumbnail, Kind::Image, &kept, &rows[0])
            .map(|_| ());

        assert!(made.is_err(), "nothing is stored for a deleted account");
        assert!(
            !account_dir.exists(),
            "the account directory is not made again"
        );
    });
}

/// Every file under `dir`, at any depth.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// A pass stopped part-way leaves its work directory, with part-made copies
/// of attachments in it. The next pass removes one older than a day, and
/// leaves a younger one, which a pass running now may be using.
#[test]
fn a_work_directory_a_stopped_pass_left_is_removed_by_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".media-work");
    let (stale, live) = (root.join("pass-old"), root.join("pass-new"));
    for path in [&stale, &live] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("Thumbnail-abc.jpg"), b"half").unwrap();
    }
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86_400);
    fs::File::open(&stale)
        .unwrap()
        .set_modified(two_days_ago)
        .unwrap();

    let work = work_dir(dir.path()).unwrap();

    assert!(!stale.exists(), "the stopped pass's directory is removed");
    assert!(live.exists(), "a young directory is left alone");
    assert!(work.path().starts_with(&root));
}

/// A pass that works for more than a day on originals that need nothing
/// writes no file, so its work directory would look stopped. It touches the
/// directory before each original, so another pass leaves it alone.
#[tokio::test]
async fn a_live_pass_keeps_its_work_directory_young() {
    let (opened, dir, _) = fixture_with("imessage", ".txt", b"notes").await;
    let rows = versions_db::stored_originals(&mut opened.conn().await.unwrap(), ACCOUNT, None)
        .await
        .unwrap();
    let work = work_dir(dir.path()).unwrap();
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86_400);
    fs::File::open(work.path())
        .unwrap()
        .set_modified(two_days_ago)
        .unwrap();
    let opts = ProcessAssetsOptions::default();
    let pass = AccountPass::new(
        &opened.cfg,
        &opts,
        work.path(),
        ACCOUNT,
        &NOT_STOPPED,
        Log::Print,
    )
    .unwrap()
    .unwrap();

    pass.process_rows(&opened.db, &rows).await;

    let _other = work_dir(dir.path()).unwrap();
    assert!(
        work.path().is_dir(),
        "another pass leaves a live directory alone"
    );
}

/// The line that ends `process-assets` words each count singular for one
/// and plural for every other count (#1825).
#[test]
fn the_done_line_counts_one_and_many() {
    let one = ProcessAssetsStats {
        scanned: 1,
        derived: 1,
        thumbnails: 1,
        skipped: 1,
        errors: 1,
    };
    assert_eq!(
        done_line(&one, false),
        "done: read 1 original, made 1 Preview and 1 Thumbnail, left 1 original as it was, \
         1 original whose Preview or Thumbnail could not be made"
    );
    let many = ProcessAssetsStats {
        scanned: 4,
        derived: 2,
        thumbnails: 3,
        skipped: 0,
        errors: 0,
    };
    assert_eq!(
        done_line(&many, true),
        "done: read 4 originals, made 2 Previews and 3 Thumbnails, left 0 originals as they were, \
         0 originals whose Preview or Thumbnail could not be made (dry run)"
    );
}
