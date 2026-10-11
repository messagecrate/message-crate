//! Cross-source content fingerprint and soft-hide dedupe.
//!
//! This module sequences the dedupe, hashes the content keys, picks the copy
//! shown, says how far it is and keeps the counts. Its statements are in
//! `crate::db::dedupe` (`docs/architecture/http-api.md`, "Code").

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Instant;

use anyhow::{Context, Result};
use rayon::prelude::*;
use sqlx::SqliteConnection;

use crate::counts::words;
use crate::db::conversations::is_group_type;
use crate::db::dedupe::{self as db, ContentKeyInputs, ContentKeyRow, KeyScope, NearRows};
use crate::db::{WriteTx, begin_write};
use crate::progress::Progress;

const CONTENT_KEY_WRITE_LOG_EVERY: usize = 50_000;

/// Collapse whitespace so minor text differences do not split the same SMS.
pub fn normalize_body(body: Option<&str>) -> String {
    message_ir::collapse_whitespace(body.unwrap_or(""))
}

/// Stable chat identity for content keys.
///
/// 1:1 chats use the conversation handle's normalized form (E.164 phones,
/// lowercased emails, verbatim usernames). Groups use sorted normalized
/// participant handles so the same people across exporters (different
/// `chat_identifier`s) share one fingerprint.
pub fn chat_identity_for_content_key(
    chat_identifier: &str,
    group_handles: Option<&[String]>,
) -> String {
    match group_handles {
        Some(handles) if !handles.is_empty() => {
            let mut sorted: Vec<&str> = handles
                .iter()
                .map(|h| h.as_str())
                .filter(|h| !h.is_empty())
                .collect();
            sorted.sort_unstable();
            sorted.dedup();
            format!("group:{}", sorted.join("|"))
        }
        _ => chat_identifier.to_string(),
    }
}

/// Build a content key from chat + direction + sender + UTC second + body + attachment hashes.
///
/// The key is the message's [`message_ir::MessageIdentity`] at whole seconds,
/// made by the same function as an exported message's `guid` (which is made at
/// milliseconds): it matches one message across sources, and iMazing,
/// OpenExtract and GO SMS Pro's PDU files record whole seconds only.
///
/// `timestamp` is the stored UTC instant; `None` when it does not parse.
/// For groups, pass the sorted-participant identity from [`chat_identity_for_content_key`].
/// Incoming group messages include the normalized sender so two peers sending the same
/// text at the same second do not collide; outgoing (`is_from_me`) uses an empty sender.
pub fn compute_content_key(
    chat_identifier: &str,
    is_from_me: bool,
    sender_normalized: Option<&str>,
    timestamp: &str,
    body: Option<&str>,
    attachment_shas: &[String],
) -> Option<String> {
    let secs = parse_rfc3339_utc_secs(timestamp)?;
    let identity = message_ir::MessageIdentity {
        chat: chat_identifier,
        is_from_me,
        sender: sender_normalized,
        timestamp_unix_ms: secs.saturating_mul(1000),
        text: body.unwrap_or(""),
        attachment_digests: attachment_shas,
        vendor_key: None,
    };
    Some(identity.key(message_ir::TimePrecision::Seconds))
}

/// Fingerprint one message row from its chat identity, direction, sender, time, body, and attachment hashes.
///
/// `None` when the row's time does not parse; such a row keeps no content key.
fn content_key_for_row(
    row: &ContentKeyRow,
    group_handles: &HashMap<i64, Vec<String>>,
    shas_by_msg: &HashMap<i64, Vec<String>>,
) -> Option<(i64, String)> {
    let empty: &[String] = &[];
    let shas = shas_by_msg.get(&row.id).map_or(empty, Vec::as_slice);
    let group_identity = if is_group_type(&row.conversation_type) {
        Some(chat_identity_for_content_key(
            &row.chat_id,
            group_handles.get(&row.conversation_id).map(Vec::as_slice),
        ))
    } else {
        None
    };
    let identity = group_identity.as_deref().unwrap_or(&row.chat_id);
    let key = compute_content_key(
        identity,
        row.is_from_me != 0,
        row.sender_normalized.as_deref(),
        &row.timestamp,
        row.body.as_deref(),
        shas,
    )?;
    Some((row.id, key))
}

/// `(message id, content key)` for every row of `inputs`, hashed in parallel.
fn hash_content_keys(inputs: &ContentKeyInputs) -> Vec<(i64, String)> {
    inputs
        .rows
        .par_iter()
        .filter_map(|row| content_key_for_row(row, &inputs.group_handles, &inputs.shas_by_msg))
        .collect()
}

/// Counts reported by one cross-source dedupe pass.
#[derive(Debug, Default)]
pub struct DedupeStats {
    /// Content keys written: those missing and those whose inputs changed
    /// (not a duplicate count).
    pub keys_filled: u64,
    /// Groups of messages sharing one content key in which a message was
    /// hidden.
    pub exact_groups: u64,
    /// Messages hidden as exact duplicates: all but as many per group as one
    /// source holds, and each whole-second message whose own source holds
    /// it with milliseconds too.
    pub exact_flagged: u64,
    /// Messages flagged as near duplicates.
    pub near_flagged: u64,
}

/// Refresh the content keys, clear prior flags, then soft-hide cross-source
/// duplicates and the whole-second twins of a message one source holds with
/// milliseconds too, all in one transaction.
///
/// The copy shown is the first by [`rank`]. Optional `source_priority`
/// overrides the order of the sources (tests); `None` loads it from the DB.
pub async fn dedupe_cross_source(
    conn: &mut SqliteConnection,
    account_id: i64,
    source_priority: Option<&[String]>,
    near_window_secs: i64,
    progress: Progress,
) -> Result<DedupeStats> {
    // One write transaction for the whole run: the flags are cleared and set
    // again, so a pass that fails after the clearing would otherwise leave
    // every duplicate in the account shown until the next successful run. It
    // holds the write lock from the first read, so an import that commits
    // while the keys are hashed waits instead of failing the pass.
    let mut tx = begin_write(conn).await?;
    let owned_priority;
    let priority = if let Some(p) = source_priority {
        p
    } else {
        owned_priority = db::sources_in_import_order(&mut tx, account_id).await?;
        owned_priority.as_slice()
    };
    let mut stats = DedupeStats::default();
    let prio: HashMap<&str, usize> = priority
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let started = Instant::now();
    let exact_hidden: HashSet<i64>;

    {
        progress.say("Refreshing the content keys that match the same message across sources…");
        stats.keys_filled = refresh_content_keys(&mut tx, account_id, progress).await?;
        db::clear_account_duplicate_flags(&mut tx, account_id).await?;
        progress.say(format_args!(
            "Wrote {} in {:.1} s",
            words(stats.keys_filled, "1 content key", "{n} content keys"),
            started.elapsed().as_secs_f64()
        ));
    }

    {
        progress.say("Hiding exact duplicates, the messages that share a content key…");
        let (groups, flags) = flag_exact_content_key_dupes(&mut tx, account_id, &prio).await?;
        stats.exact_groups = groups;
        stats.exact_flagged = flags.len() as u64;
        exact_hidden = flags.into_iter().map(|(loser, _)| loser).collect();
        progress.say(format_args!(
            "Found {} and hid {}, {:.1} s in all",
            words(
                stats.exact_groups,
                "1 group of exact duplicates",
                "{n} groups of exact duplicates"
            ),
            words(stats.exact_flagged, "1 message", "{n} messages"),
            started.elapsed().as_secs_f64()
        ));
    }

    {
        progress.say(format_args!(
            "Flagging near duplicates, sent within {near_window_secs} s of each other…"
        ));
        stats.near_flagged =
            flag_near_time_dupes(&mut tx, account_id, &prio, near_window_secs, &exact_hidden)
                .await?;
        progress.say(format_args!(
            "Flagged {}, {:.1} s in all",
            words(
                stats.near_flagged,
                "1 near duplicate",
                "{n} near duplicates"
            ),
            started.elapsed().as_secs_f64()
        ));
    }
    tx.commit().await?;

    Ok(stats)
}

/// The near-time window, in seconds, of a dedupe nobody chose one for: the
/// one after each import batch, and the pass a phone country change runs
/// for the messages it changed.
pub const NEAR_WINDOW_SECS: i64 = 2;

/// What [`dedupe_changed_messages`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChangedDedupe {
    /// Messages that were hidden and are shown now.
    pub shown: u64,
    /// Messages that were shown and are hidden now, or hidden behind
    /// another message than before.
    pub hidden: u64,
}

/// Put right the duplicate flags of the messages `changed`, whose content
/// a change in the transaction `conn` is in made different, and of every
/// message whose flag depends on theirs (#1805). A phone number given its
/// country merges conversations and senders, which changes the content
/// keys of the messages that moved (`crate::identity_country`); an import
/// leaves this to the full dedupe after each batch instead.
///
/// This computes the changed messages' content keys again, then runs both
/// passes of [`dedupe_cross_source`] over the messages with a content key,
/// so a message without one is neither hidden nor a winner here. Only the
/// flags of the messages tied to a changed one are written: those it was
/// hidden behind or hid, before or now, and the messages tied to those in
/// turn. Every other flag stays as it is.
///
/// # Errors
///
/// Returns an error when a statement fails or the hashing task panics.
pub async fn dedupe_changed_messages(
    conn: &mut SqliteConnection,
    account_id: i64,
    changed: &[i64],
    near_window_secs: i64,
    progress: Progress,
) -> Result<ChangedDedupe> {
    if changed.is_empty() {
        return Ok(ChangedDedupe::default());
    }
    db::clear_content_keys(conn, changed).await?;
    recompute_content_keys(conn, KeyScope::Changed(changed), account_id, progress).await?;

    let priority = db::sources_in_import_order(conn, account_id).await?;
    let prio: HashMap<&str, usize> = priority
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let (_, mut flags) = exact_flags(conn, account_id, &prio).await?;
    let exact_hidden: HashSet<i64> = flags.iter().map(|&(loser, _)| loser).collect();
    let by_conversation = load_near_rows(conn, account_id, NearRows::Keyed, &exact_hidden).await?;
    flags.extend(cluster_near_dupes(by_conversation, &prio, near_window_secs));
    let now: HashMap<i64, i64> = flags.into_iter().collect();

    let stored = db::duplicate_flags(conn, account_id).await?;

    let tied = tied_messages(changed, &stored, &now);
    let mut result = ChangedDedupe::default();
    let mut flags: Vec<(i64, i64)> = Vec::new();
    for &id in &tied {
        match (stored.get(&id), now.get(&id)) {
            (Some(_), None) => result.shown += 1,
            (before, Some(&winner)) => {
                if before != Some(&winner) {
                    result.hidden += 1;
                }
                flags.push((id, winner));
            }
            (None, None) => {}
        }
    }
    let tied: Vec<i64> = tied.into_iter().collect();
    db::clear_duplicate_flags(conn, &tied).await?;
    if !flags.is_empty() {
        db::write_duplicate_flags(conn, "_changed_flags", &flags).await?;
    }
    Ok(result)
}

/// The messages tied to one in `changed`: `changed` itself, and every
/// message one hides or is hidden behind, in the `(loser → winner)` maps
/// `stored` (the flags as they are) or `now` (as the passes would set
/// them), followed to the end.
///
/// The set is closed both ways, so writing the flags `now` gives its
/// messages leaves no stored flag outside it pointing at a message inside
/// it, and no new flag inside it pointing outside: every hidden message
/// still points at a shown one.
fn tied_messages(
    changed: &[i64],
    stored: &HashMap<i64, i64>,
    now: &HashMap<i64, i64>,
) -> BTreeSet<i64> {
    let mut neighbours: HashMap<i64, Vec<i64>> = HashMap::new();
    for (&loser, &winner) in stored.iter().chain(now.iter()) {
        neighbours.entry(loser).or_default().push(winner);
        neighbours.entry(winner).or_default().push(loser);
    }
    let mut tied: BTreeSet<i64> = changed.iter().copied().collect();
    let mut queue: Vec<i64> = changed.to_vec();
    while let Some(id) = queue.pop() {
        for &next in neighbours.get(&id).into_iter().flatten() {
            if tied.insert(next) {
                queue.push(next);
            }
        }
    }
    tied
}

/// Compute `content_key` for production rows that still lack one (after attachments exist).
pub async fn fill_missing_content_keys(
    conn: &mut SqliteConnection,
    account_id: i64,
    progress: Progress,
) -> Result<u64> {
    recompute_content_keys(conn, KeyScope::Missing, account_id, progress).await
}

/// Recompute every content key of the account and write the ones that differ
/// from what is stored. Returns how many were written.
///
/// A key is first written at import, but a later append import can add
/// attachments to a message the database already holds, or participants to a
/// group, and both are part of the key. A key left as it was stops matching
/// the same message from another source.
async fn refresh_content_keys(
    conn: &mut SqliteConnection,
    account_id: i64,
    progress: Progress,
) -> Result<u64> {
    recompute_content_keys(conn, KeyScope::All, account_id, progress).await
}

/// Compute and store content keys for the account's messages in `scope`.
/// Returns how many were written.
///
/// # Errors
///
/// Returns an error when a query fails or the hashing task panics.
async fn recompute_content_keys(
    conn: &mut SqliteConnection,
    scope: KeyScope<'_>,
    account_id: i64,
    progress: Progress,
) -> Result<u64> {
    let Some(inputs) = db::content_key_inputs(conn, account_id, scope).await? else {
        return Ok(0);
    };
    progress.say(format_args!(
        "Hashing the content keys of {}…",
        words(inputs.rows.len() as u64, "1 message", "{n} messages")
    ));
    let keys = tokio::task::spawn_blocking(move || hash_content_keys(&inputs))
        .await
        .context("content-key hash task panicked")?;
    let keys = if scope != KeyScope::All {
        keys
    } else {
        let stored = db::stored_content_keys(conn, account_id).await?;
        keys.into_iter()
            .filter(|(id, key)| stored.get(id) != Some(key))
            .collect()
    };
    if keys.is_empty() {
        return Ok(0);
    }
    let total = keys.len();
    let mut previous = 0usize;
    db::write_content_keys(conn, &keys, |written| {
        // Say how far the write is at each multiple of the log interval it
        // passes, and when it is done.
        if written == total
            || written / CONTENT_KEY_WRITE_LOG_EVERY != previous / CONTENT_KEY_WRITE_LOG_EVERY
        {
            progress.say(format_args!("Wrote {written} of {total} content keys"));
        }
        previous = written;
    })
    .await?;
    Ok(keys.len() as u64)
}

/// One copy of a message, with what decides whether it is the copy shown
/// ([`rank`]).
#[derive(Clone)]
struct Cand {
    id: i64,
    source: String,
    att_count: i64,
    /// Whether its source recorded its time in whole seconds. The flag
    /// decides, never the time: a millisecond time can end in `.000`.
    whole_seconds: bool,
    /// How many messages its Import Run brought: the account's messages
    /// stamped with the run, which are the ones it added; 0 for a message
    /// no run stamped.
    run_messages: i64,
}

/// Whether `time_precision`, as `messages.time_precision` stores it, is
/// whole seconds.
fn is_whole_seconds(id: i64, time_precision: &str) -> Result<bool> {
    Ok(message_ir::TimePrecision::parse(time_precision)
        .with_context(|| format!("message {id}: time_precision {time_precision:?}"))?
        == message_ir::TimePrecision::Seconds)
}

/// Hide the messages that share a fingerprint with a preferred-source twin,
/// keeping as many as one source holds (see [`exact_group_flags`]), and
/// each whole-second message whose own source holds it with milliseconds
/// too (see [`content_key_group_flags`]). Returns the groups in which a
/// message was hidden, and the `(loser, winner)` pairs.
async fn flag_exact_content_key_dupes(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    prio: &HashMap<&str, usize>,
) -> Result<(u64, Vec<(i64, i64)>)> {
    let conn: &mut SqliteConnection = tx;
    let (groups, flags) = exact_flags(conn, account_id, prio).await?;
    if !flags.is_empty() {
        db::write_duplicate_flags(conn, "_pass_a_flags", &flags).await?;
    }
    Ok((groups, flags))
}

/// The exact pass over every message of the account with a content key,
/// as though none were hidden: the groups in which a message would be
/// hidden, and the `(loser, winner)` pairs. Nothing is written.
async fn exact_flags(
    conn: &mut SqliteConnection,
    account_id: i64,
    prio: &HashMap<&str, usize>,
) -> Result<(u64, Vec<(i64, i64)>)> {
    let mut by_key: HashMap<String, Vec<Cand>> = HashMap::new();
    for row in db::exact_rows(conn, account_id).await? {
        by_key.entry(row.content_key).or_default().push(Cand {
            id: row.id,
            whole_seconds: is_whole_seconds(row.id, &row.time_precision)?,
            source: row.source,
            att_count: row.att_count,
            run_messages: row.run_messages,
        });
    }

    let mut flags: Vec<(i64, i64)> = Vec::new(); // (loser_id, winner_id)
    let mut groups = 0u64;
    for cands in by_key.into_values() {
        let group_flags = content_key_group_flags(cands, prio);
        if !group_flags.is_empty() {
            groups += 1;
        }
        flags.extend(group_flags);
    }
    Ok((groups, flags))
}

/// The `(loser, winner)` pairs of the messages that share one content key.
///
/// A whole-second message whose own source also holds a message of the same
/// key with milliseconds is first set aside as that message's twin: the key
/// is taken at whole seconds, so the two match in everything else and fall
/// in the same second, and the source recorded the message twice, once
/// without its milliseconds (an SMS Backup+ mail timed by its `Date` header
/// beside one timed by `X-smssync-date`). The rest are flagged by
/// [`exact_group_flags`] when two or more sources hold them, and each twin
/// is hidden under the rest's winner, which is always shown, so the message
/// is shown once. It keeps its milliseconds unless another source's copy
/// wins the cross-source comparison ([`rank`]), as one with more attachments
/// does. A source that holds the message only in whole seconds keeps
/// every copy, as one that holds it only with milliseconds does.
fn content_key_group_flags(cands: Vec<Cand>, prio: &HashMap<&str, usize>) -> Vec<(i64, i64)> {
    let with_milliseconds: HashSet<String> = cands
        .iter()
        .filter(|c| !c.whole_seconds)
        .map(|c| c.source.clone())
        .collect();
    let (twins, rest): (Vec<Cand>, Vec<Cand>) = cands
        .into_iter()
        .partition(|c| c.whole_seconds && with_milliseconds.contains(c.source.as_str()));
    let sources: HashSet<&str> = rest.iter().map(|c| c.source.as_str()).collect();
    let mut flags = if sources.len() < 2 {
        Vec::new()
    } else {
        exact_group_flags(&rest, prio)
    };
    if !twins.is_empty() {
        let winner = pick_winner(&rest, prio);
        flags.extend(twins.into_iter().map(|t| (t.id, winner)));
    }
    flags
}

/// The `(loser, winner)` pairs of one group of copies that two or more
/// sources hold: the messages of one content key in the exact pass, or one
/// cluster of the near-time pass ([`cluster_near_dupes`]).
///
/// One source that holds a message twice holds two messages, so the group
/// stays shown as many times as the source that holds it most often. Those
/// places go to the copies first by [`rank`], the winner among them, and
/// every other copy is hidden as a duplicate of the winner.
fn exact_group_flags(cands: &[Cand], prio: &HashMap<&str, usize>) -> Vec<(i64, i64)> {
    let mut per_source: HashMap<&str, usize> = HashMap::new();
    for c in cands {
        *per_source.entry(c.source.as_str()).or_default() += 1;
    }
    let shown = per_source.values().copied().max().unwrap_or(0);
    let winner = pick_winner(cands, prio);
    let mut ranked: Vec<&Cand> = cands.iter().collect();
    ranked.sort_by(|a, b| rank(a, b, prio));
    ranked
        .into_iter()
        .skip(shown)
        .map(|c| (c.id, winner))
        .collect()
}

/// The message to keep from a duplicate group: the first by [`rank`].
fn pick_winner(cands: &[Cand], prio: &HashMap<&str, usize>) -> i64 {
    cands
        .iter()
        .min_by(|a, b| rank(a, b, prio))
        .map_or(cands[0].id, |c| c.id)
}

/// The order in which copies of one message are shown, the first shown
/// before the rest (`docs/architecture/contacts-identities-and-messages.md`,
/// "Which copy is shown"): the copy with more attachments; then the copy
/// timed to the millisecond over one timed to the second; then the copy
/// from the Import Run that brought more messages; then the copy of the
/// source imported first (`prio`); then the lower id. The last two only
/// make the order stable.
fn rank(a: &Cand, b: &Cand, prio: &HashMap<&str, usize>) -> std::cmp::Ordering {
    let priority = |c: &Cand| prio.get(c.source.as_str()).copied().unwrap_or(usize::MAX);
    b.att_count
        .cmp(&a.att_count)
        .then(a.whole_seconds.cmp(&b.whole_seconds))
        .then(b.run_messages.cmp(&a.run_messages))
        .then_with(|| priority(a).cmp(&priority(b)))
        .then(a.id.cmp(&b.id))
}

/// Parse an RFC3339 timestamp into Unix UTC seconds, honoring Z / ±HH:MM
/// offsets and dropping the milliseconds: the content key and the near-time
/// pass match at whole seconds.
///
/// Strict RFC3339 is sufficient: `messages.timestamp` is only ever written by
/// `models::utc_timestamp_text`, so no lenient spellings reach this path.
/// Unparseable input yields `None`.
fn parse_rfc3339_utc_secs(timestamp_rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp_rfc3339.trim())
        .ok()
        .map(|dt| dt.timestamp())
}

#[derive(Clone)]
struct NearRow {
    id: i64,
    source: String,
    is_from_me: i64,
    sender_norm: String,
    secs: i64,
    body_norm: String,
    att_fp: String,
    att_count: i64,
    content_key: String,
    whole_seconds: bool,
    run_messages: i64,
}

impl NearRow {
    /// Whether `other`, a later row of the same conversation, is a near-time
    /// twin of this one: same direction and sender, a different source, a
    /// different content key, and the same body or the same attachments.
    ///
    /// Two shown rows with one content key are rows the exact pass chose to
    /// keep, because one source holds the message that many times, so they
    /// are never twins here.
    fn is_twin_of(&self, other: &NearRow) -> bool {
        let same_body = !self.body_norm.is_empty() && other.body_norm == self.body_norm;
        let same_attachments = !self.att_fp.is_empty() && other.att_fp == self.att_fp;
        let same_key = !self.content_key.is_empty() && other.content_key == self.content_key;
        other.is_from_me == self.is_from_me
            && other.sender_norm == self.sender_norm
            && other.source != self.source
            && !same_key
            && (same_body || same_attachments)
    }

    /// The row as a winner candidate.
    fn candidate(&self) -> Cand {
        Cand {
            id: self.id,
            source: self.source.clone(),
            att_count: self.att_count,
            whole_seconds: self.whole_seconds,
            run_messages: self.run_messages,
        }
    }
}

/// Flag messages that match one from another source within `window_secs` on chat,
/// direction, sender, and body or attachments, leaving out those the exact
/// pass hid (`exact_hidden`). Returns how many were flagged.
async fn flag_near_time_dupes(
    tx: &mut WriteTx<'_>,
    account_id: i64,
    prio: &HashMap<&str, usize>,
    window_secs: i64,
    exact_hidden: &HashSet<i64>,
) -> Result<u64> {
    let conn: &mut SqliteConnection = tx;
    let by_conversation = load_near_rows(conn, account_id, NearRows::All, exact_hidden).await?;
    let flags = cluster_near_dupes(by_conversation, prio, window_secs);
    if flags.is_empty() {
        return Ok(0);
    }
    db::write_duplicate_flags(conn, "_pass_b_flags", &flags).await?;
    Ok(flags.len() as u64)
}

/// The messages of the account in `rows` that are not in `hidden`, with
/// their attachment fingerprints, grouped by conversation. Two queries and
/// the grouping happen here so the clustering does no per-message lookups.
async fn load_near_rows(
    conn: &mut SqliteConnection,
    account_id: i64,
    rows: NearRows,
    hidden: &HashSet<i64>,
) -> Result<HashMap<i64, Vec<NearRow>>> {
    let msg_rows = db::near_message_rows(conn, account_id, rows).await?;
    let mut shas_by_msg = db::attachment_digests(conn, account_id).await?;

    let mut by_conversation: HashMap<i64, Vec<NearRow>> = HashMap::new();
    for row in msg_rows {
        if hidden.contains(&row.id) {
            continue;
        }
        let Some(secs) = parse_rfc3339_utc_secs(&row.timestamp) else {
            continue;
        };
        let shas = shas_by_msg.remove(&row.id).unwrap_or_default();
        by_conversation
            .entry(row.conversation_id)
            .or_default()
            .push(NearRow {
                id: row.id,
                source: row.source,
                is_from_me: row.is_from_me,
                // Outgoing rows have no sender of their own.
                sender_norm: if row.is_from_me != 0 {
                    String::new()
                } else {
                    row.sender_normalized
                },
                secs,
                body_norm: normalize_body(row.body.as_deref()),
                att_count: shas.len() as i64,
                att_fp: shas.join(","),
                content_key: row.content_key,
                whole_seconds: is_whole_seconds(row.id, &row.time_precision)?,
                run_messages: row.run_messages,
            });
    }
    Ok(by_conversation)
}

/// Walk each conversation in time order, gather each message's twins within
/// the window, and flag each cluster by the rule of the exact pass
/// ([`exact_group_flags`]): the cluster stays shown as many times as the
/// source that holds it most often. A cluster counts only when it spans two
/// sources: same-source near-duplicates are left alone. Returns
/// `(loser, winner)` pairs.
///
/// A cluster is a star around its first row, and a row from the first row's
/// own source is never its twin, so the first row's source counts once in
/// it however often that source holds the message. A row the cluster kept,
/// other than the winner, therefore stays free to start or join a later
/// cluster, where it can pair with that source's next copy. The winner and
/// the hidden rows join no other cluster: every hidden row points at a
/// winner, and a winner hidden later would leave a chain.
fn cluster_near_dupes(
    by_conversation: HashMap<i64, Vec<NearRow>>,
    prio: &HashMap<&str, usize>,
    window_secs: i64,
) -> Vec<(i64, i64)> {
    let mut clustered: HashSet<i64> = HashSet::new();
    let mut flags: Vec<(i64, i64)> = Vec::new();
    for mut rows in by_conversation.into_values() {
        rows.sort_by(|a, b| a.secs.cmp(&b.secs).then(a.id.cmp(&b.id)));
        for i in 0..rows.len() {
            let first = &rows[i];
            if clustered.contains(&first.id) {
                continue;
            }
            let cluster: Vec<Cand> = std::iter::once(first)
                .chain(
                    rows[i + 1..]
                        .iter()
                        .take_while(|row| row.secs - first.secs <= window_secs)
                        .filter(|row| first.is_twin_of(row) && !clustered.contains(&row.id)),
                )
                .map(NearRow::candidate)
                .collect();
            let sources: HashSet<&str> = cluster.iter().map(|c| c.source.as_str()).collect();
            if sources.len() < 2 {
                continue;
            }
            let cluster_flags = exact_group_flags(&cluster, prio);
            clustered.extend(
                cluster_flags
                    .iter()
                    .flat_map(|&(loser, winner)| [loser, winner]),
            );
            flags.extend(cluster_flags);
        }
    }
    flags
}

#[cfg(test)]
mod tests;
