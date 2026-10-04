use super::*;

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    out.sort();
    out
}

#[test]
fn refuses_a_folder_of_the_persons_own_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

    let err = clean_previous_ir_output(tmp.path()).unwrap_err();

    assert!(
        err.to_string().contains("Refusing to write into it"),
        "{err}"
    );
    assert_eq!(names(tmp.path()), ["notes.txt"]);
}

#[test]
fn removes_only_export_files_from_a_marked_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_export_sentinel(dir).unwrap();
    for name in [
        "a.jsonl",
        "b.csv",
        "c.json",
        "c.meta.json",
        "d.jsonl.tmp",
        "notes.txt",
    ] {
        fs::write(dir.join(name), "x").unwrap();
    }
    fs::create_dir(dir.join("attachments")).unwrap();
    fs::write(dir.join("attachments").join("a.jpg"), "x").unwrap();
    // A folder named like an export file is not an export file.
    fs::create_dir(dir.join("kept.json")).unwrap();

    clean_previous_ir_output(dir).unwrap();

    assert_eq!(names(dir), [EXPORT_SENTINEL, "kept.json", "notes.txt"]);
}

#[test]
fn cleans_a_marked_folder_that_holds_no_export_files() {
    let tmp = tempfile::tempdir().unwrap();
    write_export_sentinel(tmp.path()).unwrap();
    fs::write(tmp.path().join("notes.txt"), "mine").unwrap();

    clean_previous_ir_output(tmp.path()).unwrap();

    assert_eq!(names(tmp.path()), [EXPORT_SENTINEL, "notes.txt"]);
}

#[test]
fn refuses_an_unmarked_folder_that_holds_export_like_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("budget.csv"), "mine").unwrap();
    fs::write(tmp.path().join("settings.json"), "{}").unwrap();
    fs::write(tmp.path().join("notes.txt"), "mine").unwrap();
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/holiday.jpg"), "mine").unwrap();

    let err = clean_previous_ir_output(tmp.path()).unwrap_err();

    assert!(
        err.to_string().contains(&tmp.path().display().to_string()),
        "{err}"
    );
    assert_eq!(
        names(tmp.path()),
        ["attachments", "budget.csv", "notes.txt", "settings.json"]
    );
    assert!(tmp.path().join("attachments/holiday.jpg").exists());
}

/// Every exporter cleans through here, so the files a merged archive
/// recorded go whatever the next run writes, and an XML file nothing
/// recorded, such as a person's own backup, stays.
#[test]
fn removes_the_files_an_archive_recorded_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("export");
    fs::create_dir(&dir).unwrap();
    let dir = dir.as_path();
    write_export_sentinel(dir).unwrap();
    let recorded = ["archive.xml", "archive.xml.tmp"].map(String::from);
    record_archive_files(dir, &recorded).unwrap();
    // A damaged list cannot reach outside the folder.
    record_archive_files(dir, &["../outside.xml".to_string()]).unwrap();
    let outside = tmp.path().join("outside.xml");
    fs::write(&outside, "mine").unwrap();
    for name in ["archive.xml", "archive.xml.tmp", "sms-20261001.xml"] {
        fs::write(dir.join(name), "x").unwrap();
    }

    clean_previous_ir_output(dir).unwrap();

    assert_eq!(names(dir), [EXPORT_SENTINEL, "sms-20261001.xml"]);
    assert!(outside.is_file(), "a file outside the folder is kept");
    assert_eq!(fs::read_to_string(dir.join(EXPORT_SENTINEL)).unwrap(), "");
}

/// Pull and WhatsApp mark a folder without cleaning it, so marking it
/// again keeps the archive files an earlier export listed for the next
/// clean.
#[test]
fn marking_a_folder_again_keeps_the_archive_files_it_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_export_sentinel(dir).unwrap();
    record_archive_files(dir, &["archive.xml".to_string()]).unwrap();
    fs::write(dir.join("archive.xml"), "x").unwrap();

    mark_export_folder(dir).unwrap();
    clean_previous_ir_output(dir).unwrap();

    assert_eq!(names(dir), [EXPORT_SENTINEL]);
}

#[test]
fn marks_an_empty_folder() {
    let tmp = tempfile::tempdir().unwrap();

    clean_previous_ir_output(tmp.path()).unwrap();

    assert_eq!(names(tmp.path()), [EXPORT_SENTINEL]);
}

#[test]
fn export_artifacts_are_recognised_by_name() {
    for name in [
        "a.csv",
        "a.csv.tmp",
        "a.meta.json",
        "a.meta.json.tmp",
        "a.json",
        "a.json.tmp",
        "a.jsonl",
        "a.jsonl.tmp",
    ] {
        assert!(is_export_artifact(name), "{name} is an export file");
    }
    for name in ["notes.txt", "photo.jpg", "other.xml", "a.csv.bak", ""] {
        assert!(!is_export_artifact(name), "{name} is not an export file");
    }
}

#[test]
fn marks_a_folder_that_holds_only_operating_system_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join(".DS_Store"), "finder").unwrap();

    clean_previous_ir_output(tmp.path()).unwrap();

    assert_eq!(names(tmp.path()), [".DS_Store", EXPORT_SENTINEL]);
}

/// The clean of earlier mail archives is reached only through
/// [`clean_previous_ir_output`], and refuses a directory without the
/// sentinel itself as well, so `.mbox` files and directories of `.eml`
/// files a person keeps there stay (#1531).
#[test]
fn the_mail_clean_refuses_a_directory_without_the_sentinel_and_removes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    fs::write(dir.join("+15555550101.mbox"), "From x\n").unwrap();
    fs::create_dir(dir.join("+15555550102")).unwrap();
    fs::write(dir.join("+15555550102/0001.eml"), "Subject: x\n").unwrap();

    assert!(clean_previous_mail_output(dir).is_err());

    assert_eq!(names(dir), ["+15555550101.mbox", "+15555550102"]);
    assert!(dir.join("+15555550102/0001.eml").is_file());
}

#[test]
fn the_mail_clean_removes_only_mail_archives() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    write_export_sentinel(dir).unwrap();
    fs::write(dir.join("+15555550101.mbox"), "From x\n").unwrap();
    fs::write(dir.join("Old.MBOX"), "From x\n").unwrap();
    fs::create_dir(dir.join("+15555550102")).unwrap();
    fs::write(dir.join("+15555550102/0001.eml"), "Subject: x\n").unwrap();
    // An email kept as an attachment is not a previous export.
    fs::create_dir(dir.join("attachments")).unwrap();
    fs::write(dir.join("attachments/forwarded.eml"), "Subject: x\n").unwrap();
    fs::create_dir(dir.join("photos")).unwrap();
    fs::write(dir.join("photos/a.jpg"), "jpg").unwrap();
    fs::write(dir.join("notes.txt"), "mine").unwrap();

    clean_previous_mail_output(dir).unwrap();

    assert_eq!(
        names(dir),
        [EXPORT_SENTINEL, "attachments", "notes.txt", "photos"]
    );
    assert!(dir.join("attachments/forwarded.eml").is_file());
    assert!(dir.join("photos/a.jpg").is_file());
}

/// An entry of a subdirectory that cannot be read fails the clean-up with
/// the directory named, rather than reading the directory as holding no
/// `.eml` and leaving an earlier export's directory beside the new one
/// (#1563).
#[test]
fn an_entry_that_cannot_be_read_fails_the_eml_check_and_names_the_directory() {
    let dir = Path::new("/exports/+15555550102");
    let entries = vec![
        Ok(dir.join("notes.txt")),
        Err(std::io::Error::other("stale file handle")),
        Ok(dir.join("0001.eml")),
    ];

    let error = holds_eml(dir, entries).unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("/exports/+15555550102"), "{message}");
    assert!(message.contains("stale file handle"), "{message}");
}

/// A subdirectory the mail clean cannot list fails it with the
/// subdirectory named.
#[cfg(unix)]
#[test]
fn a_subdirectory_that_cannot_be_read_fails_the_mail_clean_and_names_it() {
    let tmp = tempfile::tempdir().unwrap();
    write_export_sentinel(tmp.path()).unwrap();
    let sub = tmp.path().join("+15555550102");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("0001.eml"), "Subject: x\n").unwrap();
    let Some(result) =
        crate::with_directory_mode(&sub, 0o000, || clean_previous_mail_output(tmp.path()))
    else {
        return;
    };

    let message = format!("{:#}", result.unwrap_err());
    assert!(message.contains("+15555550102"), "{message}");
    assert!(sub.join("0001.eml").is_file());
}
