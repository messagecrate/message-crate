//! Schema and tenant data helpers (accounts, tokens, contacts) over SQLite.

pub mod account_profile;
pub mod address_book;
pub mod api_tokens;
pub mod attachment_versions;
pub mod audit_trail;
pub mod contacts;
pub mod conversation_messages;
pub mod conversations;
pub mod dedupe;
pub mod demo;
pub mod demo_account_build;
pub mod engine;
pub mod erase;
pub mod exports;
pub mod free_name;
pub mod handles;
pub mod identity_country;
pub mod import_contacts;
pub mod imports;
pub mod maintenance;
pub mod media_queue;
pub mod named_membership;
pub mod ownership;
pub mod participant_names;
pub mod participants;
pub mod permissions;
pub mod saved_searches;
pub mod schema;
pub mod server_settings;
pub mod session_tokens;
pub mod sql;
pub mod sqlite_functions;
pub mod staging;
pub mod storage;
pub mod trash;
#[cfg(test)]
pub mod write_guard;
pub mod write_tx;

pub use write_tx::{WriteTx, begin_write};

/// Why a list read for a query in the search language failed.
#[derive(Debug)]
pub enum SearchListError {
    /// The search language refused the query.
    Query(crate::search::QueryError),
    /// A statement failed.
    Internal(anyhow::Error),
}

impl std::fmt::Display for SearchListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query(err) => write!(f, "{err}"),
            Self::Internal(err) => write!(f, "{err:#}"),
        }
    }
}

impl From<crate::search::QueryError> for SearchListError {
    fn from(e: crate::search::QueryError) -> Self {
        Self::Query(e)
    }
}

impl From<anyhow::Error> for SearchListError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

impl From<sqlx::Error> for SearchListError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

/// `now` as a Unix second, the form the token tables and the Demo Account
/// build record store. An instant before 1970 reads as 0.
pub(crate) fn unix_secs(now: chrono::DateTime<chrono::Utc>) -> u64 {
    u64::try_from(now.timestamp()).unwrap_or(0)
}
