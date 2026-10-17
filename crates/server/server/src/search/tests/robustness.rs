//! Truncated and corrupted queries must never panic the lexer, the parser or
//! the compiler: every query is either answered or refused in the `detail`
//! form, and an answered one runs on SQLite without an error (#2127).
//!
//! Every seed query is cut at every character boundary, from the front and
//! from the back, then mutated many times over by a fixed-seed generator, so
//! a failure names the seed, the case and the mutations that made it, and
//! reruns the same way every time.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::search::check_for_every_list;

const CASES_PER_SEED: u64 = 300;

const LISTS: [ListKind; 3] = [
    ListKind::Contacts,
    ListKind::Conversations,
    ListKind::Messages,
];

/// Queries that use every part of the language: fields with bare, quoted
/// and doubled-quote values, ranges, lists, groups, negation, operators,
/// prefixes, and text outside ASCII. Each names a list it compiles on.
fn seeds() -> Vec<(ListKind, &'static str)> {
    vec![
        (
            ListKind::Messages,
            r#"from:"Ana Lima" tag:Work -date:2019..2021 (avocado or "two words") not kind:group"#,
        ),
        (
            ListKind::Messages,
            "-(a or b) and c* size:>1mb attachment:image service:imessage,sms",
        ),
        (
            ListKind::Conversations,
            r#"title:"say ""hi"" now" participants:>=3 group:"Book Club" import:last"#,
        ),
        (
            ListKind::Contacts,
            "name:Zoë café \"crème brûlée\" 日本語* messages:>0 first-message:<2022-01-01",
        ),
        (
            ListKind::Messages,
            "party \u{1F389} \"\u{1F389} tonight\" -\u{1F389}* tag:\u{1F389}",
        ),
        (
            ListKind::Messages,
            "zero\u{200B}width \"a\u{200B}b\" body:\u{200B}x -in:\"\u{200B}\"",
        ),
        (
            ListKind::Messages,
            "http://example.com/a:b 5:30 o'brien e\u{301} שלום date:today..7d",
        ),
        (
            ListKind::Messages,
            "plain\0words deleted:yes unsent:no trashed:any filename:a_b%c\\d",
        ),
    ]
}

/// Characters that mean something to the lexer or the parser, and some
/// that take more than one byte.
const INTERESTING: &[char] = &[
    '"',
    '(',
    ')',
    '-',
    ':',
    '*',
    ',',
    '.',
    '<',
    '>',
    '=',
    ' ',
    '\t',
    '\0',
    '\\',
    '%',
    '_',
    '\'',
    'é',
    '\u{301}',
    '\u{200B}',
    '\u{1F389}',
    '日',
    'o',
    'r',
];

/// A small linear congruential generator: deterministic, no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A number in `0..n`; `n` must be above zero.
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).expect("below n")
    }
}

/// Apply one mutation to `chars` and describe it: delete, duplicate, swap or
/// replace one character, or insert one the lexer reads specially.
fn mutate(rng: &mut Lcg, chars: &mut Vec<char>) -> String {
    let pick = INTERESTING[rng.below(INTERESTING.len())];
    if chars.is_empty() {
        chars.push(pick);
        return format!("push {pick:?}");
    }
    let at = rng.below(chars.len());
    match rng.below(5) {
        0 => {
            let gone = chars.remove(at);
            format!("delete {gone:?} at {at}")
        }
        1 => {
            chars.insert(at, chars[at]);
            format!("duplicate {:?} at {at}", chars[at])
        }
        2 => {
            let other = rng.below(chars.len());
            chars.swap(at, other);
            format!("swap {at} and {other}")
        }
        3 => {
            let was = std::mem::replace(&mut chars[at], pick);
            format!("replace {was:?} at {at} with {pick:?}")
        }
        _ => {
            chars.insert(at, pick);
            format!("insert {pick:?} at {at}")
        }
    }
}

/// The text a panic carried.
fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "a panic with no message".into())
}

/// A broken rule of a refusal, if any: its sentence is in the `detail` form
/// ([`detail_form_problem`]), and its span is a range of whole characters of
/// the input.
fn refusal_problem(input: &str, e: &QueryError) -> Option<String> {
    if let Some(problem) = detail_form_problem(&e.message) {
        return Some(problem);
    }
    let span = &e.span;
    if span.start > span.end
        || span.end > input.len()
        || !input.is_char_boundary(span.start)
        || !input.is_char_boundary(span.end)
    {
        return Some(format!(
            "a refusal whose span {span:?} is not inside the {} bytes typed: {}",
            input.len(),
            e.message
        ));
    }
    None
}

/// Compile `input` on every list, then check it with
/// [`check_for_every_list`], each under `catch_unwind`. `Err` names a panic
/// or a broken rule; `Ok` holds the filter of each list that answers it.
fn compile_on_every_list(input: &str) -> Result<Vec<(ListKind, Filter)>, String> {
    let mut compiled = Vec::new();
    for list in LISTS {
        let result = catch_unwind(AssertUnwindSafe(|| {
            compile(CompileRequest {
                list,
                query: input,
                account_id: ACCOUNT,
                today: today(),
                zone: chrono_tz::UTC,
            })
        }))
        .map_err(|panic| format!("{list:?}: panicked: {}", panic_text(&*panic)))?;
        match result {
            Ok(filter) => {
                let placeholders = filter.where_sql().matches('?').count();
                if placeholders != filter.params().len() {
                    return Err(format!(
                        "{list:?}: {placeholders} placeholders for {} values",
                        filter.params().len()
                    ));
                }
                compiled.push((list, filter));
            }
            Err(e) => {
                if let Some(problem) = refusal_problem(input, &e) {
                    return Err(format!("{list:?}: {problem}"));
                }
            }
        }
    }
    let shared = catch_unwind(AssertUnwindSafe(|| check_for_every_list(input)))
        .map_err(|panic| format!("every list: panicked: {}", panic_text(&*panic)))?;
    if let Err(e) = shared {
        if let Some(problem) = refusal_problem(input, &e) {
            return Err(format!("every list: {problem}"));
        }
        // What every list refuses alike, no list may answer.
        if let Some((list, _)) = compiled.first() {
            return Err(format!(
                "refused for every list ({}) but {list:?} answers it",
                e.message
            ));
        }
    }
    Ok(compiled)
}

/// One query the sweep feeds, and how it was made from its seed.
struct Input {
    how: String,
    text: String,
}

/// Every input the sweep feeds.
fn inputs() -> Vec<Input> {
    let mut out = Vec::new();
    for (seed_index, (_, seed)) in seeds().into_iter().enumerate() {
        let chars: Vec<char> = seed.chars().collect();
        for len in 0..=chars.len() {
            out.push(Input {
                how: format!("seed {seed_index}: first {len} characters"),
                text: chars[..len].iter().collect(),
            });
            out.push(Input {
                how: format!("seed {seed_index}: from character {len}"),
                text: chars[len..].iter().collect(),
            });
        }
        for case in 0..CASES_PER_SEED {
            let mut rng = Lcg(((seed_index as u64) << 32) ^ case ^ 0x9e37_79b9_7f4a_7c15);
            let mut text = chars.clone();
            let steps = 1 + rng.below(4);
            let applied: Vec<String> = (0..steps).map(|_| mutate(&mut rng, &mut text)).collect();
            out.push(Input {
                how: format!("seed {seed_index}: case {case} [{}]", applied.join(", ")),
                text: text.into_iter().collect(),
            });
        }
    }
    out
}

#[test]
fn every_seed_compiles_before_it_is_cut_or_mutated() {
    for (list, seed) in seeds() {
        if let Err(e) = compile(CompileRequest {
            list,
            query: seed,
            account_id: ACCOUNT,
            today: today(),
            zone: chrono_tz::UTC,
        }) {
            panic!("{seed:?} on {list:?}: {}", e.message);
        }
    }
}

/// What running an answered query on SQLite showed.
#[derive(PartialEq, Eq)]
enum Ran {
    /// It ran without an error.
    Clean,
    /// It ran, and the messages page held a message found only by an
    /// earlier version.
    FoundByEarlierVersion,
}

/// Run a Messages `filter` as the messages route does, in its default
/// order, and also run the rank join over every message: SQLite reads the
/// rank join, and so asks the full-text index, only when a row reaches it,
/// and most swept queries match no message.
async fn run_on_messages_route(
    conn: &mut SqliteConnection,
    filter: &Filter,
) -> Result<Ran, String> {
    use crate::db::conversation_messages::{
        default_message_list_sort, load_message_list_page, ranked_messages_from_sql,
    };
    use crate::db::sql::SqlParam;
    let mut statements = Vec::new();
    if let Some(rank_query) = filter.rank_query() {
        statements.push((
            "the rank join",
            format!("SELECT m.id {}", ranked_messages_from_sql()),
            vec![SqlParam::Text(rank_query)],
        ));
    }
    if let Some((sql, params)) = filter.matching_earlier_version_ids() {
        statements.push((
            "the earlier-version match",
            sql.to_string(),
            params.to_vec(),
        ));
    }
    for (what, sql, params) in statements {
        sqlx::query_scalar_with::<_, i64, _>(sqlx::AssertSqlSafe(sql), bind_args(&params))
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| format!("{what}: {e}"))?;
    }
    let order = default_message_list_sort(!filter.ranked_terms().is_empty());
    let page = load_message_list_page(conn, filter, &order, 50, 0)
        .await
        .map_err(|e| format!("the messages page: {e:?}"))?;
    Ok(if page.iter().any(|m| m.matched_earlier_version) {
        Ran::FoundByEarlierVersion
    } else {
        Ran::Clean
    })
}

/// Every cut and mutated query is answered or refused in the `detail` form
/// on every list, never a panic, and every answer runs on SQLite: a panic
/// in the handler drops the connection, and a database error is an internal
/// error, so either one shows the person a failed search. The database
/// holds a contact and an edited message, so each list's filter is tested
/// against a row, and the messages page reads the earlier versions.
#[tokio::test]
async fn truncated_and_corrupted_queries_are_answered_or_refused() {
    let (pool, _dir) = crate::db::engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    crate::test_support::insert_account_with_id(&mut conn, ACCOUNT, "alice").await;
    let chat = handle(&mut conn, ACCOUNT, "+15555550150", "imessage").await;
    contact(&mut conn, ACCOUNT, "Zoë Lima", &[chat]).await;
    let conv = conversation(&mut conn, ACCOUNT, chat, "one-to-one", None, &[chat]).await;
    let body = "party \u{1F389} café o'brien";
    let edited = message(
        &mut conn,
        ACCOUNT,
        msg(conv, "2024-06-01T10:00:00Z", false, Some(chat), body),
    )
    .await;
    let mut tx = crate::db::begin_write(&mut conn).await.unwrap();
    sqlx::query(
        "INSERT INTO message_versions (message_id, part_index, text, edited_at)
         VALUES ($1, 0, 'avocado two words', NULL)",
    )
    .bind(edited)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    let mut cases = 0usize;
    let mut found_by_earlier_version = 0usize;
    for Input { how, text } in inputs() {
        if !seen.insert(text.clone()) {
            continue;
        }
        cases += 1;
        let compiled = match compile_on_every_list(&text) {
            Ok(compiled) => compiled,
            Err(e) => {
                failures.push(format!("{how}: {e}\n  input {text:?}"));
                continue;
            }
        };
        for (list, filter) in compiled {
            let ran = match list {
                ListKind::Messages => run_on_messages_route(&mut conn, &filter).await,
                _ => run_filter(&mut conn, list, &filter)
                    .await
                    .map(|_| Ran::Clean)
                    .map_err(|e| e.to_string()),
            };
            match ran {
                Ok(Ran::FoundByEarlierVersion) => found_by_earlier_version += 1,
                Ok(Ran::Clean) => {}
                Err(e) => failures.push(format!("{how}: {list:?}: {e}\n  input {text:?}")),
            }
        }
    }
    // Some queries must find the message by its earlier version alone, or
    // the statements that read earlier versions were never run.
    assert!(
        found_by_earlier_version > 0,
        "no query found the edited message by its earlier version"
    );
    assert!(
        failures.is_empty(),
        "{} of {cases} queries failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
