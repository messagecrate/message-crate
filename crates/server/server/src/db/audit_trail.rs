//! The Audit Trail: what each user did on this Message Crate, and when.
//!
//! The record is three tables read as one list. `audit_entries` holds
//! everything that has no record of its own: sessions, refused logins, the
//! owner's and holders' changes to accounts, and the server closing an Export
//! Run nobody closed. Import Runs and Export Runs stay in `imports` and
//! `exports` and are not copied. [`page`] reads all three in
//! time order, and adds a `session_ended` entry with the reason `expired` for
//! each session that ran out with nothing to end it, so expiry needs no write.
//!
//! An entry is written once. No route changes or deletes one; the only
//! deletion is [`trim_refused_logins`], for refused logins as a username no
//! account holds, after [`UNKNOWN_USERNAME_RETENTION_DAYS`]. Deleting an
//! account keeps its entries and runs ([`prepare_account_deletion`]).
//! A password, a session token and an API token never enter the trail, and
//! nothing here says what a message said or which conversation it was in
//! (`docs/adr/0008-the-owner-holds-no-messages.md`,
//! `docs/adr/0020-the-audit-trail-outlives-the-account.md`).
//!
//! An entry keeps the id of the request that wrote it
//! ([`crate::request_id::current`]), so the owner finds that request's lines
//! in the server's log by it. An entry written outside a request, by a server
//! command or the first start's Demo Account build, keeps none (#2276).
//!
//! A refused login as a username no account holds keeps the username,
//! trimmed, only when `require_valid_username` accepts it
//! ([`record_refused_login`]), because a person whose browser fills the
//! wrong field types their password there. Anything else is recorded with no
//! username. A password that is also a valid username is still kept.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection};

use message_crate_api_types::{ExportQueryList, ExportStatus};

use crate::credentials::require_valid_username;
use crate::db::account_profile;
use crate::db::address_book::LoadMode;
use crate::db::demo_account_build::DemoDataSize;
use crate::db::exports::ExportScopeKind;
use crate::db::imports::ImportStatus;
use crate::db::permissions::Permission;
use crate::db::session_tokens::{AppKind, ConnectingApp};

/// Days a refused login as a username no account holds is kept. Such an
/// entry belongs to no one, so without a limit anyone who can reach the
/// server could grow the record forever.
pub const UNKNOWN_USERNAME_RETENTION_DAYS: i64 = 90;

/// What an entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// A login opened a Session: logging in, claiming the Message Crate, or
    /// registering.
    LoggedIn,
    /// A Session ended; `reason` says how.
    SessionEnded,
    /// A login was refused; `reason` says why.
    LoginRefused,
    /// An account was made.
    AccountCreated,
    /// The owner disabled the account.
    AccountDisabled,
    /// The owner re-enabled the account.
    AccountEnabled,
    /// The account's password was set or changed.
    PasswordSet,
    /// The owner changed the account's permissions.
    PermissionsChanged,
    /// Every message of the account was deleted for good.
    MessagesDeleted,
    /// One conversation was deleted for good from the trash.
    ConversationDeleted,
    /// The trash was emptied.
    TrashEmptied,
    /// The account was deleted.
    AccountDeleted,
    /// The owner let strangers create accounts.
    RegistrationOpened,
    /// The owner stopped strangers creating accounts.
    RegistrationClosed,
    /// The owner changed the attachment size limit; `asset_max_bytes` is the
    /// new limit.
    AssetLimitChanged,
    /// The Demo Account was added where there was none: a build started, by
    /// the owner, the `reset-demo` command, or the server's first start;
    /// `demo_data_size` is the set of Demo Data asked for.
    DemoAccountAdded,
    /// The Demo Account was reset: a build started, by the owner or the
    /// `reset-demo` command, that removes it, with all it holds, and builds it
    /// again; `demo_data_size` is the set of Demo Data asked for.
    DemoAccountReset,
    /// The holder made an API token.
    ApiTokenCreated,
    /// The holder deleted an API token.
    ApiTokenDeleted,
    /// The holder loaded an address book.
    AddressBookLoaded,
    /// The holder exported the address book.
    AddressBookExported,
    /// The server closed a running Export Run as `cancelled`, removing its
    /// list of messages; `reason` says why and `export_run_id` names the run.
    ExportRunClosed,
    /// An Import Run, read from `imports`.
    ImportRun,
    /// An Export Run, read from `exports`.
    ExportRun,
}

impl AuditAction {
    /// The value as the wire and the database spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoggedIn => "logged_in",
            Self::SessionEnded => "session_ended",
            Self::LoginRefused => "login_refused",
            Self::AccountCreated => "account_created",
            Self::AccountDisabled => "account_disabled",
            Self::AccountEnabled => "account_enabled",
            Self::PasswordSet => "password_set",
            Self::PermissionsChanged => "permissions_changed",
            Self::MessagesDeleted => "messages_deleted",
            Self::ConversationDeleted => "conversation_deleted",
            Self::TrashEmptied => "trash_emptied",
            Self::AccountDeleted => "account_deleted",
            Self::RegistrationOpened => "registration_opened",
            Self::RegistrationClosed => "registration_closed",
            Self::AssetLimitChanged => "asset_limit_changed",
            Self::DemoAccountAdded => "demo_account_added",
            Self::DemoAccountReset => "demo_account_reset",
            Self::ApiTokenCreated => "api_token_created",
            Self::ApiTokenDeleted => "api_token_deleted",
            Self::AddressBookLoaded => "address_book_loaded",
            Self::AddressBookExported => "address_book_exported",
            Self::ExportRunClosed => "export_run_closed",
            Self::ImportRun => "import_run",
            Self::ExportRun => "export_run",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
    }
}

/// Who acted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditActor {
    /// The owner, through Message Crate.
    Owner,
    /// The account's own holder: the person logged in to it, or a program
    /// holding one of its API tokens.
    Holder,
    /// A command run on the server, such as `reset-owner-password` or
    /// `reset-demo`.
    CommandLine,
    /// The server's first start on a new database, which adds the Demo
    /// Account before the server listens.
    StartUp,
    /// The server on its own: removing a Demo Account build that failed, or
    /// a session running out.
    Server,
    /// Someone at the login card, before any login: a refused login, or a
    /// stranger creating an account.
    Anonymous,
}

impl AuditActor {
    /// The value as the database spells it.
    fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Holder => "holder",
            Self::CommandLine => "command_line",
            Self::StartUp => "start_up",
            Self::Server => "server",
            Self::Anonymous => "anonymous",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "owner" => Self::Owner,
            "holder" => Self::Holder,
            "command_line" => Self::CommandLine,
            "start_up" => Self::StartUp,
            "anonymous" => Self::Anonymous,
            _ => Self::Server,
        }
    }

    /// The person logged in to `account_id` acting on it: the owner on the
    /// owner's account, its holder on any other.
    #[must_use]
    pub fn logged_in_as(account_id: i64) -> Self {
        if account_profile::is_server_owner(account_id) {
            Self::Owner
        } else {
            Self::Holder
        }
    }
}

/// How a Session ended, why a login was refused, or why the server closed an
/// Export Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditReason {
    /// `session_ended`: the holder logged out.
    LoggedOut,
    /// `session_ended`: a new login took the account's one Session.
    /// `export_run_closed`: a new Export took the account's one running
    /// Export Run.
    Replaced,
    /// `session_ended`: the server ended it, as `reset-owner-password` does.
    Revoked,
    /// `session_ended`: it ran out. Never stored: read from a `logged_in`
    /// entry with no end once its expiry has passed.
    Expired,
    /// `login_refused`: no account has the username.
    UnknownUsername,
    /// `login_refused`: the password was wrong.
    WrongPassword,
    /// `login_refused`: the password was right and the account is disabled.
    AccountDisabled,
    /// `export_run_closed`: no page of the run was read for
    /// [`crate::db::exports::IDLE_EXPORT_RUN_LIMIT`].
    Idle,
}

impl AuditReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::LoggedOut => "logged_out",
            Self::Replaced => "replaced",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::UnknownUsername => "unknown_username",
            Self::WrongPassword => "wrong_password",
            Self::AccountDisabled => "account_disabled",
            Self::Idle => "idle",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
    }
}

/// What started a run: a Session or an API token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunCredential {
    /// A logged-in Session; `app` and `app_build` name the app when the
    /// request did.
    Session,
    /// An API token; `api_token_label` and `api_token_hint` name it as it
    /// was when the run started.
    ApiToken,
}

/// How a run stands, as the run's own list spells it: an Import Run's
/// status, or an Export Run's, which never finishes with issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Still going.
    Running,
    /// Finished with nothing to report.
    Completed,
    /// An Import Run that finished, and some items were skipped or failed.
    CompletedWithIssues,
    /// Ended without finishing.
    Failed,
    /// Ended by the person, or by the server on their behalf.
    Cancelled,
}

impl From<ImportStatus> for RunStatus {
    fn from(status: ImportStatus) -> Self {
        match status {
            ImportStatus::Running => Self::Running,
            ImportStatus::Completed => Self::Completed,
            ImportStatus::CompletedWithIssues => Self::CompletedWithIssues,
            ImportStatus::Failed => Self::Failed,
            ImportStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<ExportStatus> for RunStatus {
    fn from(status: ExportStatus) -> Self {
        match status {
            ExportStatus::Running => Self::Running,
            ExportStatus::Completed => Self::Completed,
            ExportStatus::Failed => Self::Failed,
            ExportStatus::Cancelled => Self::Cancelled,
        }
    }
}

/// The credential a request came with, as the Audit Trail records it on a
/// run: never the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialUsed {
    /// A Session, and the app the request named, if it named one.
    Session(Option<ConnectingApp>),
    /// An API token's label and masked hint.
    ApiToken {
        /// The token's label.
        label: String,
        /// The token's masked hint, such as `mc-api-Sd..mE`.
        hint: String,
    },
}

/// A run row's columns that record what started it, as
/// [`CredentialUsed::run_columns`] fills them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunCredentialColumns<'a> {
    /// `credential`: `session` or `api_token`.
    credential: &'static str,
    /// `app_kind`: the app a Session's request named.
    app_kind: Option<&'a str>,
    /// `app_build`: that app's Build.
    app_build: Option<&'a str>,
    /// `api_token_label`: the token's label as it is now.
    api_token_label: Option<&'a str>,
    /// `api_token_hint`: the token's masked hint as it is now.
    api_token_hint: Option<&'a str>,
}

impl CredentialUsed {
    /// The run row's columns for this credential.
    fn run_columns(&self) -> RunCredentialColumns<'_> {
        match self {
            Self::Session(app) => RunCredentialColumns {
                credential: "session",
                app_kind: app.as_ref().map(|app| app.kind.as_str()),
                app_build: app.as_ref().map(|app| app.build.as_str()),
                api_token_label: None,
                api_token_hint: None,
            },
            Self::ApiToken { label, hint } => RunCredentialColumns {
                credential: "api_token",
                app_kind: None,
                app_build: None,
                api_token_label: Some(label.as_str()),
                api_token_hint: Some(hint.as_str()),
            },
        }
    }
}

/// The run table a run's row is in: `imports` for an Import Run, `exports`
/// for an Export Run. Both carry the same columns for what started the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTable {
    /// `imports`, one row per Import Run.
    Imports,
    /// `exports`, one row per Export Run.
    Exports,
}

impl RunTable {
    /// The table's name, as SQL names it.
    const fn name(self) -> &'static str {
        match self {
            Self::Imports => "imports",
            Self::Exports => "exports",
        }
    }
}

/// Record what started a run on its row: a Session and the app it named, or
/// an API token's label and hint as they are now. The Audit Trail reads these
/// columns from both run tables ([`page`]), so the one statement that writes
/// them is here too. `POST /v1/imports` and `POST /v1/exports` call it with
/// their own table.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn record_run_credential(
    conn: &mut SqliteConnection,
    table: RunTable,
    run_id: i64,
    credential: &CredentialUsed,
) -> Result<()> {
    let columns = credential.run_columns();
    let table = table.name();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET credential = $1, app_kind = $2, app_build = $3,
                api_token_label = $4, api_token_hint = $5
         WHERE id = $6"
    )))
    .bind(columns.credential)
    .bind(columns.app_kind)
    .bind(columns.app_build)
    .bind(columns.api_token_label)
    .bind(columns.api_token_hint)
    .bind(run_id)
    .execute(&mut *conn)
    .await
    .context("record what started the run")?;
    Ok(())
}

/// The counts and names an entry carries beyond who, what and when, stored as
/// `audit_entries.details`. Every field is optional; each action fills the
/// ones that describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Details {
    /// `permissions_changed`: permissions turned on (`import`, `export`, `delete`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions_added: Option<Vec<Permission>>,
    /// `permissions_changed`: permissions turned off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions_removed: Option<Vec<Permission>>,
    /// Conversations deleted for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversations: Option<i64>,
    /// Attachments deleted for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<i64>,
    /// Contacts: forgotten when the trash was emptied, or written to an
    /// exported address book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts: Option<i64>,
    /// Identities written to an exported address book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identities: Option<i64>,
    /// `address_book_loaded`: `append` or `edit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<LoadMode>,
    /// `address_book_loaded`: contacts the load made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_created: Option<i64>,
    /// `address_book_loaded`: contacts the load renamed or changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_updated: Option<i64>,
    /// `address_book_loaded`: contacts the load removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contacts_deleted: Option<i64>,
    /// `api_token_created` and `api_token_deleted`: the token's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_label: Option<String>,
    /// `api_token_created` and `api_token_deleted`: the token's masked hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_hint: Option<String>,
    /// `asset_limit_changed`: the new attachment size limit, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_max_bytes: Option<u64>,
    /// `demo_account_added` and `demo_account_reset`: the set of Demo Data the
    /// build was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo_data_size: Option<DemoDataSize>,
    /// `export_run_closed`: the Export Run the server closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_run_id: Option<i64>,
}

/// One entry of the Audit Trail as the interface hands it out: an entry of
/// `audit_entries`, an Import Run, or an Export Run. `id` is the row's id in
/// its own table, so `action` and `id` together name an entry. Every field is
/// sent, and the ones that do not describe the action are `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AuditEntry {
    /// The row's id: the entry's, the Import Run's or the Export Run's. For a
    /// session that expired, the id of its `logged_in` entry.
    pub id: i64,
    /// What happened.
    pub action: AuditAction,
    /// When, RFC 3339 UTC: the run's start for a run, the expiry for a
    /// session that expired.
    pub at: String,
    /// Who acted.
    pub actor: AuditActor,
    /// The account the entry is about, or `null` when it is about none, or
    /// the account has been deleted: `account_deleted` tells the two apart.
    pub account_id: Option<i64>,
    /// Whether the account the entry is about has since been deleted. `false`
    /// for an entry about no account, and for a refused login as a username
    /// no account holds.
    pub account_deleted: bool,
    /// The username of the account the entry is about, as it was; for a
    /// refused login as a username no account holds, the username as typed,
    /// or `null` when the text typed could not be a username. Kept after the
    /// account is deleted.
    pub username: Option<String>,
    /// The id of the request that wrote the entry, as its response's
    /// `x-request-id` and the request's lines in the server's log carry it.
    /// `null` for an entry written outside a request, by a server command or
    /// the Demo Account build of the server's first start, and for a run and
    /// a session that expired, which no one request wrote.
    pub request_id: Option<String>,
    /// `session_ended`: how the Session ended. `login_refused`: why.
    pub reason: Option<AuditReason>,
    /// The app the request named, `desktop` or `website`.
    pub app: Option<AppKind>,
    /// That app's Build, such as `0.9.0+343fe0d8`. `null` exactly when `app` is.
    pub app_build: Option<String>,
    /// A run: what started it. `null` for a run the server started itself.
    pub credential: Option<RunCredential>,
    /// The API token's label as it was then.
    pub api_token_label: Option<String>,
    /// The API token's masked hint as it was then, such as `mc-api-Sd..mE`.
    /// Only on the entries about the reader's own account: the owner never
    /// reads another account's.
    pub api_token_hint: Option<String>,
    /// `permissions_changed`: permissions turned on.
    pub permissions_added: Option<Vec<Permission>>,
    /// `permissions_changed`: permissions turned off.
    pub permissions_removed: Option<Vec<Permission>>,
    /// A run: its status as the run's own list spells it.
    pub status: Option<RunStatus>,
    /// An Import Run: the source it imported, such as `imessage`.
    pub source: Option<String>,
    /// An Export Run: `everything`, `query` or `selection`. Never the query
    /// or the picked ids.
    pub scope_kind: Option<ExportScopeKind>,
    /// An Export Run with a query: the list it was for, `conversations` or
    /// `messages`.
    pub scope_list: Option<ExportQueryList>,
    /// Messages a run accepted or matched.
    pub messages: Option<i64>,
    /// Conversations an Export Run matched, or deleted for good.
    pub conversations: Option<i64>,
    /// Attachments a run accepted or matched, or deleted for good.
    pub attachments: Option<i64>,
    /// Bytes an Import Run uploaded, or an Export Run's attachments total.
    pub bytes: Option<i64>,
    /// Contacts forgotten when the trash was emptied, or written to an
    /// exported address book.
    pub contacts: Option<i64>,
    /// Identities written to an exported address book.
    pub identities: Option<i64>,
    /// `address_book_loaded`: `append` or `edit`.
    pub mode: Option<LoadMode>,
    /// `address_book_loaded`: contacts made.
    pub contacts_created: Option<i64>,
    /// `address_book_loaded`: contacts renamed or changed.
    pub contacts_updated: Option<i64>,
    /// `address_book_loaded`: contacts removed.
    pub contacts_deleted: Option<i64>,
    /// `asset_limit_changed`: the new attachment size limit, in bytes.
    pub asset_max_bytes: Option<u64>,
    /// `demo_account_added` and `demo_account_reset`: the set of Demo Data the
    /// build was asked for.
    pub demo_data_size: Option<DemoDataSize>,
    /// `export_run_closed`: the id of the Export Run the server closed, the
    /// `id` of that run's own `export_run` entry.
    pub export_run_id: Option<i64>,
}

impl AuditEntry {
    /// An entry saying who did what to which account, and when, with every
    /// count and name left out.
    fn new(
        id: i64,
        action: AuditAction,
        at: String,
        actor: AuditActor,
        account_id: Option<i64>,
        account_deleted: bool,
        username: Option<String>,
    ) -> Self {
        Self {
            id,
            action,
            at,
            actor,
            account_id,
            account_deleted,
            username,
            request_id: None,
            reason: None,
            app: None,
            app_build: None,
            credential: None,
            api_token_label: None,
            api_token_hint: None,
            permissions_added: None,
            permissions_removed: None,
            status: None,
            source: None,
            scope_kind: None,
            scope_list: None,
            messages: None,
            conversations: None,
            attachments: None,
            bytes: None,
            contacts: None,
            identities: None,
            mode: None,
            contacts_created: None,
            contacts_updated: None,
            contacts_deleted: None,
            asset_max_bytes: None,
            demo_data_size: None,
            export_run_id: None,
        }
    }
}

/// An entry to write.
#[derive(Debug, Clone)]
pub struct NewEntry<'a> {
    /// What happened.
    pub action: AuditAction,
    /// Who acted.
    pub actor: AuditActor,
    /// The account it is about, or `None` when it is about no account.
    pub account_id: Option<i64>,
    /// That account's username; for a refused login as a username no account
    /// holds, the username as typed when it could be one.
    pub username: Option<&'a str>,
    /// `session_ended` and `login_refused`: how or why.
    pub reason: Option<AuditReason>,
    /// The app the request named.
    pub app: Option<&'a ConnectingApp>,
    /// Counts and names.
    pub details: Details,
}

impl<'a> NewEntry<'a> {
    /// An entry about no account, such as the owner opening registration.
    #[must_use]
    pub fn about_no_account(action: AuditAction, actor: AuditActor) -> Self {
        Self {
            action,
            actor,
            account_id: None,
            username: None,
            reason: None,
            app: None,
            details: Details::default(),
        }
    }

    /// An entry about `account`, with nothing else to say.
    #[must_use]
    pub fn about(action: AuditAction, actor: AuditActor, account: (i64, &'a str)) -> Self {
        Self {
            action,
            actor,
            account_id: Some(account.0),
            username: Some(account.1),
            reason: None,
            app: None,
            details: Details::default(),
        }
    }

    /// The same entry carrying `details`.
    #[must_use]
    pub fn with_details(mut self, details: Details) -> Self {
        self.details = details;
        self
    }
}

/// The time an entry is stamped with, `now` written as `imports.started_at`
/// is, so entries and runs sort together.
fn at_text(now: DateTime<Utc>) -> String {
    now.to_rfc3339()
}

/// Write one entry, stamped `now`, and return its id.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record(
    conn: &mut SqliteConnection,
    entry: &NewEntry<'_>,
    now: DateTime<Utc>,
) -> Result<i64> {
    insert(conn, entry, None, None, now).await
}

/// Write one entry about the account `account_id`, looking its username up,
/// stamped `now`. Nothing is written when the account does not exist.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_about(
    conn: &mut SqliteConnection,
    action: AuditAction,
    actor: AuditActor,
    account_id: i64,
    details: Details,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(username) = account_profile::username_for_account(conn, account_id).await? else {
        return Ok(());
    };
    record(
        conn,
        &NewEntry::about(action, actor, (account_id, &username)).with_details(details),
        now,
    )
    .await?;
    Ok(())
}

/// Record that `actor` is building the Demo Account with the `size` set of
/// Demo Data, before the build removes the Demo Account it replaces: the
/// owner's build from Owner Home, the `reset-demo` command and the build
/// `serve` runs on a new database all record it here, so every reset is
/// filed alike (#2977). It is stamped `now`.
///
/// The entry is `demo_account_reset` about the Demo Account when there is
/// one, so it stays in that account's record once the build deletes it. It
/// is `demo_account_added` about no account when there is none: an entry
/// with a username and no account reads as one whose account was deleted.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_demo_build(
    conn: &mut SqliteConnection,
    actor: AuditActor,
    size: DemoDataSize,
    now: DateTime<Utc>,
) -> Result<()> {
    let demo =
        account_profile::username_for_account(conn, account_profile::DEMO_ACCOUNT_ID).await?;
    let about = demo
        .as_deref()
        .map(|username| (account_profile::DEMO_ACCOUNT_ID, username));
    record(conn, &demo_build_entry(actor, size, about), now).await?;
    Ok(())
}

/// The id of the last Audit Trail entry, or 0 when there is none.
pub async fn last_audit_entry_id(conn: &mut SqliteConnection) -> Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM audit_entries")
        .fetch_one(&mut *conn)
        .await
        .context("read the last Audit Trail entry")
}

/// The entry that records a build of the Demo Account by `actor` with the
/// `size` set: `demo_account_reset` about the Demo Account `replaced`, and
/// `demo_account_added` about no account when there was none.
fn demo_build_entry(
    actor: AuditActor,
    size: DemoDataSize,
    replaced: Option<(i64, &str)>,
) -> NewEntry<'_> {
    let entry = match replaced {
        Some(account) => NewEntry::about(AuditAction::DemoAccountReset, actor, account),
        None => NewEntry::about_no_account(AuditAction::DemoAccountAdded, actor),
    };
    entry.with_details(Details {
        demo_data_size: Some(size),
        ..Details::default()
    })
}

async fn insert(
    conn: &mut SqliteConnection,
    entry: &NewEntry<'_>,
    session_entry_id: Option<i64>,
    session_expires_at: Option<&str>,
    now: DateTime<Utc>,
) -> Result<i64> {
    let details = (entry.details != Details::default())
        .then(|| serde_json::to_string(&entry.details))
        .transpose()?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO audit_entries (
            at, request_id, action, actor, account_id, username, reason, app_kind,
            app_build, session_entry_id, session_expires_at, details
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING id",
    )
    .bind(at_text(now))
    .bind(crate::request_id::current())
    .bind(entry.action.as_str())
    .bind(entry.actor.as_str())
    .bind(entry.account_id)
    .bind(entry.username)
    .bind(entry.reason.map(AuditReason::as_str))
    .bind(entry.app.map(|app| app.kind.as_str()))
    .bind(entry.app.map(|app| app.build.as_str()))
    .bind(session_entry_id)
    .bind(session_expires_at)
    .bind(details)
    .fetch_one(&mut *conn)
    .await
    .context("insert an Audit Trail entry")?;
    Ok(id)
}

/// The RFC 3339 form of a Unix-seconds expiry, as `audit_entries` stores it.
fn expiry_text(expires_at_unix: u64) -> String {
    let secs = i64::try_from(expires_at_unix).unwrap_or(i64::MAX);
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or(chrono::DateTime::<Utc>::MAX_UTC)
        .to_rfc3339()
}

/// Write the `logged_in` entry, stamped `now`, for a Session that expires at
/// `expires_at_unix`, and return its id for the session row to keep.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record_login(
    conn: &mut SqliteConnection,
    account: (i64, &str),
    app: Option<&ConnectingApp>,
    expires_at_unix: u64,
    now: DateTime<Utc>,
) -> Result<i64> {
    let entry = NewEntry {
        app,
        ..NewEntry::about(
            AuditAction::LoggedIn,
            AuditActor::logged_in_as(account.0),
            account,
        )
    };
    insert(conn, &entry, None, Some(&expiry_text(expires_at_unix)), now).await
}

/// Write the `password_set` entry, stamped `now`, for a holder changing their
/// own password. The change renews their Session, so when `renewed` names its
/// `logged_in` entry and new expiry, this entry links to that login and holds
/// the expiry: entries are never edited, so the read takes the latest expiry
/// a login's entries hold.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_own_password_change(
    conn: &mut SqliteConnection,
    account_id: i64,
    renewed: Option<(i64, u64)>,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(username) = account_profile::username_for_account(conn, account_id).await? else {
        return Ok(());
    };
    let entry = NewEntry::about(
        AuditAction::PasswordSet,
        AuditActor::logged_in_as(account_id),
        (account_id, &username),
    );
    let expiry = renewed.map(|(_, expires_at_unix)| expiry_text(expires_at_unix));
    insert(
        conn,
        &entry,
        renewed.map(|(login_entry_id, _)| login_entry_id),
        expiry.as_deref(),
        now,
    )
    .await?;
    Ok(())
}

/// Record that the Session whose `logged_in` entry is `login_entry_id` now
/// expires at `expires_at_unix`, because it was used. Not an entry: a
/// Session in use moves its expiry as often as once an hour, and the Audit
/// Trail reads the latest one only to say when the Session ran out.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn record_session_extended(
    conn: &mut SqliteConnection,
    login_entry_id: i64,
    expires_at_unix: u64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO session_extensions (login_entry_id, expires_at) VALUES ($1, $2)
         ON CONFLICT(login_entry_id) DO UPDATE SET expires_at = excluded.expires_at",
    )
    .bind(login_entry_id)
    .bind(expiry_text(expires_at_unix))
    .execute(&mut *conn)
    .await
    .context("record a Session's new expiry")?;
    Ok(())
}

/// Write the `session_ended` entry, stamped `now`, for the Session whose
/// `logged_in` entry is `login_entry_id`.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_session_end(
    conn: &mut SqliteConnection,
    login_entry_id: i64,
    reason: AuditReason,
    actor: AuditActor,
    now: DateTime<Utc>,
) -> Result<()> {
    let login: Option<(Option<i64>, Option<String>)> =
        sqlx::query_as("SELECT account_id, username FROM audit_entries WHERE id = $1")
            .bind(login_entry_id)
            .fetch_optional(&mut *conn)
            .await
            .context("read the login an ending Session belongs to")?;
    let Some((account_id, username)) = login else {
        return Ok(());
    };
    let entry = NewEntry {
        action: AuditAction::SessionEnded,
        actor,
        account_id,
        username: username.as_deref(),
        reason: Some(reason),
        app: None,
        details: Details::default(),
    };
    insert(conn, &entry, Some(login_entry_id), None, now).await?;
    Ok(())
}

/// Write the entry for a refused login, stamped `now`. `account` is the
/// account the username named, when it named one. Otherwise the entry
/// belongs to no account and keeps the username, trimmed, only when
/// `require_valid_username` accepts it; the module comment says why. It also
/// writes a warning to the server's log, with the account's id, the reason
/// and the app.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record_refused_login(
    conn: &mut SqliteConnection,
    typed_username: &str,
    account: Option<(i64, &str)>,
    reason: AuditReason,
    app: Option<&ConnectingApp>,
    now: DateTime<Utc>,
) -> Result<()> {
    // The owner's Logs panel shows what the Audit Trail shows one account at
    // a time. The line names the account by its id, never the username
    // typed: the log holds ids and outcomes only
    // (`docs/architecture/server-log.md`).
    tracing::warn!(
        account_id = account.map(|(id, _)| id),
        reason = reason.as_str(),
        app = app.map(|app| app.kind.as_str()),
        app_build = app.map(|app| app.build.as_str()),
        "A login was refused"
    );
    let typed = require_valid_username(typed_username).ok();
    let typed = typed.as_deref();
    let entry = NewEntry {
        action: AuditAction::LoginRefused,
        actor: AuditActor::Anonymous,
        account_id: account.map(|(id, _)| id),
        username: account.map_or(typed, |(_, username)| Some(username)),
        reason: Some(reason),
        app,
        details: Details::default(),
    };
    insert(conn, &entry, None, None, now).await?;
    Ok(())
}

/// Write the `export_run_closed` entry, stamped `now`, for the account's
/// Export Run `export_id`, which the server closed as `cancelled` for
/// `reason`: `replaced` when `actor` started a new Export, `idle` when no
/// page was read for an hour. Nothing is written when the account does not
/// exist.
///
/// # Errors
///
/// Returns an error when the lookup or the insert fails.
pub async fn record_export_run_closed(
    conn: &mut SqliteConnection,
    account_id: i64,
    export_id: i64,
    reason: AuditReason,
    actor: AuditActor,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(username) = account_profile::username_for_account(conn, account_id).await? else {
        return Ok(());
    };
    let entry = NewEntry {
        reason: Some(reason),
        ..NewEntry::about(AuditAction::ExportRunClosed, actor, (account_id, &username))
            .with_details(Details {
                export_run_id: Some(export_id),
                ..Details::default()
            })
    };
    insert(conn, &entry, None, None, now).await?;
    Ok(())
}

/// Delete the refused logins as a username no account held that are older
/// than [`UNKNOWN_USERNAME_RETENTION_DAYS`] at `now`. Run when a login is
/// handled, so the record of them stays bounded with no sweeper. Nothing else
/// in the Audit Trail is ever deleted.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn trim_refused_logins(conn: &mut SqliteConnection, now: DateTime<Utc>) -> Result<u64> {
    let cutoff = (now - chrono::Duration::days(UNKNOWN_USERNAME_RETENTION_DAYS)).to_rfc3339();
    let deleted = sqlx::query(
        "DELETE FROM audit_entries
         WHERE action = 'login_refused' AND reason = 'unknown_username' AND at < $1",
    )
    .bind(cutoff)
    .execute(&mut *conn)
    .await
    .context("trim refused logins for unknown usernames")?
    .rows_affected();
    Ok(deleted)
}

/// Make the account's record ready to outlive it, just before its row is
/// deleted, and write the `account_deleted` entry. Returns false, writing
/// nothing, when the account does not exist.
///
/// Its live Session ends as `revoked` by `actor`, so the login does not read
/// as live, then as expired, after the account is gone. Every entry and run
/// about it is marked with the `account_deleted` entry's id, which is how
/// the deleted account is read afterwards ([`Scope::DeletedAccount`]). Its
/// runs keep its username and lose what describes the person's messages
/// ([`crate::db::imports::detach_from_account`],
/// [`crate::db::exports::detach_from_account`]). The delete that follows
/// sets `account_id` NULL on the runs and entries. Everything it writes is
/// stamped `now`.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn prepare_account_deletion(
    tx: &mut crate::db::WriteTx<'_>,
    account_id: i64,
    actor: AuditActor,
    now: DateTime<Utc>,
) -> Result<bool> {
    let Some(username) = account_profile::username_for_account(tx, account_id).await? else {
        return Ok(false);
    };
    crate::db::session_tokens::revoke_account_sessions(tx, account_id, actor, now).await?;
    let deletion = record(
        tx,
        &NewEntry::about(AuditAction::AccountDeleted, actor, (account_id, &username)),
        now,
    )
    .await?;
    sqlx::query("UPDATE audit_entries SET deletion_entry_id = $2 WHERE account_id = $1")
        .bind(account_id)
        .bind(deletion)
        .execute(&mut **tx)
        .await
        .context("mark a deleted account's entries")?;
    let at = at_text(now);
    crate::db::imports::detach_from_account(tx, account_id, &username, deletion, &at).await?;
    crate::db::exports::detach_from_account(tx, account_id, &username, deletion, &at).await?;
    Ok(true)
}

/// Which entries a read covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every entry: the owner's Audit Trail.
    All,
    /// The entries about one account, whoever acted.
    Account(i64),
    /// The entries and runs of one deleted account, by the id of its
    /// `account_deleted` entry ([`DeletedAccount::id`]): what it did and
    /// what was done to it, and nothing of a later account given its
    /// username, nor the logins refused for that username once it was gone.
    DeletedAccount(i64),
}

/// When the Session a `logged_in` entry `l` opened runs out: the latest
/// expiry among the entry, the password changes that renewed it, and the
/// use that last moved it (`session_extensions`). Each is RFC 3339 UTC, so
/// the latest is the greatest as text.
macro_rules! session_expiry_sql {
    () => {
        "MAX(COALESCE((SELECT MAX(r.session_expires_at) FROM audit_entries r
                        WHERE r.session_entry_id = l.id AND r.action = 'password_set'),
                      l.session_expires_at),
             COALESCE((SELECT x.expires_at FROM session_extensions x
                        WHERE x.login_entry_id = l.id),
                      l.session_expires_at))"
    };
}

/// The `account_deleted` column of a row of `table`: whether the account the
/// row is about has since been deleted, which its deletion marks on every
/// such row by setting `deletion_entry_id`.
macro_rules! account_deleted_sql {
    ($table:literal) => {
        concat!($table, ".deletion_entry_id IS NOT NULL AS account_deleted")
    };
}

/// Where a row of [`sources_sql`] comes from, by the one-letter tag the
/// query gives it. The tags also break a tie between rows at the same time,
/// the greatest first: an expiry, then an Export Run, an Import Run, and an
/// entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// An entry of `audit_entries`.
    Entry,
    /// A Session that ran out, read from its `logged_in` entry.
    Expiry,
    /// An Import Run.
    ImportRun,
    /// An Export Run.
    ExportRun,
}

impl Source {
    const ALL: [Self; 4] = [Self::Entry, Self::Expiry, Self::ImportRun, Self::ExportRun];

    /// The tag the query gives the source's rows.
    const fn tag(self) -> &'static str {
        match self {
            Self::Entry => "e",
            Self::Expiry => "x",
            Self::ImportRun => "i",
            Self::ExportRun => "o",
        }
    }

    fn from_tag(tag: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.tag() == tag)
            .with_context(|| format!("unknown Audit Trail source {tag}"))
    }
}

/// The union of the trail's sources as
/// `(src, id, at, account_id, deletion_entry_id)` rows, `src` being a
/// [`Source`] tag.
fn sources_sql() -> String {
    format!(
        "
    SELECT '{entry}' AS src, id, at, account_id, deletion_entry_id FROM audit_entries
    UNION ALL
    SELECT '{expiry}', id, expires_at, account_id, deletion_entry_id FROM (
        SELECT l.id, l.account_id, l.deletion_entry_id, {expires_at} AS expires_at FROM audit_entries l
         WHERE l.action = 'logged_in'
           AND NOT EXISTS (SELECT 1 FROM audit_entries e
                            WHERE e.session_entry_id = l.id AND e.action = 'session_ended')
    ) WHERE expires_at <= $1
    UNION ALL
    SELECT '{import}', id, started_at, account_id, deletion_entry_id FROM imports
    UNION ALL
    SELECT '{export}', id, started_at, account_id, deletion_entry_id FROM exports",
        entry = Source::Entry.tag(),
        expiry = Source::Expiry.tag(),
        import = Source::ImportRun.tag(),
        export = Source::ExportRun.tag(),
        expires_at = session_expiry_sql!(),
    )
}

/// One page of the Audit Trail, newest first, and how many entries it holds
/// in all, read at `now`: a Session that runs out after `now` is not shown
/// as ended.
///
/// # Errors
///
/// Returns an error when a statement fails or a stored row cannot be read.
pub async fn page(
    conn: &mut SqliteConnection,
    scope: Scope,
    limit: usize,
    offset: usize,
    now: DateTime<Utc>,
) -> Result<(Vec<AuditEntry>, u64)> {
    let at = at_text(now);
    let (filter, id) = match scope {
        Scope::All => ("", None),
        Scope::Account(id) => ("WHERE account_id = $2", Some(id)),
        Scope::DeletedAccount(id) => ("WHERE deletion_entry_id = $2", Some(id)),
    };
    let sources = sources_sql();
    let count_sql = format!("SELECT COUNT(*) FROM ({sources}) {filter}");
    let mut count = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql)).bind(&at);
    if let Some(id) = id {
        count = count.bind(id);
    }
    let total = count
        .fetch_one(&mut *conn)
        .await
        .context("count the Audit Trail entries")?;

    let page_sql = format!(
        "SELECT src, id FROM ({sources}) {filter}
         ORDER BY at DESC, src DESC, id DESC LIMIT {limit} OFFSET {offset}"
    );
    let mut rows = sqlx::query_as::<_, (String, i64)>(sqlx::AssertSqlSafe(page_sql)).bind(&at);
    if let Some(id) = id {
        rows = rows.bind(id);
    }
    let rows = rows
        .fetch_all(&mut *conn)
        .await
        .context("read a page of the Audit Trail")?;

    let mut items = Vec::with_capacity(rows.len());
    for (src, id) in rows {
        let item = match Source::from_tag(&src)? {
            Source::Entry => load_entry(conn, id).await?,
            Source::Expiry => load_expiry(conn, id).await?,
            Source::ImportRun => load_import(conn, id).await?,
            Source::ExportRun => load_export(conn, id).await?,
        };
        if let Some(item) = item {
            items.push(item);
        }
    }
    Ok((items, u64::try_from(total).unwrap_or(0)))
}

/// A deleted account as the Audit Trail remembers it: the username its
/// entries and runs keep, and when it was deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct DeletedAccount {
    /// The id of its `account_deleted` entry, which every entry and run about
    /// the account carries once it is deleted. Two accounts deleted under one
    /// username have two ids.
    pub id: i64,
    /// The username the account had, which its entries and runs still carry.
    pub username: String,
    /// When it was deleted, RFC 3339 UTC.
    pub deleted_at: String,
}

/// One page of the deleted accounts whose entries the Audit Trail keeps, by
/// username A to Z and the latest deletion first under one username, and how
/// many there are in all.
///
/// # Errors
///
/// Returns an error when a statement fails.
pub async fn deleted_accounts_page(
    conn: &mut SqliteConnection,
    limit: usize,
    offset: usize,
) -> Result<(Vec<DeletedAccount>, u64)> {
    const DELETED: &str = "FROM audit_entries
         WHERE action = 'account_deleted' AND deletion_entry_id = id AND username IS NOT NULL";
    let total: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) {DELETED}")))
        .fetch_one(&mut *conn)
        .await
        .context("count deleted accounts")?;
    let rows: Vec<(i64, String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT id, username, at {DELETED}
         ORDER BY username COLLATE NOCASE, at DESC, id DESC LIMIT {limit} OFFSET {offset}"
    )))
    .fetch_all(&mut *conn)
    .await
    .context("list deleted accounts")?;
    let items = rows
        .into_iter()
        .map(|(id, username, deleted_at)| DeletedAccount {
            id,
            username,
            deleted_at,
        })
        .collect();
    Ok((items, u64::try_from(total).unwrap_or(0)))
}

/// An entry of `audit_entries` as the interface hands it out.
async fn load_entry(conn: &mut SqliteConnection, id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(concat!(
        "SELECT id, at, request_id, action, actor, account_id, ",
        account_deleted_sql!("audit_entries"),
        ", username, reason, app_kind, app_build, details
         FROM audit_entries WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .context("read an Audit Trail entry")?;
    let Some(row) = row else { return Ok(None) };
    let details: Option<String> = row.try_get("details")?;
    let details: Details = match details {
        Some(text) => serde_json::from_str(&text).context("audit entry details are not JSON")?,
        None => Details::default(),
    };
    let reason: Option<String> = row.try_get("reason")?;
    let action: String = row.try_get("action")?;
    let actor: String = row.try_get("actor")?;
    let app_kind: Option<String> = row.try_get("app_kind")?;
    let app_build: Option<String> = row.try_get("app_build")?;
    let app = app_kind.as_deref().and_then(AppKind::parse);
    let action = AuditAction::parse(&action)
        .with_context(|| format!("audit entry {id} has an unknown action {action}"))?;
    Ok(Some(AuditEntry {
        reason: reason.as_deref().and_then(AuditReason::parse),
        app,
        app_build: app.and(app_build),
        api_token_label: details.api_token_label,
        api_token_hint: details.api_token_hint,
        permissions_added: details.permissions_added,
        permissions_removed: details.permissions_removed,
        conversations: details.conversations,
        attachments: details.attachments,
        contacts: details.contacts,
        identities: details.identities,
        mode: details.mode,
        contacts_created: details.contacts_created,
        contacts_updated: details.contacts_updated,
        contacts_deleted: details.contacts_deleted,
        asset_max_bytes: details.asset_max_bytes,
        demo_data_size: details.demo_data_size,
        export_run_id: details.export_run_id,
        request_id: row.try_get("request_id")?,
        ..AuditEntry::new(
            row.try_get("id")?,
            action,
            row.try_get("at")?,
            AuditActor::parse(&actor),
            row.try_get("account_id")?,
            row.try_get("account_deleted")?,
            row.try_get("username")?,
        )
    }))
}

/// The `session_ended` entry read, not stored, for a Session that ran out:
/// its `logged_in` entry's account, at its expiry.
async fn load_expiry(conn: &mut SqliteConnection, login_id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(concat!(
        "SELECT l.id, ",
        session_expiry_sql!(),
        " AS at, l.account_id, ",
        account_deleted_sql!("l"),
        ", l.username FROM audit_entries l WHERE l.id = $1"
    ))
    .bind(login_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read the login of an expired Session")?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(AuditEntry {
        reason: Some(AuditReason::Expired),
        ..AuditEntry::new(
            row.try_get("id")?,
            AuditAction::SessionEnded,
            row.try_get("at")?,
            AuditActor::Server,
            row.try_get("account_id")?,
            row.try_get("account_deleted")?,
            row.try_get("username")?,
        )
    }))
}

/// The account a run is about, and the username to show: the account's while
/// it exists, the one written on the run once it is deleted.
const RUN_ACCOUNT_SQL: &str = concat!(
    "r.account_id, ",
    account_deleted_sql!("r"),
    ", COALESCE(a.username, r.username) AS username, r.credential, r.app_kind,
     r.app_build, r.api_token_label, r.api_token_hint"
);

/// Fill the fields every run has: whose it is and what started it.
fn run_entry(row: &sqlx::sqlite::SqliteRow, action: AuditAction) -> Result<AuditEntry> {
    let credential: Option<String> = row.try_get("credential")?;
    let credential = match credential.as_deref() {
        Some("session") => Some(RunCredential::Session),
        Some("api_token") => Some(RunCredential::ApiToken),
        _ => None,
    };
    let app_kind: Option<String> = row.try_get("app_kind")?;
    let app_build: Option<String> = row.try_get("app_build")?;
    let app = app_kind.as_deref().and_then(AppKind::parse);
    let actor = if credential.is_some() {
        let account_id: Option<i64> = row.try_get("account_id")?;
        account_id.map_or(AuditActor::Holder, AuditActor::logged_in_as)
    } else {
        AuditActor::Server
    };
    Ok(AuditEntry {
        app,
        app_build: app.and(app_build),
        credential,
        api_token_label: row.try_get("api_token_label")?,
        api_token_hint: row.try_get("api_token_hint")?,
        ..AuditEntry::new(
            row.try_get("id")?,
            action,
            row.try_get("started_at")?,
            actor,
            row.try_get("account_id")?,
            row.try_get("account_deleted")?,
            row.try_get("username")?,
        )
    })
}

/// An Import Run as an Audit Trail entry: its source, status and counts.
async fn load_import(conn: &mut SqliteConnection, id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT r.id, r.started_at, r.status, r.source, r.message_count, r.attachment_count,
                r.bytes_uploaded, {RUN_ACCOUNT_SQL}
         FROM imports r LEFT JOIN accounts a ON a.id = r.account_id
         WHERE r.id = $1"
    )))
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .context("read an Import Run for the Audit Trail")?;
    let Some(row) = row else { return Ok(None) };
    let status: String = row.try_get("status")?;
    let status = ImportStatus::parse(&status)
        .with_context(|| format!("imports.status holds unknown value '{status}'"))?;
    Ok(Some(AuditEntry {
        status: Some(status.into()),
        source: row.try_get("source")?,
        messages: row.try_get("message_count")?,
        attachments: row.try_get("attachment_count")?,
        bytes: row.try_get("bytes_uploaded")?,
        ..run_entry(&row, AuditAction::ImportRun)?
    }))
}

/// An Export Run as an Audit Trail entry: the kind of scope and how much it
/// matched, never the query or the picked ids.
async fn load_export(conn: &mut SqliteConnection, id: i64) -> Result<Option<AuditEntry>> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT r.id, r.started_at, r.status, r.scope_kind, r.scope_list, r.message_count,
                r.conversation_count, r.attachment_count, r.total_bytes, {RUN_ACCOUNT_SQL}
         FROM exports r LEFT JOIN accounts a ON a.id = r.account_id
         WHERE r.id = $1"
    )))
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .context("read an Export Run for the Audit Trail")?;
    let Some(row) = row else { return Ok(None) };
    let status: String = row.try_get("status")?;
    let status = ExportStatus::parse(&status)
        .with_context(|| format!("exports.status holds unknown value '{status}'"))?;
    let scope_kind: String = row.try_get("scope_kind")?;
    let scope_kind = ExportScopeKind::parse(&scope_kind)
        .with_context(|| format!("exports.scope_kind holds unknown value '{scope_kind}'"))?;
    let scope_list: Option<String> = row.try_get("scope_list")?;
    let scope_list = scope_list
        .map(|list| {
            ExportQueryList::parse(&list)
                .with_context(|| format!("exports.scope_list holds unknown value '{list}'"))
        })
        .transpose()?;
    Ok(Some(AuditEntry {
        status: Some(status.into()),
        scope_kind: Some(scope_kind),
        scope_list,
        messages: row.try_get("message_count")?,
        conversations: row.try_get("conversation_count")?,
        attachments: row.try_get("attachment_count")?,
        bytes: row.try_get("total_bytes")?,
        ..run_entry(&row, AuditAction::ExportRun)?
    }))
}

#[cfg(test)]
mod tests;
