use super::*;
use crate::export_dir;

/// The committed bytes of the image placeholder.
fn placeholder_jpg() -> &'static [u8] {
    PLACEHOLDER_FILES
        .iter()
        .find(|(rel, _)| rel.ends_with(".jpg"))
        .expect("an image placeholder")
        .1
}

/// The placeholder pass replaces real media with three stand-in files and
/// deletes everything else, which is the whole point: an obfuscated export
/// must not ship the photographs.
#[test]
fn materializing_placeholders_removes_the_real_media_and_keeps_the_three() {
    let dir = export_dir();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    fs::write(attachments.join("IMG_0001.jpg"), b"real photo bytes").expect("write");
    fs::write(attachments.join("clip.mp4"), b"real video bytes").expect("write");
    fs::write(attachments.join("notes.bin"), b"real other bytes").expect("write");

    materialize_placeholders(dir.path()).expect("materialize");

    let mut names: Vec<String> = fs::read_dir(&attachments)
        .expect("read")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["placeholder.bin", "placeholder.jpg", "placeholder.mp4"],
        "the real media must be gone and the three placeholders present"
    );
    assert_eq!(
        fs::read(attachments.join("placeholder.jpg")).expect("read jpg"),
        placeholder_jpg(),
        "the placeholder must be the committed bytes, not a leftover file"
    );
}

/// Running it twice must be a no-op, not a pass that deletes what the
/// first one wrote.
#[test]
fn materializing_placeholders_twice_keeps_them() {
    let dir = export_dir();
    materialize_placeholders(dir.path()).expect("first");
    materialize_placeholders(dir.path()).expect("second");

    let attachments = dir.path().join("attachments");
    assert!(attachments.join("placeholder.jpg").is_file());
    assert!(attachments.join("placeholder.mp4").is_file());
    assert!(attachments.join("placeholder.bin").is_file());
}

/// A real attachment the pass cannot delete stays in the obfuscated
/// export, and that export exists to be shared without the real content.
/// The pass must fail and name the file, not report success (#1139).
#[cfg(unix)]
#[test]
fn a_real_attachment_that_cannot_be_removed_fails_the_pass_and_names_the_file() {
    let dir = export_dir();
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    let photo = attachments.join("IMG_0001.jpg");
    fs::write(&photo, b"real photo bytes").expect("write");

    let Some(result) =
        crate::with_directory_mode(&attachments, 0o555, || materialize_placeholders(dir.path()))
    else {
        return;
    };

    let err = result.expect_err("a file that cannot be removed must fail the pass");
    let message = format!("{err:#}");
    assert!(
        message.contains(&photo.display().to_string()),
        "the error must name the file it could not remove: {message}"
    );
    assert!(
        photo.is_file(),
        "the test's premise: the file is still there"
    );
}

/// A real attachment in a subdirectory that the pass cannot delete fails
/// the pass, and the error names that file, not only its directory (#1406).
#[cfg(unix)]
#[test]
fn a_file_in_a_subdirectory_that_cannot_be_removed_is_the_one_the_error_names() {
    let dir = export_dir();
    let sub = dir.path().join("attachments").join("sub");
    fs::create_dir_all(&sub).expect("subdirectory");
    let photo = sub.join("photo.jpg");
    fs::write(&photo, b"real photo bytes").expect("write");

    let Some(result) =
        crate::with_directory_mode(&sub, 0o555, || materialize_placeholders(dir.path()))
    else {
        return;
    };

    let message = format!("{:#}", result.expect_err("the pass must fail"));
    assert!(
        message.contains(&photo.display().to_string()),
        "the error must name the file that stayed: {message}"
    );
}

/// A real attachment in a subdirectory of `attachments/` is real content
/// too, and the obfuscated export exists to leave it out (#1406).
#[test]
fn materializing_placeholders_removes_real_media_in_a_subdirectory() {
    let dir = export_dir();
    let sub = dir.path().join("attachments").join("sub");
    fs::create_dir_all(&sub).expect("subdirectory");
    fs::write(sub.join("photo.jpg"), b"real photo bytes").expect("write");

    materialize_placeholders(dir.path()).expect("materialize");

    assert!(
        !sub.exists(),
        "the subdirectory and the photo in it must be gone"
    );
}

/// A symlink under `attachments/` is removed, never followed: the
/// directory it points at, outside the export, keeps its files.
#[cfg(unix)]
#[test]
fn a_symlink_to_a_directory_outside_the_export_is_removed_not_followed() {
    let dir = export_dir();
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("keep.jpg"), b"not the export's").expect("write");
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    let link = attachments.join("linked");
    std::os::unix::fs::symlink(outside.path(), &link).expect("symlink");

    materialize_placeholders(dir.path()).expect("materialize");

    assert!(
        fs::symlink_metadata(&link).is_err(),
        "the symlink must be gone"
    );
    assert_eq!(
        fs::read(outside.path().join("keep.jpg")).expect("read"),
        b"not the export's",
        "the directory the symlink pointed at must be untouched"
    );
}

/// A symlink with a placeholder's name is not kept as a placeholder:
/// writing the placeholder through it would overwrite a file outside the
/// export.
#[cfg(unix)]
#[test]
fn a_symlink_named_like_a_placeholder_is_replaced_by_a_real_placeholder() {
    let dir = export_dir();
    let outside = tempfile::tempdir().expect("outside");
    let target = outside.path().join("elsewhere.jpg");
    fs::write(&target, b"not the export's").expect("write");
    let attachments = dir.path().join("attachments");
    fs::create_dir_all(&attachments).expect("attachments dir");
    let placeholder = attachments.join("placeholder.jpg");
    std::os::unix::fs::symlink(&target, &placeholder).expect("symlink");

    materialize_placeholders(dir.path()).expect("materialize");

    let meta = fs::symlink_metadata(&placeholder).expect("placeholder");
    assert!(
        meta.file_type().is_file(),
        "the placeholder must be a real file"
    );
    assert_eq!(fs::read(&placeholder).expect("read"), placeholder_jpg());
    assert_eq!(
        fs::read(&target).expect("read target"),
        b"not the export's",
        "the file the symlink pointed at must be untouched"
    );
}

/// The obfuscated export's placeholders replace everything under
/// `attachments/`, so a directory without the `.message-crate-export`
/// sentinel is refused and nothing in it is removed (#1531).
#[test]
fn placeholders_refuse_a_directory_without_the_sentinel_and_remove_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let att = tmp.path().join("attachments");
    fs::create_dir_all(att.join("2024")).expect("write");
    fs::write(att.join("photo.jpg"), b"mine").expect("write");
    fs::write(att.join("2024/video.mp4"), b"mine").expect("write");

    assert!(materialize_placeholders(tmp.path()).is_err());

    assert!(att.join("photo.jpg").is_file());
    assert!(att.join("2024/video.mp4").is_file());
    assert!(!att.join("placeholder.jpg").exists());
}
