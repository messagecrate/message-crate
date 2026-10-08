use super::*;

use std::ffi::OsString;

use httpmock::prelude::*;

/// The SHA-256 of `bytes`, in lowercase hex.
impl Retry {
    /// The check's thread, when one started.
    fn started(self) -> Option<JoinHandle<()>> {
        match self {
            Self::Started(handle) => Some(handle),
            Self::AlreadyRunning | Self::CouldNotStart => None,
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// `bytes` gzipped, as `ffmpeg-static` publishes its files.
fn gzipped(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// `program` pinned to release `release`, whose asset is `published` with
/// the checksum `sha256`.
fn pin_with(
    program: Program,
    release: &'static str,
    published: &[u8],
    gzip: bool,
    sha256: &str,
) -> (Pinned, Vec<u8>) {
    let pinned = Pinned {
        program,
        repo: "owner/project",
        release,
        asset: leak(format!(
            "{}-test{}",
            program.name(),
            if gzip { ".gz" } else { "" }
        )),
        sha256: leak(sha256.to_string()),
        program_sha256: leak(if gzip {
            let mut program = Vec::new();
            flate2::read::GzDecoder::new(published)
                .read_to_end(&mut program)
                .unwrap();
            sha256_hex(&program)
        } else {
            sha256.to_string()
        }),
        gzip,
    };
    (pinned, published.to_vec())
}

/// `program` pinned to the asset `published`, with its true checksum.
fn pin(program: Program, release: &'static str, published: &[u8], gzip: bool) -> (Pinned, Vec<u8>) {
    pin_with(program, release, published, gzip, &sha256_hex(published))
}

/// Serve `body` at `pinned`'s path on `server`.
fn serve<'a>(server: &'a MockServer, pinned: &Pinned, body: &[u8]) -> httpmock::Mock<'a> {
    let path = format!(
        "/{}/releases/download/{}/{}",
        pinned.repo, pinned.release, pinned.asset
    );
    let body = body.to_vec();
    server.mock(move |when, then| {
        when.method(GET).path(path);
        then.status(200).body(body);
    })
}

/// The lookup pointed at an empty `PATH` and the Tools Directory `dir`, so
/// ffmpeg on this machine's `PATH` changes nothing.
fn no_ffmpeg_on_path(dir: &Path) -> media::testutil::ToolsHidden {
    media::testutil::locate_tools(OsString::new(), dir.to_path_buf())
}

/// [`download_missing`] as one check of `downloads`, on this thread.
fn check_now(dir: &Path, base: &str, pinned: &[Pinned], downloads: &ToolDownloads) {
    download_missing(dir, base, pinned, downloads.begin_check());
}

/// The names in `dir` other than `keep` and the lock file, to see that no
/// temporary file was left behind.
fn other_files(dir: &Path, keep: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != LOCK_FILE && !keep.contains(&name.as_str()))
        .collect();
    names.sort();
    names
}

/// The file that arrives is refused when its checksum is not the pinned one:
/// it is deleted, nothing is put in place, and the failure says the
/// checksum did not match.
#[test]
fn a_download_whose_checksum_does_not_match_is_refused() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let server = MockServer::start();
    let (pinned, _) = pin_with(
        Program::Wtsexporter,
        "r1",
        b"",
        false,
        &sha256_hex(b"the pinned file"),
    );
    serve(&server, &pinned, b"a file changed on the way");
    let downloads = ToolDownloads::default();

    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &downloads,
    );

    assert!(!tools.path().join(Program::Wtsexporter.file_name()).exists());
    assert_eq!(other_files(tools.path(), &[]), Vec::<String>::new());
    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Wtsexporter) else {
        panic!(
            "the download did not fail: {:?}",
            downloads.get(Program::Wtsexporter)
        );
    };
    assert!(
        reason.contains("checksum did not match"),
        "the reason does not name the checksum: {reason}"
    );
    assert!(reason.contains(&sha256_hex(b"a file changed on the way")));
}

/// A program the app wrote from an older release stays in use while the
/// newer one fails to download or does not match, and is replaced only by
/// a download that passed.
#[test]
fn an_older_program_is_replaced_only_after_a_good_download() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let target = tools.path().join(Program::Wtsexporter.file_name());
    std::fs::write(&target, b"old release").unwrap();
    let (old, _) = pin(Program::Wtsexporter, "r1", b"old release", false);
    make_executable(&target).unwrap();
    let mut manifest = Manifest::new();
    manifest.insert(
        Program::Wtsexporter,
        Written::of(&old, Stamp::at(&target).unwrap()),
    );
    write_manifest(tools.path(), &manifest).unwrap();
    let (new, new_bytes) = pin(Program::Wtsexporter, "r2", b"new release", false);
    let downloads = ToolDownloads::default();

    // 404 Not Found: the old file stays.
    let server = MockServer::start();
    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&new),
        &downloads,
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"old release");
    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Wtsexporter) else {
        panic!("the download did not fail");
    };
    assert!(reason.contains("404 Not Found"), "{reason}");

    // A file that does not match: the old file stays.
    let mut wrong = serve(&server, &new, b"not the new release");
    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&new),
        &downloads,
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"old release");
    assert_eq!(read_manifest(tools.path()), manifest);
    wrong.delete();

    // A good download replaces it, and the record names the new release.
    serve(&server, &new, &new_bytes);
    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&new),
        &downloads,
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"new release");
    assert_eq!(downloads.get(Program::Wtsexporter), None);
    assert_eq!(
        read_manifest(tools.path()).get(&Program::Wtsexporter),
        Some(&Written::of(&new, Stamp::at(&target).unwrap()))
    );
    assert_eq!(
        other_files(
            tools.path(),
            &[MANIFEST_FILE, &Program::Wtsexporter.file_name()]
        ),
        Vec::<String>::new()
    );
}

/// A gzipped asset is checked as it arrives, then unpacked into the program,
/// which can run.
#[test]
fn a_gzipped_program_is_unpacked_and_made_executable() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let program = b"#!/bin/sh\nexit 0\n";
    let (pinned, published) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(program), true);
    let server = MockServer::start();
    serve(&server, &pinned, &published);

    check_now(
        tools.path(),
        &server.base_url(),
        &[pinned],
        &ToolDownloads::default(),
    );

    let path = tools.path().join(Program::Ffmpeg.file_name());
    assert_eq!(std::fs::read(&path).unwrap(), program);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "not executable: {mode:o}");
    }
}

/// A gzipped asset that passes its own checksum is still refused when the
/// program it unpacks to is not the pinned program.
#[test]
fn a_gzipped_program_that_is_not_the_pinned_one_is_refused() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (mut pinned, published) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(b"ffmpeg"), true);
    pinned.program_sha256 = leak(sha256_hex(b"the pinned program"));
    let server = MockServer::start();
    serve(&server, &pinned, &published);
    let downloads = ToolDownloads::default();

    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &downloads,
    );

    assert!(!tools.path().join(Program::Ffmpeg.file_name()).exists());
    assert_eq!(other_files(tools.path(), &[]), Vec::<String>::new());
    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Ffmpeg) else {
        panic!(
            "the download did not fail: {:?}",
            downloads.get(Program::Ffmpeg)
        );
    };
    assert!(reason.contains(&sha256_hex(b"ffmpeg")), "{reason}");
}

/// Write a program at `path` that answers `-version`.
#[cfg(unix)]
fn write_runnable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// ffmpeg and ffprobe on `PATH` are used as they are, and neither is
/// downloaded; wtsexporter still is.
#[cfg(unix)]
#[test]
fn ffmpeg_on_path_is_not_downloaded() {
    let bin = tempfile::tempdir().unwrap();
    write_runnable(&bin.path().join("ffmpeg"));
    write_runnable(&bin.path().join("ffprobe"));
    let tools = tempfile::tempdir().unwrap();
    let _tools = media::testutil::locate_tools(
        bin.path().as_os_str().to_os_string(),
        tools.path().to_path_buf(),
    );
    let server = MockServer::start();
    let (ffmpeg, ffmpeg_gz) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(b"ffmpeg"), true);
    let (ffprobe, ffprobe_gz) = pin(Program::Ffprobe, "b6.1.1", &gzipped(b"ffprobe"), true);
    let (wts, wts_bytes) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let ffmpeg_asked = serve(&server, &ffmpeg, &ffmpeg_gz);
    let ffprobe_asked = serve(&server, &ffprobe, &ffprobe_gz);
    let wts_asked = serve(&server, &wts, &wts_bytes);

    check_now(
        tools.path(),
        &server.base_url(),
        &[ffmpeg, ffprobe, wts],
        &ToolDownloads::default(),
    );

    ffmpeg_asked.assert_calls(0);
    ffprobe_asked.assert_calls(0);
    wts_asked.assert_calls(1);
    assert!(!tools.path().join("ffmpeg").exists());
    assert!(!tools.path().join("ffprobe").exists());
}

/// ffmpeg on `PATH` without ffprobe is not enough: both are downloaded.
#[cfg(unix)]
#[test]
fn ffmpeg_on_path_without_ffprobe_is_downloaded() {
    let bin = tempfile::tempdir().unwrap();
    write_runnable(&bin.path().join("ffmpeg"));
    let tools = tempfile::tempdir().unwrap();
    let _tools = media::testutil::locate_tools(
        bin.path().as_os_str().to_os_string(),
        tools.path().to_path_buf(),
    );
    let server = MockServer::start();
    let (ffmpeg, ffmpeg_gz) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(b"ffmpeg"), true);
    let (ffprobe, ffprobe_gz) = pin(Program::Ffprobe, "b6.1.1", &gzipped(b"ffprobe"), true);
    serve(&server, &ffmpeg, &ffmpeg_gz);
    serve(&server, &ffprobe, &ffprobe_gz);

    check_now(
        tools.path(),
        &server.base_url(),
        &[ffmpeg, ffprobe],
        &ToolDownloads::default(),
    );

    assert_eq!(
        std::fs::read(tools.path().join("ffmpeg")).unwrap(),
        b"ffmpeg"
    );
    assert_eq!(
        std::fs::read(tools.path().join("ffprobe")).unwrap(),
        b"ffprobe"
    );
}

/// A program the app wrote from the pinned release is not downloaded again,
/// and its file is not read to know it.
#[test]
fn the_pinned_program_the_app_wrote_is_kept() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"pinned", false);
    let target = tools.path().join(Program::Wtsexporter.file_name());
    std::fs::write(&target, b"pinned").unwrap();
    make_executable(&target).unwrap();
    let mut manifest = Manifest::new();
    manifest.insert(
        Program::Wtsexporter,
        Written::of(&pinned, Stamp::at(&target).unwrap()),
    );
    write_manifest(tools.path(), &manifest).unwrap();
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);

    check_now(
        tools.path(),
        &server.base_url(),
        &[pinned],
        &ToolDownloads::default(),
    );

    asked.assert_calls(0);
}

/// A file put over the one the app wrote, the same size but not the same
/// file, is not the app's: its stamp differs, and it is replaced.
#[test]
fn a_file_put_over_the_apps_own_is_replaced() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"pinned", false);
    let target = tools.path().join(Program::Wtsexporter.file_name());
    std::fs::write(&target, b"pinned").unwrap();
    make_executable(&target).unwrap();
    let mut manifest = Manifest::new();
    manifest.insert(
        Program::Wtsexporter,
        Written::of(&pinned, Stamp::at(&target).unwrap()),
    );
    write_manifest(tools.path(), &manifest).unwrap();
    // Six other bytes, modified at another time.
    std::fs::write(&target, b"theirs").unwrap();
    File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_000_000))
        .unwrap();
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);

    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &ToolDownloads::default(),
    );

    asked.assert_calls(1);
    assert_eq!(std::fs::read(&target).unwrap(), b"pinned");
}

/// ffmpeg and ffprobe put in the Tools Directory by hand are replaced by a
/// good download, even when they run, because the Tools Directory belongs
/// to the app. With no download to replace them they stay in use.
#[cfg(unix)]
#[test]
fn ffmpeg_put_there_by_hand_is_replaced_after_a_good_download() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let theirs = b"#!/bin/sh\n# theirs\nexit 0\n";
    let ours = b"#!/bin/sh\n# pinned\nexit 0\n";
    for name in ["ffmpeg", "ffprobe"] {
        let path = tools.path().join(name);
        std::fs::write(&path, theirs).unwrap();
        make_executable(&path).unwrap();
    }
    let (ffmpeg, ffmpeg_gz) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(ours), true);
    let (ffprobe, ffprobe_gz) = pin(Program::Ffprobe, "b6.1.1", &gzipped(ours), true);
    let downloads = ToolDownloads::default();

    // No network: what is there stays.
    check_now(
        tools.path(),
        "http://127.0.0.1:9",
        &[ffmpeg.clone(), ffprobe.clone()],
        &downloads,
    );
    for name in ["ffmpeg", "ffprobe"] {
        assert_eq!(std::fs::read(tools.path().join(name)).unwrap(), theirs);
    }

    let server = MockServer::start();
    serve(&server, &ffmpeg, &ffmpeg_gz);
    serve(&server, &ffprobe, &ffprobe_gz);
    check_now(
        tools.path(),
        &server.base_url(),
        &[ffmpeg, ffprobe],
        &downloads,
    );
    for (name, program) in [("ffmpeg", Program::Ffmpeg), ("ffprobe", Program::Ffprobe)] {
        assert_eq!(std::fs::read(tools.path().join(name)).unwrap(), ours);
        assert_eq!(downloads.get(program), None);
    }
}

/// A pinned file the app recorded that does not run is not downloaded
/// again, start after start, because the download would be the same file;
/// the failure says it doesn't run.
#[cfg(unix)]
#[test]
fn a_pinned_program_that_does_not_run_is_not_downloaded_again() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let not_a_program = b"not a program";
    let (pinned, published) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(not_a_program), true);
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);

    for _start in 0..2 {
        let downloads = ToolDownloads::default();
        check_now(
            tools.path(),
            &server.base_url(),
            std::slice::from_ref(&pinned),
            &downloads,
        );
        let Some(DownloadState::Failed { reason }) = downloads.get(Program::Ffmpeg) else {
            panic!(
                "the program was not said not to run: {:?}",
                downloads.get(Program::Ffmpeg)
            );
        };
        assert_eq!(
            reason,
            "ffmpeg in the Tools Directory doesn't run on this computer. \
             Install it with your package manager instead; the app uses the copy on PATH."
        );
    }

    asked.assert_calls(1);
}

/// The pinned program put there by hand that does not run is kept and
/// recorded, and not downloaded, because the download would be the same file.
#[cfg(unix)]
#[test]
fn a_pinned_program_put_there_by_hand_that_does_not_run_is_not_downloaded() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let not_a_program = b"not a program";
    let (pinned, published) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(not_a_program), true);
    std::fs::write(tools.path().join("ffmpeg"), not_a_program).unwrap();
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);
    let downloads = ToolDownloads::default();

    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &downloads,
    );

    asked.assert_calls(0);
    assert!(read_manifest(tools.path()).contains_key(&Program::Ffmpeg));
    assert!(matches!(
        downloads.get(Program::Ffmpeg),
        Some(DownloadState::Failed { .. })
    ));
}

/// ffmpeg is kept and recorded again when its record is lost, because the
/// file is the pinned program, though the pin is of the `.gz`.
#[cfg(unix)]
#[test]
fn ffmpeg_with_no_record_is_adopted_when_it_is_the_pinned_program() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let program = b"#!/bin/sh\n# pinned\nexit 0\n";
    let (pinned, published) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(program), true);
    let target = tools.path().join(Program::Ffmpeg.file_name());
    std::fs::write(&target, program).unwrap();
    make_executable(&target).unwrap();
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);
    let downloads = ToolDownloads::default();

    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &downloads,
    );

    asked.assert_calls(0);
    assert_eq!(downloads.get(Program::Ffmpeg), None);
    assert_eq!(
        read_manifest(tools.path()).get(&Program::Ffmpeg),
        Some(&Written::of(&pinned, Stamp::at(&target).unwrap()))
    );
}

/// The app's own file whose stamp stopped matching, as after a sync or a
/// remount, is recorded again with its new stamp and not downloaded, because
/// its content is still the pinned program.
#[cfg(unix)]
#[test]
fn a_changed_stamp_on_the_pinned_program_is_recorded_again() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let program = b"#!/bin/sh\n# pinned\nexit 0\n";
    let (pinned, published) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(program), true);
    let target = tools.path().join(Program::Ffmpeg.file_name());
    std::fs::write(&target, program).unwrap();
    make_executable(&target).unwrap();
    let mut manifest = Manifest::new();
    manifest.insert(
        Program::Ffmpeg,
        Written::of(&pinned, Stamp::at(&target).unwrap()),
    );
    write_manifest(tools.path(), &manifest).unwrap();
    File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_000_000))
        .unwrap();
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);

    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &ToolDownloads::default(),
    );

    asked.assert_calls(0);
    let recorded = read_manifest(tools.path());
    assert_eq!(
        recorded.get(&Program::Ffmpeg),
        Some(&Written::of(&pinned, Stamp::at(&target).unwrap()))
    );
    assert_ne!(recorded, manifest);
}

/// A program put there by hand is kept and recorded when it is the pinned
/// file, and replaced when it is anything else, because its release can't
/// be told from the file.
#[test]
fn a_program_put_there_by_hand_is_kept_only_when_it_is_the_pinned_file() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let target = tools.path().join(Program::Wtsexporter.file_name());
    let (pinned, published) = pin(Program::Wtsexporter, "r2", b"pinned", false);
    let server = MockServer::start();
    let mut asked = serve(&server, &pinned, &published);

    std::fs::write(&target, b"pinned").unwrap();
    check_now(
        tools.path(),
        &server.base_url(),
        std::slice::from_ref(&pinned),
        &ToolDownloads::default(),
    );
    asked.assert_calls(0);
    assert_eq!(
        read_manifest(tools.path()).get(&Program::Wtsexporter),
        Some(&Written::of(&pinned, Stamp::at(&target).unwrap()))
    );
    asked.delete();

    std::fs::write(&target, b"a pipx shim, or an older release").unwrap();
    let asked = serve(&server, &pinned, &published);
    check_now(
        tools.path(),
        &server.base_url(),
        &[pinned],
        &ToolDownloads::default(),
    );
    asked.assert_calls(1);
    assert_eq!(std::fs::read(&target).unwrap(), b"pinned");
}

/// The temporary files an interrupted check left are deleted at the next
/// start, and nothing else in the Tools Directory is.
#[test]
fn an_interrupted_downloads_leftovers_are_deleted() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    for name in [
        ".ffmpeg.download-a1b2c3",
        ".ffprobe.unpack-d4e5f6",
        ".wtsexporter.download-g7h8i9",
        ".manifest-j0k1l2",
        "notes.txt",
    ] {
        std::fs::write(tools.path().join(name), b"left").unwrap();
    }

    check_now(
        tools.path(),
        "http://127.0.0.1:9",
        &[],
        &ToolDownloads::default(),
    );

    assert_eq!(
        other_files(tools.path(), &[]),
        vec!["notes.txt".to_string()]
    );
}

/// Every program to be downloaded shows as downloading before its first
/// request, so Settings keeps asking through a slow connect and the gap
/// between two downloads.
#[test]
fn every_program_wanted_shows_as_downloading_before_its_request() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (ffmpeg, ffmpeg_gz) = pin(Program::Ffmpeg, "b6.1.1", &gzipped(b"ffmpeg"), true);
    let (wts, wts_bytes) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let server = MockServer::start();
    let ffmpeg_path = format!(
        "/{}/releases/download/{}/{}",
        ffmpeg.repo, ffmpeg.release, ffmpeg.asset
    );
    server.mock(|when, then| {
        when.method(GET).path(ffmpeg_path);
        then.status(200)
            .delay(Duration::from_millis(500))
            .body(ffmpeg_gz);
    });
    serve(&server, &wts, &wts_bytes);
    let downloads = ToolDownloads::default();
    let base = server.base_url();
    let dir = tools.path().to_path_buf();

    let mut seen_waiting = false;
    std::thread::scope(|scope| {
        let check = scope.spawn(|| check_now(&dir, &base, &[ffmpeg, wts], &downloads));
        while !check.is_finished() {
            if downloads.get(Program::Wtsexporter)
                == Some(DownloadState::Downloading {
                    received: 0,
                    total: None,
                })
            {
                seen_waiting = true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    assert!(
        seen_waiting,
        "wtsexporter was not downloading while it waited"
    );
    assert_eq!(downloads.get(Program::Wtsexporter), None);
}

/// A second app's check, started while the first one runs, does nothing:
/// it neither downloads nor deletes the first one's temporary files.
#[test]
fn a_second_check_at_the_same_time_does_nothing() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let server = MockServer::start();
    let path = format!(
        "/{}/releases/download/{}/{}",
        pinned.repo, pinned.release, pinned.asset
    );
    let asked = server.mock(|when, then| {
        when.method(GET).path(path);
        then.status(200)
            .delay(Duration::from_millis(500))
            .body(published);
    });
    let first = ToolDownloads::default();
    let second = ToolDownloads::default();
    let base = server.base_url();
    let dir = tools.path().to_path_buf();

    std::thread::scope(|scope| {
        let check = scope.spawn(|| check_now(&dir, &base, std::slice::from_ref(&pinned), &first));
        // The first check holds the lock once it shows a download.
        while first.get(Program::Wtsexporter).is_none() {
            assert!(!check.is_finished(), "the first check ended at once");
            std::thread::sleep(Duration::from_millis(1));
        }
        let other = scope.spawn(|| check_now(&dir, &base, std::slice::from_ref(&pinned), &second));
        other.join().unwrap();
        assert!(
            !check.is_finished(),
            "the first check ended before the second"
        );
    });

    asked.assert_calls(1);
    assert_eq!(second.get(Program::Wtsexporter), None);
    assert_eq!(first.get(Program::Wtsexporter), None);
    assert_eq!(
        std::fs::read(tools.path().join(Program::Wtsexporter.file_name())).unwrap(),
        b"wtsexporter"
    );
}

/// With no network the check fails quietly, saying so, and leaves nothing.
#[test]
fn no_network_is_a_failed_download_that_says_so() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, _) = pin(Program::Wtsexporter, "r1", b"pinned", false);
    let downloads = ToolDownloads::default();

    // Port 9 on this computer, where nothing listens.
    check_now(tools.path(), "http://127.0.0.1:9", &[pinned], &downloads);

    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Wtsexporter) else {
        panic!("the download did not fail");
    };
    assert!(reason.starts_with("No connection"), "{reason}");
    assert_eq!(other_files(tools.path(), &[]), Vec::<String>::new());
}

/// Each supported platform pins ffmpeg, ffprobe and, except Linux on ARM,
/// wtsexporter, each with a whole SHA-256 and a GitHub release address.
#[test]
fn every_platform_is_pinned() {
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
        ("windows", "x86_64"),
        ("windows", "aarch64"),
    ] {
        let pinned = pinned_for(os, arch);
        let programs: Vec<Program> = pinned.iter().map(|pin| pin.program).collect();
        let expected = if (os, arch) == ("linux", "aarch64") {
            vec![Program::Ffmpeg, Program::Ffprobe]
        } else {
            vec![Program::Ffmpeg, Program::Ffprobe, Program::Wtsexporter]
        };
        assert_eq!(programs, expected, "{os} {arch}");
        for pin in &pinned {
            assert!(
                pin.sha256.len() == 64 && pin.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{}",
                pin.asset
            );
            assert_eq!(
                pin.url(GITHUB),
                format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    pin.repo, pin.release, pin.asset
                )
            );
        }
    }
}

/// An import started while its program downloads waits, hears the
/// download's progress, and goes on once the program has arrived.
#[test]
fn an_import_waits_for_a_download_and_goes_on_when_it_arrives() {
    let downloads = ToolDownloads::default();
    downloads.set(
        Program::Wtsexporter,
        DownloadState::Downloading {
            received: 0,
            total: None,
        },
    );
    let mut heard = Vec::new();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
            downloads.set(
                Program::Wtsexporter,
                DownloadState::Downloading {
                    received: 12_000_000,
                    total: Some(30_000_000),
                },
            );
            std::thread::sleep(Duration::from_millis(50));
            downloads.clear(Program::Wtsexporter);
        });
        let waited = downloads.wait_for(
            &[Program::Wtsexporter],
            &|| false,
            &mut |program, received, total| {
                heard.push((program, received, total));
            },
        );
        assert_eq!(waited, Ok(()));
    });

    assert_eq!(heard.first(), Some(&(Program::Wtsexporter, 0, None)));
    assert!(
        heard.contains(&(Program::Wtsexporter, 12_000_000, Some(30_000_000))),
        "the wait did not hear the progress: {heard:?}"
    );
}

/// A download that fails while an import waits for it fails the import,
/// with the download's reason.
#[test]
fn an_import_waiting_for_a_download_that_fails_fails_with_its_reason() {
    let downloads = ToolDownloads::default();
    downloads.set(
        Program::Ffprobe,
        DownloadState::Downloading {
            received: 10,
            total: Some(100),
        },
    );

    let waited = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
            downloads.set(
                Program::Ffprobe,
                DownloadState::Failed {
                    reason: "The download's server answered 404 Not Found.".into(),
                },
            );
        });
        downloads.wait_for(
            &[Program::Ffmpeg, Program::Ffprobe],
            &|| false,
            &mut |_, _, _| {},
        )
    });

    let err = waited.unwrap_err();
    assert_eq!(
        err,
        WaitError::Failed {
            program: Program::Ffprobe,
            reason: "The download's server answered 404 Not Found.".into(),
        }
    );
    // The run's error sends the person to the troubleshooting section.
    assert_eq!(
        err.to_string(),
        "The ffprobe download failed. The download's server answered 404 Not Found. \
         Try again on the Import form downloads it again. \
         See \"ffmpeg or ffprobe not found\" in Troubleshooting at messagecrate.app."
    );
}

/// A program not downloading is not waited for, and a failure from before
/// the import started is the lookup's to report, not the wait's.
#[test]
fn an_import_does_not_wait_for_a_program_not_downloading() {
    let downloads = ToolDownloads::default();
    downloads.set(
        Program::Wtsexporter,
        DownloadState::Failed {
            reason: "no network".into(),
        },
    );
    let mut heard = 0;

    let waited = downloads.wait_for(&[Program::Wtsexporter], &|| false, &mut |_, _, _| {
        heard += 1
    });

    assert_eq!(waited, Ok(()));
    assert_eq!(heard, 0);
}

/// A Cancel ends the wait, though the download goes on.
#[test]
fn a_cancel_ends_the_wait() {
    let downloads = ToolDownloads::default();
    downloads.set(
        Program::Wtsexporter,
        DownloadState::Downloading {
            received: 0,
            total: None,
        },
    );
    let cancel = std::sync::atomic::AtomicBool::new(false);

    let waited = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        downloads.wait_for(
            &[Program::Wtsexporter],
            &|| cancel.load(std::sync::atomic::Ordering::Relaxed),
            &mut |_, _, _| {},
        )
    });

    assert_eq!(waited, Err(WaitError::Cancelled));
}

/// Try again runs the check once on a thread of its own and returns before
/// the download ends, with the program already shown as downloading so the
/// window keeps asking.
#[test]
fn try_again_runs_the_check_once_and_returns_at_once() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let server = MockServer::start();
    let path = format!(
        "/{}/releases/download/{}/{}",
        pinned.repo, pinned.release, pinned.asset
    );
    let asked = server.mock(|when, then| {
        when.method(GET).path(path);
        then.status(200)
            .delay(Duration::from_millis(500))
            .body(published);
    });
    let downloads = ToolDownloads::default();
    downloads.set(
        Program::Wtsexporter,
        DownloadState::Failed {
            reason: "no network".into(),
        },
    );

    let started = std::time::Instant::now();
    let check = retry(
        tools.path().to_path_buf(),
        server.base_url(),
        vec![pinned],
        &downloads,
        &[Program::Wtsexporter],
    )
    .started()
    .expect("a check started");
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "Try again waited for the download"
    );
    assert!(matches!(
        downloads.get(Program::Wtsexporter),
        Some(DownloadState::Downloading { .. })
    ));
    check.join().unwrap();

    asked.assert_calls(1);
    assert_eq!(downloads.get(Program::Wtsexporter), None);
    assert_eq!(
        std::fs::read(tools.path().join(Program::Wtsexporter.file_name())).unwrap(),
        b"wtsexporter"
    );
}

/// Try again while a check holds the lock starts nothing: no second
/// download, and the state the running check keeps is left alone.
#[test]
fn try_again_while_a_check_runs_starts_nothing() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let server = MockServer::start();
    let asked = serve(&server, &pinned, &published);
    let downloads = ToolDownloads::default();
    let running = DownloadState::Downloading {
        received: 5,
        total: Some(10),
    };
    downloads.set(Program::Wtsexporter, running.clone());
    let _held = lock_check(tools.path()).unwrap().expect("the lock is free");

    let check = retry(
        tools.path().to_path_buf(),
        server.base_url(),
        vec![pinned],
        &downloads,
        &[Program::Wtsexporter],
    );

    assert!(
        matches!(check, Retry::AlreadyRunning),
        "a second check started"
    );
    asked.assert_calls(0);
    assert_eq!(downloads.get(Program::Wtsexporter), Some(running));
}

/// A program the check finds in place loses the failure an earlier check
/// left, so Try again after a file was put there by hand clears it.
#[test]
fn a_program_found_in_place_clears_an_earlier_failure() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    std::fs::write(
        tools.path().join(Program::Wtsexporter.file_name()),
        &published,
    )
    .unwrap();
    let downloads = ToolDownloads::default();
    downloads.set(
        Program::Wtsexporter,
        DownloadState::Failed {
            reason: "no network".into(),
        },
    );

    // No server: the file there is the pinned one, so nothing is asked for.
    check_now(
        tools.path(),
        "http://127.0.0.1:9",
        std::slice::from_ref(&pinned),
        &downloads,
    );

    assert_eq!(downloads.get(Program::Wtsexporter), None);
}

/// A check counts as running from Try again until it ends, so the window,
/// which shows a program the check has not looked at yet as missing, keeps
/// asking for the status meanwhile.
#[test]
fn a_check_counts_as_running_until_it_ends() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let server = MockServer::start();
    let path = format!(
        "/{}/releases/download/{}/{}",
        pinned.repo, pinned.release, pinned.asset
    );
    server.mock(|when, then| {
        when.method(GET).path(path);
        then.status(200)
            .delay(Duration::from_millis(200))
            .body(published);
    });
    let downloads = ToolDownloads::default();
    assert!(!downloads.checking());

    let check = retry(
        tools.path().to_path_buf(),
        server.base_url(),
        vec![pinned],
        &downloads,
        &[],
    )
    .started()
    .expect("a check started");
    assert!(downloads.checking(), "Try again's check is not running");
    check.join().unwrap();

    assert!(!downloads.checking(), "the check still runs after it ended");
}

/// A check whose thread panics partway through a download leaves no program
/// downloading: the program it marked fails as interrupted, and an import
/// waiting for it wakes with that reason instead of waiting until Cancel.
#[test]
fn a_check_that_panics_mid_download_fails_the_download() {
    let downloads = ToolDownloads::default();
    let mut run = downloads.begin_check();
    run.mark_downloading(Program::Wtsexporter);

    let (panicked, waited) = std::thread::scope(|scope| {
        let check = scope.spawn(move || {
            let _run = run;
            std::thread::sleep(Duration::from_millis(50));
            panic!("the fake download panics");
        });
        let waited = downloads.wait_for(&[Program::Wtsexporter], &|| false, &mut |_, _, _| {});
        (check.join().is_err(), waited)
    });

    assert!(panicked);
    assert_eq!(
        waited,
        Err(WaitError::Failed {
            program: Program::Wtsexporter,
            reason: "The download was interrupted.".into(),
        })
    );
    assert!(!downloads.checking());
}

/// A download that arrives and runs is not a failed download when its
/// record can't be written: the program is found, and an import waiting for
/// it goes on.
#[test]
fn a_record_that_cannot_be_written_does_not_fail_a_good_download() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    // A directory where the record goes: renaming the record over it fails.
    std::fs::create_dir(tools.path().join(MANIFEST_FILE)).unwrap();
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let server = MockServer::start();
    let path = format!(
        "/{}/releases/download/{}/{}",
        pinned.repo, pinned.release, pinned.asset
    );
    server.mock(|when, then| {
        when.method(GET).path(path);
        then.status(200)
            .delay(Duration::from_millis(100))
            .body(published);
    });
    let downloads = ToolDownloads::default();

    let check = retry(
        tools.path().to_path_buf(),
        server.base_url(),
        vec![pinned],
        &downloads,
        &[Program::Wtsexporter],
    )
    .started()
    .expect("a check started");
    let waited = downloads.wait_for(&[Program::Wtsexporter], &|| false, &mut |_, _, _| {});
    check.join().unwrap();

    assert_eq!(waited, Ok(()));
    assert_eq!(downloads.get(Program::Wtsexporter), None);
    assert!(runs(Program::Wtsexporter, tools.path()));
}
