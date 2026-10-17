//! Shared helpers for the server's own tests, one submodule for each kind,
//! all re-exported here so a test imports them from `test_support` alone:
//!
//! - `fixture`: [`TestFixture`], the empty database with the schema applied
//!   that every test starts from, and the accounts and sessions a test adds
//!   to it.
//! - `http`: the HTTP helpers. They start the real app through [`serve`],
//!   issue one request, read the whole response, and let the server drop;
//!   [`serve_router`], which [`serve`] calls, is the one place that binds a
//!   listener and spawns an app. Registering, logging in and the problem
//!   body of a failed request are here too.
//! - `seed`: rows and stored values written straight into the database or
//!   the asset store.
//! - `imports`: imports run on a connection, and the checks on what an
//!   import leaves behind.
//! - `import_runs`: an Import Run driven over HTTP one step at a time.
//! - `logs`: the server's log lines.
//! - `lines`: the JSON Lines a test imports.
//! - `client`: the HTTP client a test calls its own server with, shared
//!   with the tests under `tests/`.
//!
//! The HTTP helpers drive the whole stack over real HTTP, for tests that
//! check a route's contract end to end. The other helpers need no HTTP, and
//! tests that call a handler function or the server's code directly use them
//! too.

mod client;
mod fixture;
mod http;
mod import_runs;
mod imports;
mod lines;
mod logs;
mod seed;

pub use client::http_client;
pub use fixture::{
    PASSWORD, TestFixture, fixture_with_account, schema_pool, test_fixture,
    test_fixture_with_busy_timeout,
};
pub use http::{
    RegisteredAccount, TestServer, claim_as_owner, delete_json, delete_json_raw,
    delete_json_with_body, delete_raw, delete_status, delete_status_with_body,
    expect_internal_problem, expect_problem, expect_problem_for, expect_problem_response, get_json,
    get_raw, get_status, location, log_in, log_in_raw, login_status, patch_failure, patch_json,
    patch_json_raw, patch_status, post_created_json, post_json, post_json_raw, post_logged_out,
    post_raw, post_status, post_status_logged_out, problem, put_json, put_json_raw, put_raw,
    put_status, register_via_api, serve, serve_router,
};
pub use import_runs::{complete_import_run, post_import_batch, start_import_run};
pub use imports::{
    assert_every_person_is_on_a_contact, at_current_schema_version, import_ada_conversation,
    import_for_account, import_jsonl_text,
};
pub use lines::{
    ConversationHeaderLine, MessageLine, attachment, conversation_header, message_line,
};
pub use logs::{lines_written_by, log_event, small_log_files, write_log_line};
pub(crate) use seed::write_stored_original;
pub use seed::{
    ConversationRow, MessageRow, SeedConversation, SeedMessage, attach_stored_file, fake_sha256,
    fixed_instant, fixed_start_instant, insert_account_with_id, link_identity,
    routed_credential_instant, seed_conversation, seed_one_message, store_asset_max_bytes,
    stored_permissions, stored_time, unique_guid, with_trigger_lifted,
};

/// The permission bits of the file or directory at `path`, such as `0o600`,
/// for a test that checks what another user of the computer can read
/// (#2097).
#[cfg(unix)]
pub fn permission_bits(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("read the mode of {}: {e}", path.display()))
        .permissions()
        .mode()
        & 0o777
}
