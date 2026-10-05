use std::path::{Path, PathBuf};

use message_crate_core::{
    ExportReport, ExportTransforms, ExporterConfig, FormatConfig, MediaConfig, ObfuscateConfig,
    OutputFormat, SourceConfig,
};
use message_ir_format::{EXPORT_SENTINEL, FormatSink};

use super::{CONVERTING, EXPORT_DIRECTORY_NAME, ExportDirectories, ExportKind, PULLED};

/// An Export Directory in its own temporary app-data directory.
fn exports() -> (tempfile::TempDir, ExportDirectories) {
    let app_data = tempfile::tempdir().unwrap();
    let exports = ExportDirectories::in_app_data(app_data.path());
    (app_data, exports)
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Write one conversation into `dir` as JSON Lines, as a pull does.
fn pull_into(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    message_ir_format::mark_export_directory(dir).unwrap();
    let mut sink = FormatSink::open(dir, OutputFormat::Jsonl, ExportTransforms::none()).unwrap();
    sink.write_document(message_ir::testutil::sample_document("hello export"))
        .unwrap();
    sink.finish(&mut ExportReport::default()).unwrap();
    std::fs::write(dir.join(message_crate_pull::PULL_JOURNAL_NAME), "{}\n").unwrap();
}

/// Convert `input` into `output` as CSV, as Export's format step does.
fn convert(input: &Path, output: &Path, cache: &Path) {
    message_reexport::run(&ExporterConfig {
        inputs: vec![input.to_path_buf()],
        output: output.to_path_buf(),
        cache_dir: cache.to_path_buf(),
        timezone: None,
        obfuscate: ObfuscateConfig::default(),
        media: MediaConfig::default(),
        cancel: None,
        log: None,
        progress: None,
        issues: None,
        output_format: OutputFormat::Csv,
        resume: false,
        source: SourceConfig::Format(FormatConfig::default()),
    })
    .unwrap();
}

#[test]
fn an_export_writes_its_result_to_its_own_directory_and_leaves_no_in_between_files() {
    let (app_data, exports) = exports();
    let made = exports
        .create(ExportKind::Export, "csv", "2026-10-04-1430", None)
        .unwrap();
    let dir = PathBuf::from(&made.dir);
    assert_eq!(
        dir,
        app_data
            .path()
            .canonicalize()
            .unwrap()
            .join(EXPORT_DIRECTORY_NAME)
            .join("export-2026-10-04-1430-csv")
    );

    pull_into(Path::new(&made.pulled));
    convert(
        Path::new(&made.pulled),
        Path::new(&made.converting),
        &app_data.path().join("scratch"),
    );
    let finished = exports.finish(&made.dir).unwrap();

    assert_eq!(finished.as_deref(), Some(dir.as_path()));
    let left = names(&dir);
    assert!(
        !left.iter().any(|name| name == PULLED
            || name == CONVERTING
            || name == message_crate_pull::PULL_JOURNAL_NAME
            || name.ends_with(".jsonl")),
        "only the result is left: {left:?}"
    );
    assert!(
        left.iter().any(|name| name.ends_with(".csv")),
        "the CSV result is in the export's directory: {left:?}"
    );
    assert!(left.iter().any(|name| name == EXPORT_SENTINEL), "{left:?}");
}

#[test]
fn a_json_lines_export_keeps_its_files_and_drops_the_pull_journal() {
    let (_app_data, exports) = exports();
    let made = exports
        .create(ExportKind::Export, "jsonl", "2026-10-04-1430", None)
        .unwrap();
    pull_into(Path::new(&made.dir));

    exports.finish(&made.dir).unwrap();

    let left = names(Path::new(&made.dir));
    assert!(left.iter().any(|name| name.ends_with(".jsonl")), "{left:?}");
    assert!(
        !left
            .iter()
            .any(|name| name == message_crate_pull::PULL_JOURNAL_NAME),
        "{left:?}"
    );
}

#[test]
fn an_export_to_another_destination_leaves_no_directory_behind() {
    let (app_data, exports) = exports();
    let made = exports
        .create(ExportKind::Export, "csv", "2026-10-04-1430", None)
        .unwrap();
    let chosen = app_data.path().join("chosen");
    pull_into(Path::new(&made.pulled));
    convert(
        Path::new(&made.pulled),
        &chosen,
        &app_data.path().join("scratch"),
    );

    assert_eq!(exports.finish(&made.dir).unwrap(), None);

    assert!(!Path::new(&made.dir).exists());
    assert!(names(&chosen).iter().any(|name| name.ends_with(".csv")));
}

#[test]
fn two_exports_in_one_minute_get_two_directories() {
    let (_app_data, exports) = exports();
    let first = exports
        .create(ExportKind::Export, "mbox", "2026-10-04-1430", None)
        .unwrap();
    let second = exports
        .create(ExportKind::Export, "mbox", "2026-10-04-1430", None)
        .unwrap();

    assert_ne!(first.dir, second.dir);
    assert!(
        second.dir.ends_with("export-2026-10-04-1430-mbox-2"),
        "{}",
        second.dir
    );
}

#[test]
fn a_failed_export_s_directory_is_deleted_whole() {
    let (_app_data, exports) = exports();
    let made = exports
        .create(ExportKind::Convert, "eml", "2026-10-04-1430", None)
        .unwrap();
    std::fs::write(Path::new(&made.dir).join("half.eml"), "partial").unwrap();

    exports.discard(&made.dir).unwrap();

    assert!(!Path::new(&made.dir).exists());
    exports.discard(&made.dir).unwrap();
}

#[test]
fn only_an_export_s_or_convert_s_directory_is_finished_or_discarded() {
    let (app_data, exports) = exports();
    exports
        .create(ExportKind::Export, "csv", "2026-10-04-1430", None)
        .unwrap();
    let mine = app_data.path().join("my-messages");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("keep.txt"), "mine").unwrap();
    let elsewhere = exports.root().join("photos");
    std::fs::create_dir_all(&elsewhere).unwrap();

    for dir in [&mine, &elsewhere, &exports.root().to_path_buf()] {
        let dir = dir.display().to_string();
        assert!(exports.discard(&dir).is_err(), "{dir}");
        assert!(exports.finish(&dir).is_err(), "{dir}");
    }
    assert!(mine.join("keep.txt").is_file());
    assert!(elsewhere.is_dir());
}

#[test]
fn a_format_that_cannot_name_a_directory_is_refused() {
    let (_app_data, exports) = exports();
    assert!(
        exports
            .create(ExportKind::Export, "../csv", "2026-10-04-1430", None)
            .is_err()
    );
    assert!(
        exports
            .create(ExportKind::Export, "", "2026-10-04-1430", None)
            .is_err()
    );
}

#[test]
fn a_destination_that_holds_the_export_directory_is_refused_before_anything_is_made() {
    let (app_data, exports) = exports();
    for chosen in [app_data.path().to_path_buf(), exports.root().to_path_buf()] {
        std::fs::create_dir_all(&chosen).unwrap();
        let err = exports
            .create(
                ExportKind::Export,
                "csv",
                "2026-10-04-1430",
                Some(chosen.to_str().unwrap()),
            )
            .unwrap_err();
        assert!(err.contains("holds the Export Directory"), "{err}");
    }
    assert!(names(exports.root()).is_empty());

    let inside = exports.root().join("mine");
    std::fs::create_dir_all(&inside).unwrap();
    assert!(
        exports
            .create(
                ExportKind::Export,
                "csv",
                "2026-10-04-1430",
                Some(inside.to_str().unwrap())
            )
            .is_ok()
    );
}

#[test]
fn the_start_up_sweep_deletes_an_interrupted_export_and_keeps_a_running_one() {
    let (app_data, exports) = exports();
    let interrupted = exports
        .create(ExportKind::Export, "csv", "2026-10-04-1430", None)
        .unwrap();
    pull_into(Path::new(&interrupted.pulled));
    let finished = exports
        .create(ExportKind::Export, "jsonl", "2026-10-04-1430", None)
        .unwrap();
    pull_into(Path::new(&finished.dir));
    exports.finish(&finished.dir).unwrap();
    // The app quits mid-export: its hold on the marker ends with it.
    drop(exports);

    let next_start = ExportDirectories::in_app_data(app_data.path());
    let running = next_start
        .create(ExportKind::Convert, "csv", "2026-10-04-1431", None)
        .unwrap();
    // Another app process, started at the same time, sweeps too. A process
    // another test forks holds a copy of every open marker until it runs its
    // program, so the sweep is repeated for a while rather than once.
    let sweeper = ExportDirectories::in_app_data(app_data.path());
    for _ in 0..200 {
        sweeper.sweep();
        if !Path::new(&interrupted.dir).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(
        !Path::new(&interrupted.dir).exists(),
        "the copy of the messages is gone"
    );
    assert!(
        Path::new(&finished.dir).is_dir(),
        "a finished export is kept"
    );
    assert!(Path::new(&running.dir).is_dir(), "a run under way is kept");
    assert!(
        !names(next_start.root())
            .iter()
            .any(|name| name.starts_with("export-") && name.ends_with(".running")),
        "{:?}",
        names(next_start.root())
    );
}
