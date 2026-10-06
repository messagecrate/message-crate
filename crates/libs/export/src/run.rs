//! Page exported messages, fetch their Assets, and write JSON Lines directories.
//!
//! JSON Lines means one JSON object per line. Message Crate is the HTTP server
//! that stores imported messages.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use message_crate_core::{CancelFlag, check_cancel, count_of, parallel_for_each};
use message_crate_http::{auth_check as authenticate, with_retries};
use message_ir_format::mark_export_directory;
use serde::Serialize;

use crate::http::{CloseAction, ExportMessagesArgs, HttpSession};
use crate::journal::{self, ExportJournalEvent, ExportJournalState, ServerTarget};
use crate::part_file::write_asset;
use crate::project::{
    ExportPath, build_document, conversation_key, export_path, newest_backup_taken_at,
    to_ir_message,
};
use message_crate_api_types::{ExportQueryList, ExportRun, ExportScope, Message};

/// Page size for `GET /v1/exports/{id}/messages`; the server's maximum.
pub const DEFAULT_PAGE_LIMIT: usize = 500;
/// The largest page the server will hand back for `GET /v1/exports/{id}/messages`.
pub const MAX_PAGE_LIMIT: usize = 500;
/// The `tool` every run this crate creates is recorded under.
pub const TOOL_NAME: &str = "message-crate-export";
/// Default number of workers that fetch Assets in parallel.
pub const DEFAULT_ASSET_FETCH_WORKERS: usize = 8;
/// Extra tries for transient HTTP failures, matching the message-crate-push default.
const MAX_RETRIES: u32 = 3;

/// Settings for one Export Run (output directory, URL, search, flags).
#[derive(Debug, Clone)]
pub struct ExportConfig {
    /// Directory the JSON Lines files and attachments are written into.
    pub out_dir: PathBuf,
    /// Server base URL, e.g. `http://127.0.0.1:8080`.
    pub base_url: String,
    /// The logged-in Session's token. The desktop app never passes an API
    /// Token.
    pub token: String,
    /// A query in the server's search language. Blank asks for everything the
    /// account holds; anything else is the run's `query` scope.
    pub query: String,
    /// The list `query` is for: the messages it matches on the Messages list,
    /// or every message of the conversations it shows on the Conversations
    /// list. Unused when `query` is blank.
    pub list: ExportQueryList,
    /// Write messages only; fetch no Assets.
    pub skip_attachments: bool,
    /// Messages per `GET /v1/exports/{id}/messages` page, clamped to
    /// `1..=MAX_PAGE_LIMIT`.
    pub page_limit: usize,
    /// Checked between pages and Asset fetches; set it to stop the run early.
    pub cancel: Option<CancelFlag>,
    /// Number of workers that fetch Assets in parallel (default 8).
    pub asset_fetch_workers: usize,
}

/// Final summary of an Export Run: conversations, messages, and the Assets
/// fetched and kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportReport {
    /// Account id the token resolved to.
    pub account: i64,
    /// The Export Run the server recorded for this export.
    pub export_id: i64,
    /// The query the run asked the server for.
    pub query: String,
    /// Conversations written.
    pub conversations: u64,
    /// Messages written.
    pub messages: u64,
    /// Assets fetched this run.
    pub assets_fetched: u64,
    /// Assets already on disk, kept without fetching them.
    pub assets_kept: u64,
    /// Attachment paths the server sent that would leave the output directory,
    /// as the server sent them. Each such attachment is written at
    /// `attachments/{sha256}` instead.
    pub refused_attachment_paths: Vec<String>,
    /// The output directory, as given.
    pub out_dir: String,
}

/// Live progress sent to the desktop app during an Export Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// One line for the log panel.
    Log(String),
    /// The token was accepted; the run knows which account it is reading.
    Auth {
        /// Account id the token resolved to.
        account_id: i64,
        /// Username the server reports for that account.
        username: String,
    },
    /// One page of messages arrived.
    Page {
        /// Messages on this page.
        messages: usize,
        /// Messages fetched so far, this page included.
        total_so_far: u64,
    },
    /// The run finished; the report is final.
    Done(ExportReport),
}

/// Callback type for live progress (desktop log panel, tests).
pub type ProgressFn<'a> = dyn FnMut(ProgressEvent) + 'a;

/// Send one event to the caller's progress callback when it supplied one.
fn emit(on_progress: &mut Option<&mut ProgressFn<'_>>, event: ProgressEvent) {
    if let Some(callback) = on_progress.as_mut() {
        callback(event);
    }
}

/// Where the next page starts, or `None` when the walk is over.
///
/// A run's pages are fixed ranges of the list it made at creation, and a
/// message deleted since leaves its place empty, so a page can hold fewer
/// items than `limit`, or none, before the end. The walk steps by the
/// `limit` it asked for and ends at the run's `total`, never on a short or
/// empty page.
fn next_offset(offset: usize, limit: usize, total: u64) -> Option<usize> {
    if limit == 0 {
        return None;
    }
    let next = offset.saturating_add(limit);
    (u64::try_from(next).unwrap_or(u64::MAX) < total).then_some(next)
}

/// Create the output directory, mark it as a Message Crate export, and create its
/// `attachments/` child.
///
/// The sentinel names this directory as one an export wrote. The desktop app
/// refuses to act on a run directory without it
/// (`RunDirectories::directory` in `src-tauri/src/run_directories.rs`), which is
/// part of what stands between a path bug and a recursive delete somewhere
/// else on disk. An exported directory that skipped the sentinel could not be used
/// as export staging. Because a marked directory may be cleaned by a later
/// export, a directory of the person's own files is refused rather than marked
/// ([`mark_export_directory`]). The directory is marked before `attachments/` is
/// created, so a new directory is still empty when it is checked.
///
/// # Errors
///
/// Returns an error when the directory, its `attachments/` child, or the
/// sentinel cannot be written, or the directory holds files and no sentinel.
fn prepare_out_dir(out_dir: &Path, skip_attachments: bool) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    mark_export_directory(out_dir)?;
    if !skip_attachments {
        let attachments_dir = out_dir.join("attachments");
        fs::create_dir_all(&attachments_dir)
            .with_context(|| format!("create {}", attachments_dir.display()))?;
    }
    Ok(())
}

/// Export the matching messages into `cfg.out_dir` as JSON Lines plus attachments.
///
/// JSON Lines means one JSON object per line. A local journal
/// (`.message-crate-export-state.jsonl`) records which Assets were already fetched so a
/// later run can skip them.
///
/// # Errors
///
/// Returns an error when the session token or output directory is missing, login fails, a
/// page or Asset fetch fails, or a conversation file cannot be written.
pub fn run(
    cfg: &ExportConfig,
    mut on_progress: Option<&mut ProgressFn<'_>>,
) -> Result<ExportReport> {
    if cfg.token.trim().is_empty() {
        bail!("session token is required");
    }
    if cfg.out_dir.as_os_str().is_empty() {
        bail!("output directory is required");
    }
    let exporter = Export::login(cfg, &mut on_progress)?;
    prepare_out_dir(&cfg.out_dir, cfg.skip_attachments)?;
    if exporter.journal.export_complete {
        emit(
            &mut on_progress,
            ProgressEvent::Log(
                "The previous Export Run finished. Checking for new messages…".into(),
            ),
        );
    }

    // Nothing is recorded for a run the caller already gave up on.
    check_cancel(cfg.cancel.as_ref())?;
    let export = exporter.start_export(&mut on_progress)?;
    let outcome = exporter.export_into_directory(&export, &mut on_progress);
    // The client closes the run either way, so the server's record says how
    // it ended. A close that fails after the files are written is a warning,
    // not a failed export: the directory is complete, only the record is not.
    let action = if outcome.is_ok() {
        CloseAction::Complete
    } else {
        CloseAction::Cancel
    };
    if let Err(error) = exporter.close_export(export.id, action) {
        // A refused completion follows a run whose files are all written; a
        // refused cancellation follows the failure the run returns below.
        let line = match action {
            CloseAction::Complete => format!("{error:#}. The Export wrote every file all the same"),
            CloseAction::Cancel => format!("{error:#}"),
        };
        emit(&mut on_progress, ProgressEvent::Log(line));
    }
    let Written {
        conversations,
        messages,
        assets,
        refused_paths,
    } = outcome?;

    let report = ExportReport {
        account: exporter.account,
        export_id: export.id,
        query: exporter.query,
        conversations,
        messages,
        assets_fetched: assets.fetched,
        assets_kept: assets.kept,
        refused_attachment_paths: refused_paths.into_iter().collect(),
        out_dir: cfg.out_dir.display().to_string(),
    };
    emit(
        &mut on_progress,
        ProgressEvent::Log(format!(
            "Wrote {} and {} to {}",
            count_of(report.conversations, "conversation", "conversations"),
            count_of(report.messages, "message", "messages"),
            report.out_dir
        )),
    );
    emit(&mut on_progress, ProgressEvent::Done(report.clone()));
    Ok(report)
}

/// Everything the server handed back while paging: messages grouped by
/// conversation, the attachments they reference, and the running count.
struct Fetched {
    /// Conversation key → (first message as the metadata seed, converted messages).
    by_conv: BTreeMap<String, (Message, Vec<message_ir::IrMessage>)>,
    /// sha256 → relative path under the output directory.
    assets: HashMap<String, String>,
    /// sha256 → every other path a message names for the same bytes. The
    /// Asset is fetched once, at the path in `assets`, then placed at each
    /// of these.
    other_paths: BTreeMap<String, BTreeSet<String>>,
    /// Attachment paths the server sent that would leave the output directory.
    refused_paths: BTreeSet<String>,
    total_messages: u64,
}

/// How many Assets this run fetched and kept, for the report.
#[derive(Debug, Default, Clone, Copy)]
struct AssetCounts {
    fetched: u64,
    kept: u64,
}

/// What one run left on disk.
struct Written {
    conversations: u64,
    messages: u64,
    assets: AssetCounts,
    refused_paths: BTreeSet<String>,
}

/// One authenticated Export Run: the connection, the account it resolved
/// to, and the local journal of files already on disk.
struct Export<'a> {
    cfg: &'a ExportConfig,
    session: HttpSession,
    account: i64,
    /// The server and account this Export Run's journal lines belong to.
    target: ServerTarget,
    /// The search query with surrounding whitespace removed.
    query: String,
    journal_path: PathBuf,
    journal: ExportJournalState,
}

impl<'a> Export<'a> {
    /// Check the token, announce the account and query, and load the journal.
    ///
    /// # Errors
    ///
    /// Returns an error when login fails or the journal cannot be read.
    fn login(cfg: &'a ExportConfig, out: &mut Option<&mut ProgressFn<'_>>) -> Result<Self> {
        let auth = authenticate(&cfg.base_url, &cfg.token).map_err(|e| anyhow::anyhow!("{e}"))?;
        let account = auth.account_id;
        let username = auth.username;
        emit(
            out,
            ProgressEvent::Auth {
                account_id: account,
                username: username.clone(),
            },
        );
        emit(
            out,
            ProgressEvent::Log(format!("Authenticated as {username} ({account})")),
        );
        let query = cfg.query.trim().to_string();
        emit(
            out,
            ProgressEvent::Log(if query.is_empty() {
                "Exporting every message".into()
            } else {
                match cfg.list {
                    ExportQueryList::Messages => {
                        format!("Exporting the messages that match: {query}")
                    }
                    ExportQueryList::Conversations => {
                        format!("Exporting every message of the conversations that match: {query}")
                    }
                }
            }),
        );
        // Load the journal so a later Export Run does not fetch the Assets
        // already on disk again.
        let journal_path = journal::journal_path(&cfg.out_dir);
        let target = ServerTarget::new(&cfg.base_url, username);
        let journal = journal::load(&journal_path, &target, &mut |line| {
            emit(out, ProgressEvent::Log(line));
        })?;
        Ok(Self {
            cfg,
            session: HttpSession::new()?,
            account,
            target,
            query,
            journal_path,
            journal,
        })
    }

    /// The scope this Export Run asks the server for: the trimmed query for its
    /// list, or everything when it is blank.
    fn scope(&self) -> ExportScope {
        if self.query.is_empty() {
            ExportScope::Everything
        } else {
            ExportScope::Query {
                list: self.cfg.list,
                q: self.query.clone(),
            }
        }
    }

    /// `POST /v1/exports` for this Export Run's scope, announcing what the server
    /// counted for it.
    ///
    /// # Errors
    ///
    /// Returns an error when the server refuses the scope or the request fails.
    fn start_export(&self, out: &mut Option<&mut ProgressFn<'_>>) -> Result<ExportRun> {
        let cfg = self.cfg;
        let export = with_retries(MAX_RETRIES, || {
            crate::http::create_export(
                &self.session,
                &cfg.base_url,
                &cfg.token,
                &self.scope(),
                TOOL_NAME,
            )
        })?;
        // The server's counts are never negative; a negative one reads as 0.
        let count = |n: i64| u64::try_from(n).unwrap_or(0);
        emit(
            out,
            ProgressEvent::Log(format!(
                "Export Run {} holds {} in {}, with {} ({})",
                export.id,
                count_of(count(export.message_count), "message", "messages"),
                count_of(
                    count(export.conversation_count),
                    "conversation",
                    "conversations"
                ),
                count_of(count(export.attachment_count), "attachment", "attachments"),
                media::format_bytes(count(export.total_bytes)),
            )),
        );
        Ok(export)
    }

    /// Close the run by `action`, retrying a transient failure.
    fn close_export(&self, export_id: i64, action: CloseAction) -> Result<ExportRun> {
        let cfg = self.cfg;
        with_retries(MAX_RETRIES, || {
            crate::http::close_export(&self.session, &cfg.base_url, &cfg.token, export_id, action)
        })
    }

    /// Page the run's messages, fetch their Assets, and write the
    /// conversation files; the journal records the result.
    ///
    /// # Errors
    ///
    /// Returns an error when a page or Asset fetch fails after retries, a
    /// message cannot be converted, a file cannot be written, or the run is
    /// cancelled.
    fn export_into_directory(
        &self,
        export: &ExportRun,
        out: &mut Option<&mut ProgressFn<'_>>,
    ) -> Result<Written> {
        let fetched = self.fetch_all_messages(export, out)?;
        let assets = if self.cfg.skip_attachments {
            AssetCounts::default()
        } else {
            let counts = self.fetch_assets(&fetched.assets, out)?;
            place_other_paths(&self.cfg.out_dir, &fetched.assets, &fetched.other_paths)?;
            counts
        };
        let conversations = self.write_conversations(fetched.by_conv)?;
        self.finish_journal(
            out,
            export.id,
            conversations,
            fetched.total_messages,
            &assets,
            fetched.assets,
        );
        Ok(Written {
            conversations,
            messages: fetched.total_messages,
            assets,
            refused_paths: fetched.refused_paths,
        })
    }

    /// Page through every message of the run, grouping by conversation and
    /// noting which attachments they reference.
    ///
    /// # Errors
    ///
    /// Returns an error when a page fails after retries, a message cannot be
    /// converted, or the run is cancelled.
    fn fetch_all_messages(
        &self,
        export: &ExportRun,
        out: &mut Option<&mut ProgressFn<'_>>,
    ) -> Result<Fetched> {
        let cfg = self.cfg;
        let mut fetched = Fetched {
            by_conv: BTreeMap::new(),
            assets: HashMap::new(),
            other_paths: BTreeMap::new(),
            refused_paths: BTreeSet::new(),
            total_messages: 0,
        };
        let limit = cfg.page_limit.clamp(1, MAX_PAGE_LIMIT);
        let mut offset = 0usize;
        loop {
            check_cancel(cfg.cancel.as_ref())?;
            let page = with_retries(MAX_RETRIES, || {
                crate::http::export_messages(
                    &self.session,
                    ExportMessagesArgs {
                        base_url: &cfg.base_url,
                        token: &cfg.token,
                        export_id: export.id,
                        limit,
                        offset,
                    },
                )
            })?;
            let count = page.items.len();
            fetched.total_messages += count as u64;
            emit(
                out,
                ProgressEvent::Page {
                    messages: count,
                    total_so_far: fetched.total_messages,
                },
            );
            for msg in page.items {
                if !cfg.skip_attachments {
                    for (path, rel) in
                        note_asset_refs(&msg, &mut fetched.assets, &mut fetched.other_paths)
                    {
                        let line = refused_path_line(&path, rel.as_deref());
                        if fetched.refused_paths.insert(path) {
                            emit(out, ProgressEvent::Log(line));
                        }
                    }
                }
                let ir = to_ir_message(&msg, cfg.skip_attachments)?;
                let (seed, messages) = fetched
                    .by_conv
                    .entry(conversation_key(&msg))
                    // Keep first message as seed for conversation metadata.
                    .or_insert_with(|| (msg.clone(), Vec::new()));
                newest_backup_taken_at(seed, &msg);
                messages.push(ir);
            }
            match next_offset(offset, limit, page.total) {
                Some(next) => offset = next,
                None => break,
            }
        }
        Ok(fetched)
    }

    /// Fetch every referenced Asset whose file is not already at its path,
    /// and journal each one now on disk that the journal does not list.
    ///
    /// A file at its path is kept whether or not the journal lists it, so the
    /// "Fetching" line counts as already on disk the same Assets the
    /// "Fetched" line counts as kept.
    ///
    /// # Errors
    ///
    /// Returns an error when a fetch fails after retries or the run is cancelled.
    fn fetch_assets(
        &self,
        assets: &HashMap<String, String>,
        out: &mut Option<&mut ProgressFn<'_>>,
    ) -> Result<AssetCounts> {
        let cfg = self.cfg;
        let to_fetch = assets_to_fetch(assets, &cfg.out_dir);
        let kept = assets.len() as u64 - to_fetch.len() as u64;
        let fetched = if to_fetch.is_empty() {
            0
        } else {
            emit(
                out,
                ProgressEvent::Log(format!(
                    "Fetching {} with {} ({} already on disk)…",
                    count_of(to_fetch.len() as u64, "Asset", "Assets"),
                    count_of(cfg.asset_fetch_workers as u64, "worker", "workers"),
                    kept
                )),
            );
            let stats = fetch_assets_parallel(FetchAssetsParallelArgs {
                session: &self.session,
                base_url: &cfg.base_url,
                token: &cfg.token,
                assets: &to_fetch,
                out_dir: &cfg.out_dir,
                workers: cfg.asset_fetch_workers,
                cancel: cfg.cancel.as_ref(),
            })?;
            emit(
                out,
                ProgressEvent::Log(format!(
                    "Fetched {} ({}) and kept {} already on disk",
                    count_of(stats.fetched, "Asset", "Assets"),
                    media::format_bytes(stats.bytes),
                    kept
                )),
            );
            stats.fetched
        };
        for sha in assets.keys() {
            if !self.journal.assets.contains(sha) {
                let event = ExportJournalEvent::AssetOk {
                    target: self.target.clone(),
                    sha256: sha.clone(),
                };
                if let Err(error) = journal::append(&self.journal_path, &event) {
                    emit(
                        out,
                        ProgressEvent::Log(format!(
                            "Asset {sha} could not be added to {}, the record of \
                             fetched Assets: {error:#}",
                            journal::EXPORT_JOURNAL_NAME
                        )),
                    );
                }
            }
        }
        Ok(AssetCounts { fetched, kept })
    }

    /// Write one JSON Lines file per conversation and return how many.
    ///
    /// The stem comes from [`message_ir::ConversationDocument::filename_stem`],
    /// which appends `packaging_stem_suffix` (the sanitized source, set here
    /// so the same chat from two sources lands in two files). The write is
    /// atomic: ir-format writes a `.tmp` sibling and renames it, so a crash
    /// never leaves a truncated conversation file.
    ///
    /// # Errors
    ///
    /// Returns an error when a file cannot be created, serialized, or renamed.
    fn write_conversations(
        &self,
        by_conv: BTreeMap<String, (Message, Vec<message_ir::IrMessage>)>,
    ) -> Result<u64> {
        let mut docs: Vec<message_ir::ConversationDocument> = by_conv
            .into_values()
            .map(|(seed, messages)| {
                let mut doc = build_document(&seed.source, &seed, messages);
                // Disambiguate same chat across sources.
                if !doc.export.source.trim().is_empty() {
                    doc.packaging_stem_suffix =
                        Some(format!("__{}", sanitize_source_suffix(&doc.export.source)));
                }
                doc
            })
            .collect();
        // Two groups from one source can still share a title or their people.
        let mut names: Vec<&mut message_ir::ConversationDocument> = docs.iter_mut().collect();
        message_ir::give_each_document_its_own_file(&mut names).map_err(anyhow::Error::msg)?;
        for doc in &docs {
            message_ir_format::write_conversation_jsonl(&self.cfg.out_dir, doc)?;
        }
        Ok(docs.len() as u64)
    }

    /// Record that this Export Run finished, then rewrite the journal in its
    /// shortest form. Every Asset this run saw is on disk: it was fetched
    /// above, or an earlier run had already fetched it. A journal write
    /// failure does not fail a run whose files are already written, so it
    /// goes to the progress log. A next run into the same directory still
    /// skips each Asset whose file is there, journal or not.
    fn finish_journal(
        &self,
        out: &mut Option<&mut ProgressFn<'_>>,
        export_id: i64,
        conversations: u64,
        messages: u64,
        assets: &AssetCounts,
        seen_assets: HashMap<String, String>,
    ) {
        let event = ExportJournalEvent::ExportComplete {
            target: self.target.clone(),
            conversations,
            messages,
            assets: assets.fetched + assets.kept,
        };
        if let Err(error) = journal::append(&self.journal_path, &event) {
            emit(
                out,
                ProgressEvent::Log(format!(
                    "Export Run {export_id} could not be recorded as finished in {}: {error:#}",
                    journal::EXPORT_JOURNAL_NAME
                )),
            );
        }
        let mut recorded_assets = self.journal.assets.clone();
        recorded_assets.extend(seen_assets.into_keys());
        let final_state = ExportJournalState {
            assets: recorded_assets,
            export_complete: true,
        };
        if let Err(error) = journal::compact(&self.journal_path, &self.target, &final_state) {
            emit(
                out,
                ProgressEvent::Log(format!(
                    "{} could not be rewritten in its shortest form: {error:#}",
                    journal::EXPORT_JOURNAL_NAME
                )),
            );
        }
    }
}

/// Remember where each attachment a message references should land on disk,
/// and return each attachment path [`export_path`] refused, with the path
/// used in its place.
///
/// The first message to mention a sha256 decides the path it is fetched to;
/// the server stores one blob per fingerprint for the account, whatever the
/// source, so later mentions are the same file. A later mention under
/// another path goes into `other_paths`, because staging names a file by
/// date and fingerprint and one file sent on two days has two paths.
fn note_asset_refs(
    msg: &Message,
    assets: &mut HashMap<String, String>,
    other_paths: &mut BTreeMap<String, BTreeSet<String>>,
) -> Vec<(String, Option<String>)> {
    let mut refused = Vec::new();
    for att in &msg.attachments {
        let ExportPath {
            rel,
            refused: refused_path,
        } = export_path(att);
        if let Some(path) = refused_path {
            refused.push((path, rel.clone()));
        }
        let (Some(sha), Some(rel)) = (att.sha256.as_deref().and_then(message_ir::trimmed), rel)
        else {
            continue;
        };
        // Lowercase, so one fingerprint in two cases is one fetch, never two
        // fetches writing one temporary file on a case-insensitive file
        // system.
        let sha = sha.to_ascii_lowercase();
        let first = assets.entry(sha.clone()).or_insert_with(|| rel.clone());
        if *first != rel {
            other_paths.entry(sha).or_default().insert(rel);
        }
    }
    refused
}

/// Put each fetched Asset at every other path a message names for it, so
/// every path in the conversation files exists. A hard link costs no space;
/// a copy stands in where the file system has no hard links (exFAT on a USB
/// drive). A path that already holds a file is left alone, the same way the
/// fetch skips one.
///
/// # Errors
///
/// Returns an error when a directory cannot be created or the file cannot be
/// linked or copied.
fn place_other_paths(
    out_dir: &Path,
    assets: &HashMap<String, String>,
    other_paths: &BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    for (sha, rels) in other_paths {
        let Some(first) = assets.get(sha) else {
            continue;
        };
        let from = out_dir.join(first);
        for rel in rels {
            let dest = out_dir.join(rel);
            if dest.is_file() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("mkdir {}", parent.display()))?;
            }
            if fs::hard_link(&from, &dest).is_err() {
                copy_into_place(&from, &dest, sha)?;
            }
        }
    }
    Ok(())
}

/// Copy `from`, the Asset whose SHA-256 is `sha256`, to `dest` through the
/// Asset's own temporary file beside `dest` ([`write_asset`]), so a crash
/// never leaves a short file that a later Export Run would take as finished,
/// and a file already beside `dest` under another name is left alone.
///
/// # Errors
///
/// Returns an error when `from` cannot be read, or for any reason
/// [`write_asset`] gives.
fn copy_into_place(from: &Path, dest: &Path, sha256: &str) -> Result<()> {
    let mut source = fs::File::open(from).with_context(|| format!("open {}", from.display()))?;
    write_asset(dest, sha256, |out| {
        std::io::copy(&mut source, out).with_context(|| format!("copy {}", from.display()))?;
        Ok(())
    })
    .with_context(|| format!("copy {} -> {}", from.display(), dest.display()))
}

/// The progress line for an attachment path the export refused, naming the
/// path written in its place, or saying the file is not written when the
/// attachment has no SHA-256.
fn refused_path_line(path: &str, rel: Option<&str>) -> String {
    match rel {
        Some(rel) => format!(
            "Attachment path {path} would leave the Export's directory, \
             so the file is written at {rel} instead"
        ),
        None => format!(
            "Attachment path {path} would leave the Export's directory, and the \
             attachment has no SHA-256 to name another path by, so it is not written"
        ),
    }
}

struct AssetFetchJob {
    sha256: String,
    dest: PathBuf,
}

#[derive(Default)]
struct AssetFetchStats {
    /// The bytes this run fetched.
    bytes: u64,
    fetched: u64,
}

/// The Assets in `assets` (SHA-256 to path under `out_dir`) whose file is not
/// at its path. A file at its path is kept whether or not the journal lists
/// it, because an earlier run can stop after writing a file but before
/// journaling it.
fn assets_to_fetch(assets: &HashMap<String, String>, out_dir: &Path) -> HashMap<String, String> {
    assets
        .iter()
        .filter(|(_, rel)| !out_dir.join(rel).is_file())
        .map(|(sha, rel)| (sha.clone(), rel.clone()))
        .collect()
}

/// What [`fetch_assets_parallel`] fetches, from where, and how: the
/// server and session, each Asset's SHA-256 with the path under `out_dir` it
/// is written at, the number of workers, and the run's cancel flag.
struct FetchAssetsParallelArgs<'a> {
    session: &'a crate::http::HttpSession,
    base_url: &'a str,
    token: &'a str,
    assets: &'a HashMap<String, String>, // sha256 -> rel_path
    out_dir: &'a Path,
    workers: usize,
    cancel: Option<&'a CancelFlag>,
}

/// Fetch each Asset in `assets` on `workers` threads and return the counts and
/// the bytes fetched.
///
/// The same pattern as the Upload's `upload_assets`: the jobs are collected,
/// then [`parallel_for_each`] runs them. `assets` holds only Assets whose file
/// is not at its path ([`assets_to_fetch`]). Each fetch retries a transient
/// HTTP failure, as the Upload does.
///
/// # Errors
///
/// Returns an error when a fetch fails after retries or the run is cancelled.
fn fetch_assets_parallel(args: FetchAssetsParallelArgs<'_>) -> Result<AssetFetchStats> {
    let FetchAssetsParallelArgs {
        session,
        base_url,
        token,
        assets,
        out_dir,
        workers,
        cancel,
    } = args;
    let jobs: Vec<AssetFetchJob> = assets
        .iter()
        .map(|(sha256, rel)| AssetFetchJob {
            sha256: sha256.clone(),
            dest: out_dir.join(rel),
        })
        .collect();
    let mut stats = AssetFetchStats::default();

    let results = parallel_for_each(&jobs, workers, cancel, |job| {
        with_retries(MAX_RETRIES, || {
            crate::http::fetch_asset(session, base_url, token, &job.sha256, &job.dest)
        })
        .map_err(|e| format!("{e:#}"))
    });

    for result in results {
        match result {
            Ok(bytes) => {
                stats.bytes = stats.bytes.saturating_add(bytes);
                stats.fetched += 1;
            }
            Err(error) => {
                bail!("{error}");
            }
        }
    }
    Ok(stats)
}

/// Keep letters, digits, `-`, and `_`; replace every other character with `_`.
fn sanitize_source_suffix(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for c in source.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod paging_tests {
    use super::next_offset;

    #[test]
    fn paging_steps_by_the_limit_and_stops_at_the_total() {
        assert_eq!(next_offset(0, 500, 1200), Some(500));
        assert_eq!(next_offset(500, 500, 1200), Some(1000));
        assert_eq!(next_offset(1000, 500, 1200), None);
        assert_eq!(next_offset(0, 0, 1200), None, "a zero limit cannot spin");
    }
}

#[cfg(test)]
mod out_dir_tests {
    use super::*;
    use message_ir_format::EXPORT_SENTINEL;

    #[test]
    fn marks_the_directory_as_an_export() {
        // Without the sentinel the desktop app's staging guard refuses to
        // clean an exported directory, so an export that staged into one would
        // leave the run directory behind.
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("exported");

        prepare_out_dir(&out, false).unwrap();

        assert!(out.join(EXPORT_SENTINEL).is_file());
    }

    #[test]
    fn creates_attachments_unless_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let with = dir.path().join("with");
        let without = dir.path().join("without");

        prepare_out_dir(&with, false).unwrap();
        prepare_out_dir(&without, true).unwrap();

        assert!(with.join("attachments").is_dir());
        assert!(!without.join("attachments").exists());
    }

    #[test]
    fn runs_again_over_a_directory_it_already_prepared() {
        // A second export into the same directory is a later Export Run over it.
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("exported");

        prepare_out_dir(&out, false).unwrap();
        prepare_out_dir(&out, false).unwrap();

        assert!(out.join(EXPORT_SENTINEL).is_file());
    }

    #[test]
    fn refuses_a_directory_of_the_persons_own_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("budget.csv"), "mine").unwrap();

        prepare_out_dir(dir.path(), false).unwrap_err();

        assert!(!dir.path().join(EXPORT_SENTINEL).exists());
        assert!(!dir.path().join("attachments").exists());
    }
}

#[cfg(test)]
mod suffix_tests {
    use super::sanitize_source_suffix;

    #[test]
    fn a_source_name_keeps_letters_digits_dash_and_underscore_only() {
        assert_eq!(
            sanitize_source_suffix("sms-backup-restore"),
            "sms-backup-restore"
        );
        assert_eq!(
            sanitize_source_suffix("whatsapp (phone 2)"),
            "whatsapp__phone_2_"
        );
        assert_eq!(sanitize_source_suffix("café/2024"), "caf__2024");
    }
}

#[cfg(test)]
mod asset_ref_tests {
    use super::*;
    use serde_json::json;

    /// One exported message from `source` carrying `attachments`, with the
    /// rest of the server's shape at its plainest.
    fn message_from(source: &str, attachments: serde_json::Value) -> Message {
        serde_json::from_value(json!({
            "id": 1,
            "source": source,
            "guid": "g1",
            "timestamp": "2015-03-12T18:05:22Z",
            "time_precision": "milliseconds",
            "sort_order": 0,
            "is_from_me": false,
            "is_announcement": false,
            "reply_count": 0,
            "conversation": {
                "id": 9,
                "chat_identifier": "+15555550101",
                "conversation_type": "individual",
                "is_group": false,
                "participants": []
            },
            "attachments": attachments,
            "tapbacks": [],
            "earlier_versions": [],
            "matched_earlier_version": false
        }))
        .unwrap()
    }

    #[test]
    fn the_first_mention_of_a_fingerprint_decides_its_path_and_every_other_path_is_kept() {
        let mut assets = HashMap::new();
        let mut other_paths = BTreeMap::new();

        let refused = note_asset_refs(
            &message_from(
                "imessage",
                json!([
                    { "path": "attachments/menu.pdf", "sha256": "ab", "is_sticker": false, "shown_as_is": false },
                    { "path": "/attachments/photo.png", "sha256": "cd", "is_sticker": false, "shown_as_is": false },
                    { "sha256": " ef ", "is_sticker": false, "shown_as_is": false },
                    { "path": "attachments/no-fingerprint.txt", "is_sticker": false, "shown_as_is": false },
                    { "path": "../no-fingerprint.txt", "is_sticker": false, "shown_as_is": false }
                ]),
            ),
            &mut assets,
            &mut other_paths,
        );
        note_asset_refs(
            &message_from(
                "sms",
                json!([
                    { "path": "other/menu.pdf", "sha256": "ab", "is_sticker": false, "shown_as_is": false },
                    { "path": "attachments/menu.pdf", "sha256": "ab", "is_sticker": false, "shown_as_is": false }
                ]),
            ),
            &mut assets,
            &mut other_paths,
        );

        assert_eq!(
            assets,
            HashMap::from([
                ("ab".to_string(), "attachments/menu.pdf".to_string()),
                ("cd".to_string(), "attachments/cd".to_string()),
                ("ef".to_string(), "attachments/ef".to_string()),
            ])
        );
        assert_eq!(
            other_paths,
            BTreeMap::from([(
                "ab".to_string(),
                BTreeSet::from(["other/menu.pdf".to_string()])
            )])
        );
        assert_eq!(
            refused,
            [
                (
                    "/attachments/photo.png".to_string(),
                    Some("attachments/cd".to_string())
                ),
                ("../no-fingerprint.txt".to_string(), None),
            ]
        );
    }

    #[test]
    fn one_fingerprint_in_two_cases_is_one_asset() {
        // Two keys would be two fetches at once writing one temporary file
        // on a case-insensitive file system.
        let mut assets = HashMap::new();
        let mut other_paths = BTreeMap::new();

        note_asset_refs(
            &message_from(
                "sms",
                json!([
                    { "path": "attachments/menu.pdf", "sha256": "AB", "is_sticker": false, "shown_as_is": false },
                    { "path": "attachments/menu.pdf", "sha256": "ab", "is_sticker": false, "shown_as_is": false }
                ]),
            ),
            &mut assets,
            &mut other_paths,
        );

        assert_eq!(
            assets,
            HashMap::from([("ab".to_string(), "attachments/menu.pdf".to_string())])
        );
        assert!(other_paths.is_empty());
    }
}

#[cfg(test)]
mod asset_fetch_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    /// The names of the files in `dir`, sorted.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// A server that answers two `GET /v1/assets/{sha256}` requests with the
    /// bytes `bodies` holds under that fingerprint, and returns its address.
    /// It sends the first half of both answers, waits, then sends the second
    /// halves, so both fetches are writing their files at the same time.
    fn serve_two_fetches_at_once(bodies: HashMap<String, Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            let mut answers = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut request = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                request.read_line(&mut request_line).unwrap();
                // Read the headers through to the blank line that ends them.
                let mut header = String::new();
                while request.read_line(&mut header).unwrap() > 2 {
                    header.clear();
                }
                let sha256 = request_line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|path| path.strip_prefix("/v1/assets/"))
                    .unwrap();
                answers.push((stream, bodies[sha256].clone()));
            }
            // A third request, such as a retry, is refused rather than left
            // waiting.
            drop(listener);
            for (stream, body) in &mut answers {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body[..body.len() / 2]).unwrap();
                stream.flush().unwrap();
            }
            thread::sleep(Duration::from_millis(300));
            for (stream, body) in &mut answers {
                stream.write_all(&body[body.len() / 2..]).unwrap();
                stream.flush().unwrap();
            }
        });
        base_url
    }

    #[test]
    fn two_assets_whose_paths_differ_only_in_extension_both_arrive_whole() {
        // `menu.pdf` and `menu.jpg` once shared `menu.part`: the second fetch
        // to start truncated the first one's file, and the first rename moved
        // the mixed bytes into place.
        let pdf = vec![b'p'; 4096];
        let jpg = vec![b'j'; 4096];
        let pdf_sha = hex::encode(Sha256::digest(&pdf));
        let jpg_sha = hex::encode(Sha256::digest(&jpg));
        let base_url = serve_two_fetches_at_once(HashMap::from([
            (pdf_sha.clone(), pdf.clone()),
            (jpg_sha.clone(), jpg.clone()),
        ]));
        let dir = tempfile::tempdir().unwrap();
        let assets = HashMap::from([
            (pdf_sha, "attachments/menu.pdf".to_string()),
            (jpg_sha, "attachments/menu.jpg".to_string()),
        ]);

        let stats = fetch_assets_parallel(FetchAssetsParallelArgs {
            session: &HttpSession::new().unwrap(),
            base_url: &base_url,
            token: "token",
            assets: &assets,
            out_dir: dir.path(),
            workers: 2,
            cancel: None,
        })
        .unwrap();

        assert_eq!(stats.fetched, 2);
        let attachments = dir.path().join("attachments");
        assert_eq!(fs::read(attachments.join("menu.pdf")).unwrap(), pdf);
        assert_eq!(fs::read(attachments.join("menu.jpg")).unwrap(), jpg);
        assert_eq!(
            names_in(&attachments),
            ["menu.jpg", "menu.pdf"],
            "no temporary file is left behind"
        );
    }

    #[test]
    fn a_copy_to_a_second_path_leaves_a_file_beside_it_alone() {
        // The copy once went through `<stem>.part`, which wrote over a file
        // of that name and then renamed it away.
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("first.pdf");
        let dest = dir.path().join("menu.pdf");
        fs::write(&from, b"the menu").unwrap();
        fs::write(dir.path().join("menu.part"), b"another attachment").unwrap();

        copy_into_place(&from, &dest, &"ab".repeat(32)).unwrap();

        assert_eq!(fs::read(&dest).unwrap(), b"the menu");
        assert_eq!(
            fs::read(dir.path().join("menu.part")).unwrap(),
            b"another attachment"
        );
        assert_eq!(names_in(dir.path()), ["first.pdf", "menu.part", "menu.pdf"]);
    }
}
