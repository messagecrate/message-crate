//! What a Demo Account build says as it goes: through [`Progress`], on
//! standard output for `reset-demo` and in the server's log under `serve`,
//! and on standard error when a reset leaves a backup behind. The one-line
//! heading that opens each step stays beside the step.

use std::path::Path;

use super::build::{DEMO_IMPORT_SOURCES, PreparedBundle};
use crate::counts::words;
use crate::db::address_book::LoadCounts;
use crate::process_assets;
use crate::progress::Progress;

/// Say, through `progress`, which files every demo build is built from.
pub(super) fn say_reset_header(
    account_id: i64,
    prepared: &PreparedBundle,
    db: &dyn std::fmt::Display,
    progress: Progress,
) {
    progress.heading("The Demo Account is built from these files");
    progress.say(format_args!("account:      {account_id}"));
    for source in &DEMO_IMPORT_SOURCES {
        progress.say(format_args!(
            "{:<14}{}",
            format!("{}:", source.label),
            (source.export_dir)(prepared).display()
        ));
    }
    progress.say(format_args!("database:     {db}"));
}

/// The line printed when processing the demo originals could not do everything,
/// naming each count of what it could not do, or `None` when it did everything.
/// `reset-demo`, a new database's seeding and the administrator's rebuild all
/// print it, so it names none of them.
pub(super) fn not_done_line(stats: &process_assets::ProcessAssetsStats) -> Option<String> {
    stats
        .failures()
        .map(|failures| format!("The Demo Account has {failures}. Its build goes on all the same"))
}

/// Say each warning in `stats` through `progress`. Kept apart from
/// [`super::build::generate_demo_data`] so a test can hand it a warning: the
/// generator returns one only when the file system refuses to remove a
/// directory.
pub(super) fn say_generator_warnings(stats: &demo_seed::GenStats, progress: Progress) {
    for warning in &stats.warnings {
        progress.warn(warning);
    }
}

/// Say, through `progress`, what loading the demo address book changed.
pub(super) fn say_address_book_loaded(counts: &LoadCounts, progress: Progress) {
    progress.say(format_args!(
        "The demo address book named {}: {} named in place and {} new. It moved {} and added {}",
        words(counts.contacts_changed(), "1 contact", "{n} contacts"),
        words(counts.contacts_updated, "1 Unknown", "{n} Unknowns"),
        counts.contacts_created,
        words(
            counts.identities_moved,
            "1 identity from an Unknown",
            "{n} identities from Unknowns"
        ),
        words(counts.identities_added, "1 identity", "{n} identities")
    ));
}

/// Say on standard error that the swap of `what` is done but its backup at
/// `backup` could not be removed. A warning, not a failure: the state is
/// right, only a copy is left behind.
pub(super) fn backup_not_removed(what: &str, backup: &Path, error: &anyhow::Error) {
    eprintln!(
        "The {what} is in place, but its backup at {} could not be removed: {error:#}",
        backup.display()
    );
}
