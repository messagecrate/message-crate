use super::*;
use tempfile::tempdir;

fn hash_bytes(data: &[u8]) -> Sha256 {
    Sha256::of_bytes(data)
}

#[test]
fn multipart_roundtrip() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let data = b"hello-multipart-asset-bytes!!";
    let sha = hash_bytes(data);

    let upload_id = "aabb0011";
    let session = session_dir(root, &sha, upload_id);
    fs::create_dir_all(&session).unwrap();
    let part_size = 10usize;
    let mut manifest = UploadManifest {
        sha256: sha.to_string(),
        bytes: data.len() as u64,
        part_size,
        mime: Some("text/plain".into()),
        received: BTreeSet::new(),
    };
    write_manifest(&session, &manifest).unwrap();

    let count = expected_part_count(manifest.bytes, part_size);
    for n in 1..=count {
        let start = (n as usize - 1) * part_size;
        let end = (start + part_size).min(data.len());
        let chunk = &data[start..end];
        let path = part_path(&session, n);
        fs::write(&path, chunk).unwrap();
        manifest.received.insert(n);
    }
    write_manifest(&session, &manifest).unwrap();

    let (stored, already) = complete_upload(root, &sha, upload_id).unwrap();
    assert!(!already);
    assert_eq!(stored.sha256, sha.as_str());
    assert!(root.join(&stored.assets_path).is_file());
    assert!(!session.exists());
    assert_eq!(fs::read(root.join(&stored.assets_path)).unwrap(), data);
}

#[test]
fn complete_rejects_hash_mismatch() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let data = b"abc123";
    let wrong_sha = hash_bytes(b"other");
    let upload_id = "badc0de1";
    let session = session_dir(root, &wrong_sha, upload_id);
    fs::create_dir_all(&session).unwrap();
    let mut manifest = UploadManifest {
        sha256: wrong_sha.to_string(),
        bytes: data.len() as u64,
        part_size: 64,
        mime: None,
        received: BTreeSet::new(),
    };
    fs::write(part_path(&session, 1), data).unwrap();
    manifest.received.insert(1);
    write_manifest(&session, &manifest).unwrap();

    let err = complete_upload(root, &wrong_sha, upload_id).unwrap_err();
    assert!(err.to_string().contains("sha256 mismatch"));
    assert!(!session.exists());
}

#[test]
fn complete_rejects_a_made_up_fingerprint() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let data = b"large-enough-to-skip";
    // Claimed fingerprint deliberately wrong: completion must still hash and reject.
    let claimed_sha = Sha256::parse(&"a".repeat(64)).unwrap();
    let upload_id = "aabbccdd";
    let session = session_dir(root, &claimed_sha, upload_id);
    fs::create_dir_all(&session).unwrap();
    let mut manifest = UploadManifest {
        sha256: claimed_sha.to_string(),
        bytes: data.len() as u64,
        part_size: 64,
        mime: None,
        received: BTreeSet::new(),
    };
    fs::write(part_path(&session, 1), data).unwrap();
    manifest.received.insert(1);
    write_manifest(&session, &manifest).unwrap();

    let err = complete_upload(root, &claimed_sha, upload_id).unwrap_err();
    assert!(
        err.to_string().contains("sha256 mismatch"),
        "expected mismatch, got: {err}"
    );
    assert!(!session.exists());
}

#[test]
fn require_upload_id_rejects_path_components() {
    assert!(require_upload_id("..").is_err());
    assert!(require_upload_id("../x").is_err());
    assert!(require_upload_id("a/b").is_err());
    assert!(require_upload_id("").is_err());
    assert!(require_upload_id("deadbeef").is_ok());
    assert!(require_upload_id(&"a".repeat(64)).is_ok());
    assert!(require_upload_id(&"a".repeat(65)).is_err());
}

#[test]
fn abort_rejects_parent_upload_id() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let sha = Sha256::parse(&"b".repeat(64)).unwrap();
    let parent = root.join(".incoming").join(sha.as_str());
    fs::create_dir_all(parent.join("keep")).unwrap();
    fs::write(parent.join("keep").join("marker"), b"x").unwrap();

    let err = abort_upload(root, &sha, "..").unwrap_err();
    assert!(err.to_string().contains("upload_id"));
    assert!(parent.join("keep").join("marker").is_file());
}

#[test]
fn complete_rejects_missing_part() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let data = b"0123456789abcdef";
    let sha = hash_bytes(data);
    let upload_id = "11112222";
    let session = session_dir(root, &sha, upload_id);
    fs::create_dir_all(&session).unwrap();
    let part_size = 8usize;
    let mut manifest = UploadManifest {
        sha256: sha.to_string(),
        bytes: data.len() as u64,
        part_size,
        mime: None,
        received: BTreeSet::new(),
    };
    // Only write part 1 of 2.
    fs::write(part_path(&session, 1), &data[..8]).unwrap();
    manifest.received.insert(1);
    write_manifest(&session, &manifest).unwrap();

    let err = complete_upload(root, &sha, upload_id).unwrap_err();
    assert!(err.to_string().contains("missing part"));
    // Incomplete sessions are kept so the client can resume missing parts.
    assert!(session.exists());
}

#[test]
fn put_part_and_complete_via_api() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let data = b"abcdefghijklmnopqrstuvwxyz";
    let sha = hash_bytes(data);
    // Tiny parts, set on this test's own limits so no other test sees them.
    let limits = UploadLimits::within(10, 1024);
    let (existing, start) =
        start_upload(root, &sha, data.len() as u64, Some("text/plain"), limits).unwrap();
    assert!(existing.is_none());
    let start = start.expect("upload started");
    assert_eq!(start.part_size, 10);

    let mut offset = 0usize;
    let mut part = 1u32;
    while offset < data.len() {
        let end = (offset + start.part_size).min(data.len());
        put_part(root, &sha, &start.upload_id, part, &data[offset..end]).unwrap();
        offset = end;
        part += 1;
    }
    let (stored, already) = complete_upload(root, &sha, &start.upload_id).unwrap();
    assert!(!already);
    assert_eq!(stored.sha256, sha.as_str());
}

#[test]
fn start_returns_existing() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let data = b"already-here";
    let sha = hash_bytes(data);
    let path = root.join(&sha.as_str()[..2]).join(sha.as_str());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, data).unwrap();

    let (existing, start) =
        start_upload(root, &sha, data.len() as u64, None, UploadLimits::default()).unwrap();
    assert!(existing.is_some());
    assert!(start.is_none());
}

#[test]
fn start_rejects_over_max_bytes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let sha = Sha256::parse(&"a".repeat(64)).unwrap();
    let limits = UploadLimits {
        part_size: 1024,
        max_bytes: 2048,
    };
    let err = start_upload(root, &sha, 4096, None, limits).unwrap_err();
    assert!(err.to_string().contains("server limit"));
}

/// The limit is the owner's and is never moved. A configured part size
/// above it is brought down to it; one below it is used as configured.
#[test]
fn a_part_is_never_larger_than_the_limit() {
    let small_limit = UploadLimits::within(4096, 1024);
    assert_eq!((small_limit.part_size, small_limit.max_bytes), (1024, 1024));

    let small_part = UploadLimits::within(10, 1024);
    assert_eq!((small_part.part_size, small_part.max_bytes), (10, 1024));
}

/// An account with as many uploads open as it may have is refused the
/// next one, with the limit named. Finishing one or aborting one makes
/// room for the next.
#[test]
fn an_upload_past_the_open_upload_limit_is_refused_until_one_ends() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let limits = UploadLimits {
        part_size: 1024,
        max_bytes: 2048,
    };
    let start = |data: &[u8]| {
        let sha = Sha256::of_bytes(data);
        start_upload(root, &sha, data.len() as u64, None, limits)
            .map(|(_, started)| (sha, started.unwrap().upload_id))
    };
    let files: Vec<Vec<u8>> = (0..=MAX_OPEN_UPLOADS)
        .map(|n| format!("file {n}").into_bytes())
        .collect();
    let mut open: Vec<(Sha256, String)> = files[..MAX_OPEN_UPLOADS]
        .iter()
        .map(|data| start(data).unwrap())
        .collect();
    let next = &files[MAX_OPEN_UPLOADS];

    let err = start(next).unwrap_err();
    assert!(matches!(err, AssetError::TooManyUploads), "{err}");
    assert!(
        err.to_string().contains(&MAX_OPEN_UPLOADS.to_string()),
        "the refusal does not name the limit: {err}"
    );

    let (sha, upload_id) = open.pop().unwrap();
    put_part(root, &sha, &upload_id, 1, &files[MAX_OPEN_UPLOADS - 1]).unwrap();
    complete_upload(root, &sha, &upload_id).unwrap();
    open.push(start(next).unwrap());
    assert!(start(b"one more").is_err());

    let (sha, upload_id) = open.pop().unwrap();
    abort_upload(root, &sha, &upload_id).unwrap();
    start(next).unwrap();
}

/// #2260: two starts for one fingerprint that are given the same upload
/// id, as two starts in one clock tick were, get an upload each. The
/// second takes the next id rather than writing over the first one's
/// manifest.
#[test]
fn two_starts_given_the_same_upload_id_get_an_upload_each() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let sha = Sha256::of_bytes(b"hello");
    let limits = UploadLimits {
        part_size: 1024,
        max_bytes: 2048,
    };
    let mut ids = ["aa", "aa", "bb"].into_iter().map(String::from);
    let mut start = |bytes: u64| {
        let (_, started) =
            start_upload_with_ids(root, &sha, bytes, None, limits, || ids.next().unwrap()).unwrap();
        started.unwrap().upload_id
    };

    let first = start(5);
    let second = start(6);

    assert_eq!((first.as_str(), second.as_str()), ("aa", "bb"));
    let first_manifest = read_manifest(&session_dir(root, &sha, &first)).unwrap();
    let second_manifest = read_manifest(&session_dir(root, &sha, &second)).unwrap();
    assert_eq!(
        first_manifest.bytes, 5,
        "the first upload keeps its manifest"
    );
    assert_eq!(second_manifest.bytes, 6);
}

/// #2261: a start no longer sweeps `.incoming/` each time, so an account
/// whose open uploads are all abandoned ones would be held at the limit
/// until a run ended. A start at the limit sweeps them first and goes
/// on, and an abandoned upload under the limit stays until a sweep.
#[test]
fn a_start_at_the_open_upload_limit_sweeps_abandoned_uploads_first() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let limits = UploadLimits {
        part_size: 1024,
        max_bytes: 2048,
    };
    let start = |data: &[u8]| {
        let sha = Sha256::of_bytes(data);
        let (_, started) = start_upload(root, &sha, data.len() as u64, None, limits).unwrap();
        session_dir(root, &sha, &started.unwrap().upload_id)
    };
    let abandoned: Vec<PathBuf> = (0..MAX_OPEN_UPLOADS)
        .map(|n| start(format!("file {n}").as_bytes()))
        .collect();
    for session in &abandoned {
        crate::asset_store::tests::make_abandoned(&manifest_path(session));
    }

    let fresh = start(b"one more");

    assert!(abandoned.iter().all(|session| !session.exists()));
    assert!(manifest_path(&fresh).is_file());
}

/// #2261: under the limit, a start leaves an abandoned upload to the
/// sweep when a run ends rather than walking `.incoming/` itself.
#[test]
fn a_start_under_the_open_upload_limit_does_not_sweep() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let limits = UploadLimits {
        part_size: 1024,
        max_bytes: 2048,
    };
    let sha = Sha256::of_bytes(b"left");
    let (_, started) = start_upload(root, &sha, 4, None, limits).unwrap();
    let abandoned = session_dir(root, &sha, &started.unwrap().upload_id);
    crate::asset_store::tests::make_abandoned(&manifest_path(&abandoned));

    start_upload(root, &Sha256::of_bytes(b"next"), 4, None, limits).unwrap();

    assert!(manifest_path(&abandoned).is_file());
}

/// #2260: an upload id carries random bytes beside the clock, so two
/// made at once differ, and the routes accept it.
#[test]
fn an_upload_id_is_one_the_routes_accept_and_two_differ() {
    let first = new_upload_id();
    let second = new_upload_id();

    assert_ne!(first, second);
    assert_eq!(require_upload_id(&first).unwrap(), first);
}

/// An upload of the five bytes `hello` in one part, started in a new
/// directory: the directory, the fingerprint, the upload id and its session.
fn started_upload() -> (tempfile::TempDir, Sha256, String, PathBuf) {
    let dir = tempdir().unwrap();
    let sha = Sha256::of_bytes(b"hello");
    let limits = UploadLimits {
        part_size: 1024,
        max_bytes: 2048,
    };
    let (_, start) = start_upload(dir.path(), &sha, 5, None, limits).unwrap();
    let upload_id = start.unwrap().upload_id;
    let session = session_dir(dir.path(), &sha, &upload_id);
    (dir, sha, upload_id, session)
}

/// A completion that passed the check for its upload, whose directory an
/// abort or the stale sweep then removed, finds the upload gone: the
/// client's own state, not a server that cannot store the file.
#[test]
fn a_completion_for_an_upload_removed_under_it_finds_no_upload() {
    let (dir, sha, _, session) = started_upload();
    fs::remove_dir_all(&session).unwrap();

    // `complete_upload` checks the directory before it gets here, so the
    // removal between that check and the work is staged by calling the
    // work after the check directly.
    let err = complete_session(dir.path(), &session, &sha).unwrap_err();
    assert!(matches!(err, AssetError::UploadNotFound), "{err}");
}

/// A removal deletes an upload's files before its directory. A request that
/// arrives while the manifest is gone and the directory is not finds the
/// upload gone too.
#[test]
fn a_request_to_an_upload_part_way_through_its_removal_finds_no_upload() {
    let (dir, sha, upload_id, session) = started_upload();
    fs::remove_file(manifest_path(&session)).unwrap();

    let err = put_part(dir.path(), &sha, &upload_id, 1, b"hello").unwrap_err();
    assert!(matches!(err, AssetError::UploadNotFound), "{err}");
    let err = complete_upload(dir.path(), &sha, &upload_id).unwrap_err();
    assert!(matches!(err, AssetError::UploadNotFound), "{err}");
    let err = read_upload(dir.path(), &sha, &upload_id).unwrap_err();
    assert!(matches!(err, AssetError::UploadNotFound), "{err}");
}

/// A part whose file is gone while its upload's manifest stays, as a
/// removal that failed part-way leaves it, is a part to send again: the
/// upload is still live.
#[test]
fn a_completion_whose_received_part_is_gone_asks_for_the_part_again() {
    let (dir, sha, upload_id, session) = started_upload();
    put_part(dir.path(), &sha, &upload_id, 1, b"hello").unwrap();
    fs::remove_file(part_path(&session, 1)).unwrap();

    let err = complete_upload(dir.path(), &sha, &upload_id).unwrap_err();
    assert!(matches!(err, AssetError::Invalid(_)), "{err}");
    put_part(dir.path(), &sha, &upload_id, 1, b"hello").unwrap();
    complete_upload(dir.path(), &sha, &upload_id).unwrap();
}

/// An abort while a part is still being written waits its turn, and
/// leaves the upload in place.
#[test]
fn an_abort_while_a_part_is_written_is_refused_as_locked() {
    let (dir, sha, upload_id, session) = started_upload();

    let held = lock_session(&session).unwrap();
    let err = abort_upload(dir.path(), &sha, &upload_id).unwrap_err();
    assert!(matches!(err, AssetError::Locked), "{err}");
    assert!(manifest_path(&session).is_file());

    drop(held);
    abort_upload(dir.path(), &sha, &upload_id).unwrap();
    assert!(!session.exists());
    abort_upload(dir.path(), &sha, &upload_id).unwrap();
}

/// A process the server starts (ffmpeg, say) while a request holds an
/// upload's lock gets a copy of every descriptor the server has open, and
/// keeps it until it begins running its program. A lock on a file stays
/// held while any copy of its descriptor is open, so the next part was
/// refused as locked after the request had let go. Holding an upload's
/// lock must open no descriptor a started process could copy; this fails
/// if the lock goes back to being a file in the upload's directory.
#[cfg(target_os = "linux")]
#[test]
fn holding_an_upload_lock_opens_no_descriptor_a_started_process_could_copy() {
    let (_dir, _sha, _upload_id, session) = started_upload();
    let _held = lock_session(&session).unwrap();
    // `/proc/self/fd` gives each target as an absolute path with every
    // symlink resolved, so the directory is compared in that form too.
    let resolved = fs::canonicalize(&session).unwrap();
    let open_in_session: Vec<PathBuf> = fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
        .filter(|target| target.starts_with(&resolved))
        .collect();
    assert!(open_in_session.is_empty(), "{open_in_session:?}");
}

/// S2-8: a second request to an upload while the first holds the lock is
/// refused without waiting. Its bytes may be right, so the refusal names
/// the lock and says to send again, and does not name a server path.
#[test]
fn a_request_refused_for_the_lock_says_so() {
    let dir = tempdir().unwrap();
    let sha = Sha256::parse(&"c".repeat(64)).unwrap();
    let session = session_dir(dir.path(), &sha, "locktest01");
    fs::create_dir_all(&session).unwrap();

    let _held = lock_session(&session).unwrap();
    let err = lock_session(&session).unwrap_err();
    assert!(
        matches!(err, AssetError::Locked),
        "expected lock failure, got: {err}"
    );
    let message = err.to_string();
    assert!(
        message.contains("another request to this upload holds its lock"),
        "expected the lock to be named, got: {message}"
    );
    assert!(
        !message.contains(&dir.path().display().to_string()),
        "the refusal names a server path: {message}"
    );
}
