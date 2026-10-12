use super::*;

use std::ffi::OsString;
use std::io::{Read, Write};

use file_io::sha256_hex;
use httpmock::prelude::*;

impl Retry {
    /// The check's thread, when one started.
    fn started(self) -> Option<JoinHandle<()>> {
        match self {
            Self::Started(handle) => Some(handle),
            Self::AlreadyRunning | Self::CouldNotStart => None,
        }
    }
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
    gzipped: bool,
    sha256: &str,
) -> (Pinned, Vec<u8>) {
    let pinned = Pinned {
        program,
        repo: "owner/project",
        release,
        asset: leak(format!(
            "{}-test{}",
            program.name(),
            if gzipped { ".gz" } else { "" }
        )),
        sha256: leak(sha256.to_string()),
        program_sha256: leak(if gzipped {
            let mut program = Vec::new();
            flate2::read::GzDecoder::new(published)
                .read_to_end(&mut program)
                .unwrap();
            sha256_hex(&program)
        } else {
            sha256.to_string()
        }),
        gzipped,
    };
    (pinned, published.to_vec())
}

/// `program` pinned to the asset `published`, with its true checksum.
fn pin(
    program: Program,
    release: &'static str,
    published: &[u8],
    gzipped: bool,
) -> (Pinned, Vec<u8>) {
    pin_with(program, release, published, gzipped, &sha256_hex(published))
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

/// [`download_missing`] as one check of `downloads`, on this thread. Each
/// wait before a download is tried again returns at once, and the waits
/// asked for are returned, in order.
fn check_now(
    dir: &Path,
    base: &str,
    pinned: &[Pinned],
    downloads: &ToolDownloads,
) -> Vec<Duration> {
    let waits = std::cell::RefCell::new(Vec::new());
    check_waiting(dir, base, pinned, downloads, &|pause| {
        waits.borrow_mut().push(pause);
    });
    waits.into_inner()
}

/// [`download_missing`] as one check of `downloads`, on this thread, with
/// `wait` in place of the pause before a download is tried again.
fn check_waiting(
    dir: &Path,
    base: &str,
    pinned: &[Pinned],
    downloads: &ToolDownloads,
    wait: &dyn Fn(Duration),
) {
    download_missing(
        dir,
        &HttpFetch::new(base),
        pinned,
        downloads.begin_check(),
        wait,
    );
}

/// A server on this computer that closes a connection without an answer, as
/// when the network is not there, unless `answers` says to answer the
/// request: it is given the request's number, counted from 0. A request it
/// answers gets `body`, whatever its path.
struct NoNetworkUnless {
    base: String,
    requests: Arc<AtomicUsize>,
}

fn no_network_unless(
    body: Vec<u8>,
    answers: impl Fn(usize) -> bool + Send + 'static,
) -> NoNetworkUnless {
    use std::io::BufRead;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let counting = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // The request's head, up to the empty line that ends it.
            let mut reader = io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|read| read > 0) && line != "\r\n" {
                line.clear();
            }
            if answers(counting.fetch_add(1, Ordering::SeqCst)) {
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        }
    });
    NoNetworkUnless { base, requests }
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

/// A fetch that writes `bytes` for any pin, as if the download arrived.
struct FixedFetch {
    bytes: Vec<u8>,
}

impl Fetch for FixedFetch {
    fn fetch(
        &self,
        _pinned: &Pinned,
        into: &mut dyn Write,
        progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<(), DownloadError> {
        let total = self.bytes.len() as u64;
        into.write_all(&self.bytes)
            .map_err(|err| DownloadError::CouldNotWrite(err.to_string()))?;
        progress(total, Some(total));
        Ok(())
    }
}

/// [`download_missing`] as one check of `downloads`, getting every file
/// with a fetch that writes `bytes`. A wait before a try again returns at
/// once.
fn check_fetching(dir: &Path, bytes: &[u8], pinned: &[Pinned], downloads: &ToolDownloads) {
    let fetch = FixedFetch {
        bytes: bytes.to_vec(),
    };
    download_missing(dir, &fetch, pinned, downloads.begin_check(), &|_| {});
}

/// What a fetch writes is refused when its SHA-256 is not the pinned one,
/// with no server: no program and no temporary file is left in the Tools
/// Directory, and the failure names both checksums.
#[test]
fn a_fetched_file_whose_checksum_does_not_match_is_not_left_behind() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, _) = pin_with(
        Program::Wtsexporter,
        "r1",
        b"",
        false,
        &sha256_hex(b"the pinned file"),
    );
    let downloads = ToolDownloads::default();

    check_fetching(
        tools.path(),
        b"a file changed on the way",
        std::slice::from_ref(&pinned),
        &downloads,
    );

    assert_eq!(other_files(tools.path(), &[]), Vec::<String>::new());
    assert_eq!(
        downloads.get(Program::Wtsexporter),
        Some(DownloadState::Failed {
            reason: DownloadError::ChecksumMismatch {
                expected: sha256_hex(b"the pinned file"),
                actual: sha256_hex(b"a file changed on the way"),
            }
            .to_string(),
        })
    );
}

/// A gzipped asset that passes its checksum and does not unpack puts no
/// program in place and leaves no temporary file, and the program already
/// there stays. The reason says the file did not unpack, not that the Tools
/// Directory could not be written.
#[test]
fn a_fetched_file_that_does_not_unpack_leaves_the_old_program() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let target = tools.path().join(Program::Wtsexporter.file_name());
    std::fs::write(&target, b"the old wtsexporter").unwrap();
    let not_gzip = b"not a gzip stream";
    let pinned = Pinned {
        program: Program::Wtsexporter,
        repo: "owner/project",
        release: "r2",
        asset: "wtsexporter-test.gz",
        sha256: leak(sha256_hex(not_gzip)),
        program_sha256: leak(sha256_hex(b"wtsexporter")),
        gzipped: true,
    };
    let downloads = ToolDownloads::default();

    check_fetching(
        tools.path(),
        not_gzip,
        std::slice::from_ref(&pinned),
        &downloads,
    );

    assert_eq!(std::fs::read(&target).unwrap(), b"the old wtsexporter");
    assert_eq!(
        other_files(tools.path(), &[&Program::Wtsexporter.file_name()]),
        Vec::<String>::new()
    );
    // The unpack's own failure, not the program's checksum after it.
    let unpack_error = io::copy(
        &mut flate2::read::GzDecoder::new(&not_gzip[..]),
        &mut io::sink(),
    )
    .expect_err("the bytes unpacked");
    // The old file is not the pinned one, so it is not used, and the reason
    // says so first (#2692).
    assert_eq!(
        downloads.get(Program::Wtsexporter),
        Some(DownloadState::Failed {
            reason: format!(
                "The wtsexporter in the Tools Directory is not the file of release r2, or is \
                 still being copied there, so it is not used. The downloaded file could not \
                 be unpacked: {unpack_error}."
            ),
        })
    );
}

/// A gzipped file that ends partway through did not unpack, while a
/// failure to read the file back, such as the disk under the Tools
/// Directory, is the Tools Directory's fault, not the download's.
#[test]
fn only_bad_gzip_data_is_a_file_that_does_not_unpack() {
    let whole = gzipped(b"ffmpeg");
    let mut cut_short = &whole[..whole.len() / 2];
    assert!(matches!(
        super::fetch::unpack(&mut cut_short, &mut io::sink()),
        Err(DownloadError::CouldNotUnpack(_))
    ));

    struct FailingDisk;
    impl Read for FailingDisk {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("the disk failed"))
        }
    }
    assert_eq!(
        super::fetch::unpack(&mut FailingDisk, &mut io::sink()),
        Err(DownloadError::CouldNotWrite("the disk failed".to_string()))
    );
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

/// What [`write_runnable`] writes: a program that answers `-version`.
#[cfg(unix)]
const RUNNABLE: &[u8] = b"#!/bin/sh\nexit 0\n";

/// Write a program at `path` that answers `-version`.
#[cfg(unix)]
fn write_runnable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, RUNNABLE).unwrap();
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
             Install it with your package manager instead. The app uses the copy on PATH."
        );
    }

    asked.assert_calls(1);
}

/// A recorded wtsexporter with an executable bit this account may not run,
/// as a root-owned `0o744` file is to any other account, is not kept as in
/// place: it is said not to run, and not downloaded again (#2793). It skips
/// where [`media::testutil::may_read_any_file`] says so.
#[cfg(unix)]
#[test]
fn a_recorded_wtsexporter_this_account_may_not_run_does_not_run() {
    use std::os::unix::fs::PermissionsExt;
    let tools = tempfile::tempdir().unwrap();
    if media::testutil::may_read_any_file(tools.path()) {
        return;
    }
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"pinned", false);
    let target = tools.path().join(Program::Wtsexporter.file_name());
    std::fs::write(&target, b"pinned").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o611)).unwrap();
    let mut manifest = Manifest::new();
    manifest.insert(
        Program::Wtsexporter,
        Written::of(&pinned, Stamp::at(&target).unwrap()),
    );
    write_manifest(tools.path(), &manifest).unwrap();
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
    assert_eq!(
        downloads.get(Program::Wtsexporter),
        Some(DownloadState::Failed {
            reason: "wtsexporter in the Tools Directory doesn't run on this computer.".to_string(),
        })
    );
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

/// An adopted program that does not run, and whose executable bits could
/// not be set, fails with the reason they could not, not the generic "does
/// not run" text, so Settings says what to fix, and ffmpeg keeps the advice
/// to use the copy on `PATH`.
#[test]
fn an_adopted_program_that_could_not_be_made_executable_says_why() {
    let denied = io::Error::new(io::ErrorKind::PermissionDenied, "Operation not permitted");

    let Adopted::NotExecutable(err) = classify_adopted(Err(denied), false) else {
        panic!("a program that does not run and could not be made executable");
    };

    assert_eq!(
        DownloadError::CouldNotMakeExecutable(Program::Ffmpeg, err.to_string()).to_string(),
        "ffmpeg in the Tools Directory could not be marked as a program that runs: \
         Operation not permitted. Install it with your package manager instead. \
         The app uses the copy on PATH."
    );
}

/// An adopted program that runs is in place whether or not its bits could
/// be set, and keeps the error when they could not. One whose bits were set
/// and that does not run fails as not running.
#[test]
fn an_adopted_program_blames_its_bits_only_when_setting_them_failed() {
    let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "Operation not permitted");

    assert!(matches!(
        classify_adopted(Ok(()), true),
        Adopted::Runs(None)
    ));
    assert!(matches!(
        classify_adopted(Err(denied()), true),
        Adopted::Runs(Some(_))
    ));
    assert!(matches!(
        classify_adopted(Ok(()), false),
        Adopted::DoesNotRun
    ));
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

/// With no network the check tries again after each wait, then fails
/// quietly, saying so, and leaves nothing. The waits are spent once per
/// check, not once per program, so a network that never comes keeps an
/// import waiting for its program no longer than one round of them.
#[test]
fn no_network_is_a_failed_download_that_says_so() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (wts, _) = pin(Program::Wtsexporter, "r1", b"pinned", false);
    let (ffprobe, _) = pin(Program::Ffprobe, "r1", &gzipped(b"ffprobe"), true);
    let downloads = ToolDownloads::default();

    // Port 9 on this computer, where nothing listens.
    let waits = check_now(
        tools.path(),
        "http://127.0.0.1:9",
        &[ffprobe, wts],
        &downloads,
    );

    assert_eq!(waits, NO_NETWORK_WAITS);
    for program in [Program::Ffprobe, Program::Wtsexporter] {
        let Some(DownloadState::Failed { reason }) = downloads.get(program) else {
            panic!("the {} download did not fail", program.name());
        };
        assert!(reason.starts_with("No connection"), "{reason}");
    }
    assert_eq!(other_files(tools.path(), &[]), Vec::<String>::new());
}

/// A download that finds no network, as on a laptop opened before its
/// Wi-Fi connects, is tried again after a growing wait, and shows as
/// downloading all the while, so Settings says it is still trying and an
/// import waiting for it goes on waiting (#2200).
#[test]
fn no_network_is_tried_again_after_a_growing_wait() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, published) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    // The network comes during the second wait.
    let server = no_network_unless(published, |request| request >= 2);
    let downloads = ToolDownloads::default();
    let waits = std::cell::RefCell::new(Vec::new());

    check_waiting(
        tools.path(),
        &server.base,
        &[pinned],
        &downloads,
        &|pause| {
            assert_eq!(
                downloads.get(Program::Wtsexporter),
                Some(DownloadState::Downloading {
                    received: 0,
                    total: None
                }),
                "the download was not shown as downloading while it waited"
            );
            waits.borrow_mut().push(pause);
        },
    );

    assert_eq!(waits.into_inner(), NO_NETWORK_WAITS[..2]);
    assert!(
        NO_NETWORK_WAITS.is_sorted_by(|a, b| a < b),
        "the waits do not grow"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 3);
    assert_eq!(downloads.get(Program::Wtsexporter), None);
    assert_eq!(
        std::fs::read(tools.path().join(Program::Wtsexporter.file_name())).unwrap(),
        b"wtsexporter"
    );
}

/// A download that reaches its server starts the waits over, so the next
/// program that finds no network gets the whole round again, because the
/// network was there a moment before.
#[test]
fn a_download_that_reaches_its_server_starts_the_waits_over() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (first, published) = pin(Program::Ffprobe, "r1", b"the same file", false);
    let (second, _) = pin(Program::Wtsexporter, "r1", b"the same file", false);
    // The first program's second try is answered, and nothing after it.
    let server = no_network_unless(published, |request| request == 1);
    let downloads = ToolDownloads::default();

    let waits = check_now(tools.path(), &server.base, &[first, second], &downloads);

    let mut expected = vec![NO_NETWORK_WAITS[0]];
    expected.extend(NO_NETWORK_WAITS);
    assert_eq!(waits, expected);
    assert!(tools.path().join(Program::Ffprobe.file_name()).exists());
    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Wtsexporter) else {
        panic!("the wtsexporter download did not fail");
    };
    assert!(reason.starts_with("No connection"), "{reason}");
}

/// A program put in the Tools Directory by hand while the check waits for
/// the network, as Troubleshooting suggests, ends the tries: it is used,
/// and an import waiting for it goes on instead of failing for a download
/// it no longer needs.
#[cfg(unix)]
#[test]
fn a_program_put_in_place_during_a_wait_ends_the_tries() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, _) = pin(Program::Wtsexporter, "r1", RUNNABLE, false);
    let target = tools.path().join(Program::Wtsexporter.file_name());
    let downloads = ToolDownloads::default();
    let waits = std::cell::RefCell::new(Vec::new());

    // Port 9 on this computer, where nothing listens.
    check_waiting(
        tools.path(),
        "http://127.0.0.1:9",
        &[pinned],
        &downloads,
        &|pause| {
            waits.borrow_mut().push(pause);
            write_runnable(&target);
        },
    );

    assert_eq!(waits.into_inner(), NO_NETWORK_WAITS[..1]);
    assert_eq!(downloads.get(Program::Wtsexporter), None);
    assert_eq!(std::fs::read(&target).unwrap(), RUNNABLE);
}

/// A wtsexporter still being copied in by hand while the check waits for
/// the network is not the pinned file, so it does not end the tries, and
/// the failed download says the file there is not used (#2692). An import
/// waiting for it fails with that reason instead of running a cut-off file.
#[cfg(unix)]
#[test]
fn a_truncated_wtsexporter_put_in_place_during_a_wait_is_not_used() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, _) = pin(Program::Wtsexporter, "r1", RUNNABLE, false);
    let target = tools.path().join(Program::Wtsexporter.file_name());
    let downloads = ToolDownloads::default();
    let waits = std::cell::RefCell::new(Vec::new());

    // Port 9 on this computer, where nothing listens.
    check_waiting(
        tools.path(),
        "http://127.0.0.1:9",
        &[pinned],
        &downloads,
        &|pause| {
            let mut waits = waits.borrow_mut();
            waits.push(pause);
            // A little more of the copy at each wait, never all of it.
            std::fs::write(&target, &RUNNABLE[..waits.len()]).unwrap();
            make_executable(&target).unwrap();
        },
    );

    assert_eq!(waits.into_inner(), NO_NETWORK_WAITS);
    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Wtsexporter) else {
        panic!("the wtsexporter download did not fail");
    };
    assert!(
        reason.starts_with(
            "The wtsexporter in the Tools Directory is not the file of release r1, or is still \
             being copied there, so it is not used. No connection"
        ),
        "{reason}"
    );
}

/// A program put in place by hand while an earlier program's tries wait is
/// used too, though the waits are spent by the time its turn comes.
#[cfg(unix)]
#[test]
fn a_program_put_in_place_during_another_programs_wait_is_used() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (ffprobe, _) = pin(Program::Ffprobe, "r1", b"ffprobe", false);
    let (wts, _) = pin(Program::Wtsexporter, "r1", RUNNABLE, false);
    let target = tools.path().join(Program::Wtsexporter.file_name());
    let downloads = ToolDownloads::default();
    let waits = std::cell::RefCell::new(Vec::new());

    // Port 9 on this computer, where nothing listens.
    check_waiting(
        tools.path(),
        "http://127.0.0.1:9",
        &[ffprobe, wts],
        &downloads,
        &|pause| {
            let mut waits = waits.borrow_mut();
            waits.push(pause);
            if waits.len() == 1 {
                write_runnable(&target);
            }
        },
    );

    assert_eq!(waits.into_inner(), NO_NETWORK_WAITS);
    assert!(matches!(
        downloads.get(Program::Ffprobe),
        Some(DownloadState::Failed { .. })
    ));
    assert_eq!(downloads.get(Program::Wtsexporter), None);
}

/// A program that was already in place before the check and is not the
/// pinned one does not end the tries: the check still wants the pinned
/// release, and with no network it fails after the whole round. A
/// wtsexporter that is another build is not used meanwhile, and the
/// failure says so (#2692).
#[cfg(unix)]
#[test]
fn a_program_already_in_place_does_not_end_the_tries() {
    let tools = tempfile::tempdir().unwrap();
    let _tools = no_ffmpeg_on_path(tools.path());
    let (pinned, _) = pin(Program::Wtsexporter, "r1", b"pinned", false);
    write_runnable(&tools.path().join(Program::Wtsexporter.file_name()));
    let downloads = ToolDownloads::default();

    let waits = check_now(tools.path(), "http://127.0.0.1:9", &[pinned], &downloads);

    assert_eq!(waits, NO_NETWORK_WAITS);
    let Some(DownloadState::Failed { reason }) = downloads.get(Program::Wtsexporter) else {
        panic!("the wtsexporter download did not fail");
    };
    assert!(
        reason.starts_with("The wtsexporter in the Tools Directory is not the file of release r1"),
        "{reason}"
    );
}

/// A download that reached its server and failed for another reason, such
/// as a checksum that does not match, is not tried again: the same request
/// would fail the same way.
#[test]
fn a_failure_other_than_no_network_is_not_tried_again() {
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
    let asked = serve(&server, &pinned, b"a file changed on the way");
    let downloads = ToolDownloads::default();

    let waits = check_now(tools.path(), &server.base_url(), &[pinned], &downloads);

    assert_eq!(waits, Vec::<Duration>::new());
    asked.assert_calls(1);
    assert!(matches!(
        downloads.get(Program::Wtsexporter),
        Some(DownloadState::Failed { .. })
    ));
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

/// A Try again whose thread can't start fails each program it marked with
/// that reason, and no program is ever shown as interrupted on the way, so
/// an import waiting for one wakes with the real cause.
#[test]
fn a_try_again_whose_thread_cannot_start_fails_with_that_reason() {
    let tools = tempfile::tempdir().unwrap();
    let (pinned, _) = pin(Program::Wtsexporter, "r1", b"wtsexporter", false);
    let downloads = ToolDownloads::default();
    let mut seen = None;

    let retried = retry_with(
        tools.path().to_path_buf(),
        Box::new(HttpFetch::new("http://127.0.0.1:9")),
        vec![pinned],
        &downloads,
        &[Program::Wtsexporter],
        |work| {
            // A failed start drops the work without running it.
            drop(work);
            seen = downloads.get(Program::Wtsexporter);
            Err(io::Error::other("no threads left"))
        },
    );

    assert!(matches!(retried, Retry::CouldNotStart));
    assert!(
        matches!(seen, Some(DownloadState::Downloading { .. })),
        "the program was {seen:?} before the start failed"
    );
    assert_eq!(
        downloads.get(Program::Wtsexporter),
        Some(DownloadState::Failed {
            reason: DownloadError::CouldNotStart("no threads left".into()).to_string(),
        })
    );
    assert!(!downloads.checking());
}
