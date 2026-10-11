//! Shared scaffolding for the export crates' tests and the Import Run log
//! tests, such as the exporters' `convert_smoke` tests, the attachment
//! byte-total tests, and the tests that read an Import Run log back through
//! [`run_log_lines`] (behind `testutil`).

use crate::{
    ExportReport, ExporterConfig, IssueSink, ProgressEvent, ProgressSink, RunIssue, RunLogLevel,
    parse_run_log_line,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Give `config` an issue sink, and return the rows it receives, in the
/// order the run sent them.
pub fn collect_issues(config: &mut ExporterConfig) -> Arc<Mutex<Vec<RunIssue>>> {
    let issues = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&issues);
    config.issues = Some(IssueSink::new(move |issue| {
        sink.lock().unwrap().push(issue);
    }));
    issues
}

/// Every attachment count a test's progress sink received, as
/// `(done, bytes_done, bytes_total)`.
pub type AttachmentTotals = Arc<Mutex<Vec<(usize, u64, u64)>>>;

/// A progress sink, and every attachment count it receives as
/// `(done, bytes_done, bytes_total)`, in the order the run sent them: what a
/// test of the attachment byte total asserts on.
pub fn attachment_totals() -> (ProgressSink, AttachmentTotals) {
    let totals = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&totals);
    let progress = ProgressSink::unpaced(move |event| {
        if let ProgressEvent::Attachments {
            done,
            bytes_done,
            bytes_total,
            ..
        } = event
        {
            sink.lock().unwrap().push((done, bytes_done, bytes_total));
        }
    });
    (progress, totals)
}

/// Each line of the Import Run log at `path` as its level and text, in the
/// order the file holds them: what a test of a run log asserts on.
///
/// # Panics
///
/// Panics when `path` cannot be read, or when a line of it is not a run log's
/// line.
pub fn run_log_lines(path: &Path) -> Vec<(RunLogLevel, String)> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|raw| {
            let line =
                parse_run_log_line(0, raw).unwrap_or_else(|| panic!("not a run log line: {raw:?}"));
            (line.level, line.text)
        })
        .collect()
}

/// The names in `dir`, sorted, without the `.lock` files a scratch directory
/// keeps: what a test asserts is the data a directory holds.
///
/// # Panics
///
/// Panics when `dir` cannot be read.
pub fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != crate::scratch::LOCK)
        .collect();
    names.sort();
    names
}

/// Sorted `.csv` paths under `root` (the smoke-test file collection block).
pub fn csv_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("csv"))
        .collect();
    files.sort();
    files
}

/// Every data row of `path`, keyed by lower-cased column name.
///
/// This reads the export with `message_csv::open_csv_lowercase`, the CSV reader
/// the iMazing and OpenExtract exporters parse with, so a test asserts the
/// value in a named column rather than a substring of the file. The
/// distinction matters: a substring search over the
/// whole file is satisfied by the header line, so `contains("direction")`
/// passes whether or not a single message was written.
///
/// # Panics
///
/// Panics when the file cannot be read or is not CSV — a test asserting on a
/// missing export has already failed.
pub fn csv_rows(path: &Path) -> Vec<BTreeMap<String, String>> {
    let (mut rdr, headers) =
        message_csv::open_csv_lowercase(path).expect("the export must be readable CSV");
    rdr.records()
        .map(|rec| {
            let rec = rec.expect("a readable CSV row");
            headers
                .iter()
                .enumerate()
                .map(|(i, h)| (h.clone(), rec.get(i).unwrap_or("").trim().to_string()))
                .collect()
        })
        .collect()
}

/// Assert that a CSV export under `root` is complete: every file carries the
/// `contains` header columns and none of the `not_contains` ones, one of them
/// holds a data row whose named columns all hold the given values, and no
/// stray `.json` files remain.
///
/// `row` is the part that can fail on an empty export. Header columns are
/// written before the first message is, so a column assertion alone passes an
/// exporter that parsed nothing; pass the message body, direction and
/// timestamp the fixture is known to carry, and an exporter that wrote a
/// correct header and no messages — or the wrong body against the right
/// column — fails here.
///
/// Every file is checked rather than the alphabetically first, because one
/// export writes one unified header across its conversations and which
/// conversation sorts first is not part of the claim.
///
/// # Panics
///
/// Panics when there is no CSV, when a column is missing or unexpectedly
/// present, when a `.json` file was left behind, or when no row matches.
pub fn assert_csv_export(
    root: &Path,
    contains: &[&str],
    not_contains: &[&str],
    row: &[(&str, &str)],
) {
    assert!(
        !row.is_empty(),
        "assert_csv_export needs at least one column and value to check; \
         a header-only assertion cannot fail on an export with no messages"
    );
    let files = csv_files(root);
    assert!(!files.is_empty(), "expected at least one .csv");
    let json_count = fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("json")
                && !p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".meta.json"))
        })
        .count();
    assert_eq!(json_count, 0);
    for path in &files {
        let contents = fs::read_to_string(path).expect("read the export");
        let header = contents.lines().next().expect("a header line");
        for col in contains {
            assert!(
                header.contains(col),
                "header of {} missing {col:?}",
                path.display()
            );
        }
        for col in not_contains {
            assert!(
                !header.contains(col),
                "header of {} unexpectedly has {col:?}",
                path.display()
            );
        }
    }
    assert_csv_row_in_any(&files, row);
}

/// Assert that one of `paths` holds a data row whose named columns all hold the
/// given values.
///
/// The row names its own `chat_identifier`, so which file it landed in is not
/// part of the claim. Searching every file keeps the assertion about the export
/// rather than about the alphabetical order of its conversations, which a new
/// fixture can change.
///
/// # Panics
///
/// Panics when no file holds a matching row, naming what was found instead.
pub fn assert_csv_row_in_any(paths: &[PathBuf], expected: &[(&str, &str)]) {
    let mut seen = Vec::new();
    for path in paths {
        let rows = csv_rows(path);
        if rows.iter().any(|row| row_matches(row, expected)) {
            return;
        }
        seen.push((path.display().to_string(), rows));
    }
    panic!(
        "no row in the {} has {expected:?}; found {seen:#?}",
        crate::count_of(paths.len() as u64, "exported file", "exported files")
    );
}

/// True when every named column of `row` holds the wanted value.
fn row_matches(row: &BTreeMap<String, String>, expected: &[(&str, &str)]) -> bool {
    expected.iter().all(|(col, want)| {
        row.get(&col.to_ascii_lowercase())
            .is_some_and(|v| v == want)
    })
}

/// Assert that `path` holds at least one data row whose named columns all hold
/// the given values.
///
/// # Panics
///
/// Panics when no row matches, naming the rows that were there.
pub fn assert_csv_row(path: &Path, expected: &[(&str, &str)]) {
    let rows = csv_rows(path);
    assert!(
        !rows.is_empty(),
        "{} has a header but no messages",
        path.display()
    );
    let matched = rows.iter().any(|row| row_matches(row, expected));
    assert!(
        matched,
        "no row in {} has {:?}; rows were {:#?}",
        path.display(),
        expected,
        rows
    );
}

/// Sorted names of the `.jsonl` files directly under `dir`.
fn jsonl_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read output")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    names.sort();
    names
}

/// Run a JSONL export into `out` twice, the second time with `resume` set, and
/// assert that the second run resumed the first instead of rewriting it.
///
/// `run` calls the exporter's own entry point with `OutputFormat::Jsonl`, the
/// output directory `out`, and the `resume` flag it is given. The first run must
/// write at least one conversation, one `.jsonl` file per conversation, and
/// skip nothing. The second must skip every conversation the first wrote,
/// still count them all, and leave the same files with the same bytes.
///
/// The file bytes alone prove nothing: the writer is deterministic, so a
/// resumed run that quietly rewrote every conversation would produce the same
/// bytes. `conversations_skipped` is the only observable difference between
/// resuming and starting over, so that is the assertion that matters.
///
/// Returns the first run's report, for assertions about the fixture's own
/// conversation count.
///
/// # Panics
///
/// Panics when either run fails or any of the above does not hold.
pub fn assert_jsonl_resumes(
    out: &Path,
    run: impl Fn(bool) -> anyhow::Result<ExportReport>,
) -> ExportReport {
    let report = run(false).expect("convert");
    assert!(
        report.conversations >= 1,
        "the fixture must produce at least one conversation"
    );
    assert_eq!(
        report.conversations_skipped, 0,
        "a first run into an empty directory skips nothing"
    );

    let first = jsonl_names(out);
    assert_eq!(
        first.len() as u64,
        report.conversations,
        "the queue wrote a file per conversation: {first:?}"
    );
    let bodies: Vec<String> = first
        .iter()
        .map(|n| fs::read_to_string(out.join(n)).expect("read jsonl"))
        .collect();

    let resumed = run(true).expect("resume convert");
    assert_eq!(
        resumed.conversations, report.conversations,
        "a resumed run still counts every conversation"
    );
    assert_eq!(
        resumed.conversations_skipped, report.conversations,
        "a resumed run must skip every conversation the first run wrote"
    );

    assert_eq!(jsonl_names(out), first, "same file set after a resume");
    for (name, before) in first.iter().zip(bodies) {
        assert_eq!(
            fs::read_to_string(out.join(name)).expect("reread"),
            before,
            "a resumed run must not rewrite {name}"
        );
    }
    report
}

/// One Scratch Directory for every test in the process's temporary directory. A
/// run's scratch directory under it is deleted when the run ends, and two
/// runs at once each lock their own, so tests can share it.
pub fn test_scratch_dir() -> PathBuf {
    std::env::temp_dir().join("message-crate-test-cache")
}

/// The config an exporter's `run()` test passes: read `inputs`, write JSONL
/// into `output`, copy no attachments and keep every name, so the result
/// shows only what the exporter itself did. Scratch data goes under
/// [`test_scratch_dir`].
pub fn jsonl_run_config(
    inputs: &[&Path],
    output: &Path,
    source: crate::SourceConfig,
) -> crate::ExporterConfig {
    crate::ExporterConfig {
        inputs: inputs.iter().map(|p| p.to_path_buf()).collect(),
        output: output.to_path_buf(),
        scratch_dir: test_scratch_dir(),
        timezone: None,
        obfuscate: crate::ObfuscateConfig {
            enabled: false,
            seed: None,
        },
        media: crate::MediaConfig {
            mode: media::MediaMode::Disabled,
            compress: media::CompressOptions::default(),
        },
        cancel: None,
        log: None,
        progress: None,
        issues: None,
        output_format: crate::OutputFormat::Jsonl,
        resume: false,
        source,
    }
}

/// Assert that a `run()` into `output` wrote `conversations` JSONL files and
/// opened its summary with where the export went. Returns every file's text,
/// joined, for assertions about what the files carry.
///
/// # Panics
///
/// Panics when the file count or the summary's first line is wrong.
pub fn assert_run_wrote_jsonl(
    result: &crate::RunResult,
    output: &Path,
    conversations: usize,
) -> String {
    let names = jsonl_names(output);
    assert_eq!(
        names.len(),
        conversations,
        "one JSONL file per conversation: {names:?}"
    );
    assert_eq!(
        result.messages.first().map(String::as_str),
        Some(format!("Wrote jsonl export under {}", output.display()).as_str()),
        "the summary opens with where the export went: {:?}",
        result.messages
    );
    names
        .iter()
        .map(|n| fs::read_to_string(output.join(n)).expect("read jsonl"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The backup date the exporters' tests give a source: 2026-09-30T18:45:12Z,
/// in Unix milliseconds. A date of the test's own, so a test that reads it
/// back cannot pass by reading the time it ran at.
pub const TEST_BACKUP_TAKEN_AT_UNIX_MS: i64 = 1_790_793_912_000;

/// Set `path`'s modification time to `unix_ms`, the way a backup file keeps
/// the time it was written.
///
/// # Panics
///
/// Panics when the file cannot be opened or its time cannot be set.
pub fn set_modified_unix_ms(path: &Path, unix_ms: i64) {
    let at = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(unix_ms).expect("a time after 1970"));
    fs::File::options()
        .write(true)
        .open(path)
        .expect("open the file to date")
        .set_modified(at)
        .expect("set the file's modification time");
}

/// Every JSON Lines file's `export.backup_taken_at_unix_ms` under `dir`, in
/// file name order.
///
/// # Panics
///
/// Panics when a file cannot be read or its first line is not JSON.
pub fn jsonl_backup_dates(dir: &Path) -> Vec<Option<i64>> {
    jsonl_names(dir)
        .iter()
        .map(|name| {
            let text = fs::read_to_string(dir.join(name)).expect("read jsonl");
            let header: serde_json::Value =
                serde_json::from_str(text.lines().next().expect("a header line"))
                    .expect("the header is JSON");
            header["export"]["backup_taken_at_unix_ms"].as_i64()
        })
        .collect()
}

/// How [`with_directory_mode`] checks whether a mode keeps the running user
/// out of a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryProbe {
    /// List the directory: for a test of reading it.
    List,
    /// Write a file into the directory: for a test of writing into it.
    Write,
}

/// Run `f` with the permissions of `directory` set to `mode`, then set them
/// back to `0o755` before returning, so the temporary directory can still be
/// removed.
///
/// `None`, with a line on stderr, when `probe` still succeeds on `directory`
/// under `mode`: a user such as root gets past the permissions, cannot
/// exercise the failure, and the test has nothing to check.
///
/// # Panics
///
/// Panics when the directory's mode cannot be set or restored.
#[cfg(unix)]
pub fn with_directory_mode<T>(
    directory: &Path,
    mode: u32,
    probe: DirectoryProbe,
    f: impl FnOnce() -> T,
) -> Option<T> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(directory, fs::Permissions::from_mode(mode))
        .expect("set the directory's mode");
    let (got_past, verb) = match probe {
        DirectoryProbe::List => (fs::read_dir(directory).is_ok(), "listed"),
        DirectoryProbe::Write => {
            let file = directory.join("probe");
            let written = fs::write(&file, b"").is_ok();
            if written {
                let _ = fs::remove_file(&file);
            }
            (written, "written")
        }
    };
    let result = if got_past {
        eprintln!(
            "skipped: {} can still be {verb} with mode {mode:o}",
            directory.display()
        );
        None
    } else {
        Some(f())
    };
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))
        .expect("restore the directory's mode");
    result
}
