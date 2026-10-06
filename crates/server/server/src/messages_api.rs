//! `GET /v1/messages`: one row per message matching a query in the search
//! language, paged like every other list.
//!
//! This is a read route, not Export. Opening a conversation is a lookup by id
//! (`GET /v1/conversations/{id}/messages`); searching across messages is a
//! list with a query, and this is that list. The thread's find box uses it
//! with `in:#id`, so a find reaches every message in the conversation rather
//! than whatever page the browser happens to hold (#313).

use crate::extract::{Json, Path, Query};
use axum::extract::State;
use serde::Serialize;

use crate::db::conversation_messages::{
    DEFAULT_MESSAGE_SORT, MESSAGE_LIST_SORT_KEYS, Message, count_matching_messages,
    default_message_list_sort, load_message_list_page, load_messages, message_list_sort_text,
};
use crate::db::sql::SqlParam;
use crate::paging::{ListRequest, PageQuery};
use crate::search::parse::TextTerm;
use crate::server::{ApiError, AppState, FullAccess};

/// One page of the Messages list and how the server read its search: the
/// four keys of every page, and `search`, the one key a page carries beside
/// them (`docs/architecture/http-api.md`, "Lists").
// The four keys are written out rather than taken from `Page<Message>` with
// `#[serde(flatten)]`: utoipa describes a flattened field as an `allOf` of
// two schemas, which gives the page no `properties` of its own for the page
// rules (`openapi/document_rules.rs`) and the generated web types to read.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct ListMessagesResponse {
    /// The rows on this page.
    pub(crate) items: Vec<Message>,
    /// Rows matching the query across every page.
    pub(crate) total: u64,
    /// Page size used.
    pub(crate) limit: usize,
    /// Page offset used.
    pub(crate) offset: usize,
    /// How the server read `q` and `sort`: the order it applied and the
    /// free-text terms it ranks by. It describes the query, not the rows, so
    /// every page of one query carries the same value.
    pub(crate) search: MessageSearch,
}

/// A Messages search as the server read it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct MessageSearch {
    /// The order the page is in, spelled as `sort` takes it: the `sort` the
    /// request named, or with none, `relevance` when `terms` is not empty and
    /// `-date` (newest first) when it is.
    pub(crate) sort: String,
    /// The free-text terms the search ranks by, in the order they were typed:
    /// every word and quoted phrase of `q` that is not a field word and not
    /// behind `-` or `not`, alone or in a negated group. Empty when `q` has
    /// none, and then `relevance` is refused.
    pub(crate) terms: Vec<FreeTextTerm>,
}

/// One free-text term of a search: a word, or a quoted phrase.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct FreeTextTerm {
    /// The word, or the phrase without its quotes, as typed.
    pub(crate) text: String,
    /// True when the word ended in `*` and matches any word it begins; the
    /// `*` is not in `text`. Always false for a phrase.
    pub(crate) prefix: bool,
}

impl From<&TextTerm> for FreeTextTerm {
    fn from(term: &TextTerm) -> Self {
        match term {
            TextTerm::Term { text, prefix } => Self {
                text: text.clone(),
                prefix: *prefix,
            },
            TextTerm::Phrase(text) => Self {
                text: text.clone(),
                prefix: false,
            },
        }
    }
}

/// Compile a query against the Messages list of the search language.
///
/// # Errors
///
/// Returns a `422 Unprocessable Entity` search-query-invalid problem when the
/// query does not parse or uses a word the Messages list does not have.
pub(crate) fn message_filter(
    account_id: i64,
    query: &str,
    clock: (chrono_tz::Tz, chrono::NaiveDate),
) -> Result<crate::search::Filter, ApiError> {
    let (zone, today) = clock;
    Ok(crate::search::compile(crate::search::CompileRequest {
        list: crate::search::ListKind::Messages,
        query,
        account_id,
        today,
        zone,
    })?)
}

/// Messages matching `q`: the same rows an Export Run with a `query` scope
/// would hand over, behind a logged-in session with the list defaults and the
/// list's offset ceiling.
///
/// With no `sort`, the best match comes first when `q` has a free-text word
/// to rank by, and the newest message first when it has none. The page's
/// `search` says which order it applied and which terms it ranks by, so a
/// client never parses `q` itself.
///
/// `sort=relevance` puts the best match first, ranked by the full-text
/// index's `bm25()` on the query's free-text words: the words not behind `-`
/// or `not`. A query with no such word has nothing to rank by, and
/// `relevance` is then `validation-failed`. Ties, and a message found only by
/// an attachment's file name, which the index does not rank, follow by date,
/// newest first.
#[utoipa::path(
    get,
    path = "/v1/messages",
    tag = "Messages",
    security(("session" = [])),
    params(
        ("q" = Option<String>, Query, description = "Search query in the Messages list's words; empty matches every message"),
        ("limit" = Option<usize>, Query, description = "Page size, default 40, max 500"),
        ("offset" = Option<usize>, Query, description = "Page offset, max 50000"),
        ("sort" = Option<String>, Query, description = "`date`, `-date` or `relevance` (best match first; needs a free-text word in `q`). Default `relevance` when `q` has a free-text word, and `-date`, newest first, when it has none.")
    ),
    responses(
        (status = 200, body = ListMessagesResponse),
        crate::problem::openapi::SearchQueryInvalid
    )
)]
pub(crate) async fn list_messages(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Query(query): Query<PageQuery>,
) -> Result<Json<ListMessagesResponse>, ApiError> {
    let mut conn = state.db.acquire().await?;
    // No default here: which one applies depends on the query, which is
    // compiled only after the sort has been checked.
    let list = ListRequest::read(
        &mut conn,
        auth.account_id,
        query,
        &MESSAGE_LIST_SORT_KEYS,
        &[],
    )
    .await?;
    let filter = message_filter(auth.account_id, &list.q, list.clock)?;
    let order = if list.order.is_empty() {
        default_message_list_sort(filter.rank_query().is_some()).to_vec()
    } else {
        list.order
    };
    let items = load_message_list_page(
        &mut conn,
        &filter,
        &order,
        list.page.limit,
        list.page.offset,
    )
    .await?;
    let total = count_matching_messages(&mut conn, &filter).await?;
    Ok(Json(ListMessagesResponse {
        items,
        total,
        limit: list.page.limit,
        offset: list.page.offset,
        search: MessageSearch {
            sort: message_list_sort_text(&order),
            terms: filter
                .ranked_terms()
                .iter()
                .map(FreeTextTerm::from)
                .collect(),
        },
    }))
}

/// One message by id, in the shape the Messages list gives each row.
///
/// Read-only. A message is never written through this route: an import
/// writes messages, and trashing is a conversation operation
/// (`docs/architecture/http-api.md`, "Methods"). The id is enough: an id
/// names one message, and a lookup by id does not depend on how it was
/// found, so the route returns any message of the caller's account, one in
/// a trashed conversation and a duplicate included, and takes no `q`.
/// Another account's message is `404`.
#[utoipa::path(
    get,
    path = "/v1/messages/{id}",
    tag = "Messages",
    security(("session" = [])),
    params(("id" = i64, Path, description = "Message id")),
    responses(
        (status = 200, body = Message),
    )
)]
pub(crate) async fn get_message(
    State(state): State<AppState>,
    FullAccess(auth): FullAccess,
    Path(message_id): Path<i64>,
) -> Result<Json<Message>, ApiError> {
    let mut conn = state.db.acquire().await?;
    let mut items = load_messages(
        &mut conn,
        "m.account_id = ? AND m.id = ?",
        &[SqlParam::Int(auth.account_id), SqlParam::Int(message_id)],
        &DEFAULT_MESSAGE_SORT,
        1,
        0,
    )
    .await?;
    items
        .pop()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no message {message_id}")))
}

#[cfg(test)]
mod tests;
