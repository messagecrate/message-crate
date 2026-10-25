# Administrator Replaces Owner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the claimed Owner account with one fixed administrator account, `admin`, made with the database, set up by a password alone, with public registration removed, and the word "owner" gone from every place it meant the administrator.

**Architecture:** The server keeps the fixed-id design (account id 1) but inserts the row when the schema is made and reads "set up" as "id 1 has a password hash". One route (`POST /v1/server/setup`) and one command (`set-admin-password`) set that password. The web app renames its owner screens and hooks to admin, replaces the claim form with a password-only setup form, and drops the Create Account tab. Docs, ADRs, rules and the glossary follow the code.

**Tech Stack:** Rust (Axum, sqlx on SQLite, utoipa, clap), TypeScript (React 19, TanStack Query, Vitest), Astro Starlight docs, bash dev script.

**Spec:** `docs/superpowers/specs/2026-10-24-administrator-replaces-owner-design.md`

## Global Constraints

- The word "owner" stays where it means the backup's owner (`owner_identity`, `owner_handle_id`, `owner_display_name`, `owner_sender`, the WhatsApp exporter's `owner` module, `ix_messages_owner_handle_id`) or the operating-system file owner (`owner_only.rs`, `make_data_dir_owner_only`, `keep_data_dir_to_owner`, `file_io::owner_only`, `file_io::restrict_to_owner`, `tool_downloads.rs`). It also stays in `gh api repos/{owner}/{repo}` and in third-party license text. Every other "owner" that means the administrator goes.
- The administrator's username is `admin`, fixed; its account id is 1; its permissions are none; it holds no messages.
- Server states are exactly `setup` and `ready`.
- No backwards compatibility: no alias, no migration, no old route kept. The Schema Fingerprint changes and that is accepted.
- `web-next/` is untouched.
- Generated files are regenerated, never hand-edited: `docs/src/assets/openapi.json` (`cargo run -p message-crate-server -- dump-openapi --output docs/src/assets/openapi.json`), `web/src/lib/serverApi.types.ts` (`cd web && npm run gen:api`), `docs/src/content/docs/docs/developer/reference/errors/` (`cargo run -p message-crate-server -- dump-error-docs --output docs/src/content/docs/docs/developer/reference/errors`), `docs/src/content/docs/docs/developer/reference/server-cli.md` (`cargo run -p message-crate-server -- dump-cli-docs --output docs/src/content/docs/docs/developer/reference/server-cli.md`).
- Rust test modules over a few hundred lines live beside the source as `<module>/tests.rs`. Web tests never use `waitFor`, `findBy…` or timers (`web/src/test/README.md`).
- Prose follows `docs/agents/writing-style.md`: "directory" never "folder", American spelling, plain English.
- Commit messages are plain English and end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Work on the branch `administrator-replaces-owner`, which already holds the spec and this plan.

---

## File Structure

**Server, `crates/server/server/src/`**
- `db/account_profile.rs`: `ADMIN_ACCOUNT_ID`, `ADMIN_USERNAME`, `is_admin`, `admin_has_password`, `insert_admin_row_if_missing`; remove `is_claimed`, `OWNER_ACCOUNT_ID`, `is_server_owner`.
- `db/schema.rs`: `ensure_schema` inserts the `admin` row.
- `db/server_settings.rs`: drop `public_registration`.
- `db/audit_trail.rs`: `AuditActor::Admin` (`"admin"`), drop `RegistrationOpened`/`RegistrationClosed`.
- `credentials.rs`: `hash_admin_password`, `verify_login_password` gains a "no-password login allowed" flag.
- `session_api.rs`: pass that flag for the Demo Account only.
- `server_api.rs`: `ServerState::{Setup, Ready}`, `POST /v1/server/setup`, `SetUpServerRequest`, drop registration from settings.
- `accounts_api.rs`: admin-only `create_account`, `is_admin` on the profile, `Admin…` schema names.
- `auth.rs`: `AuthCapability::Admin`, `is_admin`, `require_admin`, extractor `Admin`.
- `api_error.rs`, `problem.rs`: `NotTheAdmin`/`not-the-admin`; drop `RegistrationClosed`.
- `admin_cli.rs` (renamed from `owner_cli.rs`): `set_admin_password`.
- `cli.rs`: `SetAdminPassword(SetAdminPasswordArgs)`; drop `CreateOwner`, `ResetOwnerPassword`.
- `openapi/credential_matrix.rs`: `Credential::Admin`, scope `admin`.
- `test_support/http.rs`: `log_in_as_admin` replaces `claim_as_owner`.
- `tests/common/setup.rs` (renamed from `claim.rs`).
- `reset_demo.rs`, `startup.rs`, every other file: comments say administrator.

**Schema, `schema/sql/accounts.sql`**: comments; drop `public_registration`; actor list.

**Web, `web/src/`**
- `screens/AdminHome.tsx` + `screens/admin/` (renamed from Owner).
- `hooks/useIsAdmin.ts`, `hooks/useAdminAccounts.ts`.
- `screens/auth/SetupForm.tsx` (replaces `ClaimForm.tsx`); `LocalAuthTabs.tsx` two states; delete `CreateAccountForm.tsx`.
- `lib/serverApi.ts`: `setUpServer`; `lib/queryKeys.ts`: `adminAccounts`; `lib/auditTrail.ts`; `lib/desktop/runLog.ts`: `admin` flag.
- `App.tsx`: route `/admin`.

**Desktop, `src-tauri/src/`**: `run_logs.rs` and `commands/run_logs.rs` word changes and the `admin` flag.

**Scripts**: `scripts/run-dev.sh` (`--admin`), `docs/scripts/user-guide-screenshots.cjs`.

**Docs**: `CONTEXT.md`, `CLAUDE.md`, `AGENTS.md`, `README.md`, `CHANGELOG.md`, `docker/compose*.yml`, `config/config.toml.example`, `docs/adr/0008-*`, `docs/adr/{0002,0016,0018,0020}`, `docs/architecture/**`, `docs/src/content/docs/docs/user/features/admin/` (renamed), `docs/src/content/docs/docs/user/your-messages/set-up-the-administrator-and-create-an-account.md`, `docs/astro.config.mjs`.

---

### Task 1: The admin row is made with the database

**Files:**
- Modify: `crates/server/server/src/db/account_profile.rs:569-588` (constants and `is_claimed`), `:957-990` (`insert_account_at` comment and `granted`)
- Modify: `crates/server/server/src/db/schema.rs:439-449` (`ensure_schema`)
- Modify: `schema/sql/accounts.sql:1-5,26-29,37-38`
- Test: `crates/server/server/src/db/schema/tests.rs`, `crates/server/server/src/db/account_profile.rs` (inline tests)

**Interfaces:**
- Produces: `pub const ADMIN_ACCOUNT_ID: i64 = 1;`, `pub const ADMIN_USERNAME: &str = "admin";`, `pub fn is_admin(account_id: i64) -> bool`, `pub async fn admin_has_password(conn: &mut SqliteConnection) -> Result<bool>`, `pub async fn insert_admin_row_if_missing(conn: &mut SqliteConnection) -> Result<()>`.
- Removes: `OWNER_ACCOUNT_ID`, `is_server_owner`, `is_claimed`. Every later task uses the new names; the compiler lists the callers (`cargo check -p message-crate-server` will fail until Tasks 2 to 6 are done, so this task's tests are run by name with `--no-fail-fast` only once the crate compiles; do the rename of callers mechanically in this task to keep the crate compiling: `sed -i 's/OWNER_ACCOUNT_ID/ADMIN_ACCOUNT_ID/g; s/is_server_owner/is_admin/g' $(grep -rl 'OWNER_ACCOUNT_ID\|is_server_owner' crates/server/server/src)` and replace `is_claimed` with `admin_has_password` the same way).

- [ ] **Step 1: Write the failing tests**

In `crates/server/server/src/db/schema/tests.rs`, add after `fresh_database_has_complete_current_schema`:

```rust
/// A database made by `ensure_schema` holds the administrator's row at id 1,
/// named `admin`, with no password and no permission, so a new Message Crate
/// is in setup rather than empty.
#[tokio::test]
async fn a_fresh_database_holds_the_admin_row_without_a_password() {
    let pool = crate::test_support::fresh_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let row: (String, Option<String>, i64, i64, i64) = sqlx::query_as(
        "SELECT username, password_hash, can_import, can_export, can_delete
         FROM accounts WHERE id = $1",
    )
    .bind(crate::db::account_profile::ADMIN_ACCOUNT_ID)
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(row, ("admin".to_string(), None, 0, 0, 0));
    assert!(!crate::db::account_profile::admin_has_password(&mut conn).await.unwrap());
}

/// Running `ensure_schema` twice leaves one admin row, and leaves a password
/// it has been given alone.
#[tokio::test]
async fn re_ensuring_the_schema_keeps_the_admin_password() {
    let pool = crate::test_support::fresh_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    crate::db::account_profile::update_password_hash(
        &mut conn,
        crate::db::account_profile::ADMIN_ACCOUNT_ID,
        Some("$argon2id$fake"),
    )
    .await
    .unwrap();
    crate::db::schema::ensure_schema(&mut conn).await.unwrap();
    assert!(crate::db::account_profile::admin_has_password(&mut conn).await.unwrap());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE username = 'admin'")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
```

If `test_support::fresh_pool` does not exist under that name, use whatever helper `fresh_database_has_complete_current_schema` uses to get a pool on a new database, and keep the assertions.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p message-crate-server a_fresh_database_holds_the_admin_row -- --nocapture`
Expected: compile error, `ADMIN_ACCOUNT_ID` and `admin_has_password` not found.

- [ ] **Step 3: Implement the constants and the row**

In `db/account_profile.rs`, replace lines 569 to 588 (the `OWNER_ACCOUNT_ID` block through `is_claimed`) with:

```rust
/// Stable id for the administrator. There is exactly one, and this id is
/// what makes "one" structural: there is no flag to set, no second
/// administrator to create, and nothing to promote. The row is written when
/// the database is made ([`insert_admin_row_if_missing`]), so it always
/// exists; a Message Crate whose administrator has no password yet is in
/// setup. See `docs/adr/0008-the-administrator-holds-no-messages.md`.
pub const ADMIN_ACCOUNT_ID: i64 = 1;

/// The administrator's username. Fixed here and never read from the row,
/// like [`DEMO_USERNAME`], so every Message Crate logs its administrator in
/// as `admin` and no other account can take the name.
pub const ADMIN_USERNAME: &str = "admin";

/// True when `account_id` is the administrator.
pub fn is_admin(account_id: i64) -> bool {
    account_id == ADMIN_ACCOUNT_ID
}

/// True when the administrator has a password: this Message Crate is ready.
/// False while it is in setup.
pub async fn admin_has_password(conn: &mut SqliteConnection) -> Result<bool> {
    let hash: Option<Option<String>> =
        sqlx::query_scalar("SELECT password_hash FROM accounts WHERE id = $1")
            .bind(ADMIN_ACCOUNT_ID)
            .fetch_optional(&mut *conn)
            .await
            .context("read whether the administrator has a password")?;
    Ok(matches!(hash, Some(Some(hash)) if !hash.is_empty()))
}

/// Write the administrator's row at [`ADMIN_ACCOUNT_ID`] when it is not
/// there: username `admin`, no password, and 0 for the three permission
/// columns, because [`load_account_auth`] gives the administrator none from
/// its id and the row must not say otherwise. `ensure_schema` calls this, so
/// the row exists in every Message Crate database.
pub async fn insert_admin_row_if_missing(conn: &mut SqliteConnection) -> Result<()> {
    sqlx::query(
        "INSERT INTO accounts (id, username, password_hash, can_import, can_export, can_delete)
         VALUES ($1, $2, NULL, 0, 0, 0)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(ADMIN_ACCOUNT_ID)
    .bind(ADMIN_USERNAME)
    .execute(&mut *conn)
    .await
    .context("write the administrator's row")?;
    Ok(())
}
```

In `db/schema.rs`, `ensure_schema`, after the `message_crate` insert and before `Ok(())`:

```rust
    crate::db::account_profile::insert_admin_row_if_missing(conn).await?;
```

Update `insert_account_at`'s doc comment (lines 963-968) to say "the administrator's row holds 0 …" and keep `let granted = i32::from(!is_admin(id));`. Update the comment in `insert_account` (lines 936-937) to "1 is the administrator". Update `FIRST_GENERATED_ACCOUNT_ID`'s comment to name `ADMIN_ACCOUNT_ID`.

Run the mechanical rename from the Interfaces block so the crate compiles.

- [ ] **Step 4: Update `schema/sql/accounts.sql` comments**

Line 1: `-- Login account (web UI + API).` Lines 3-5: "the administrator is 1 and the demo account 2". Lines 26-29: "never decide the administrator's or the Demo Account's permissions … the administrator's holds 0 for all three". Lines 37-38: "(login, setting up the administrator)" instead of "(login, claiming Message Crate, or registering)". Line 115 (`server_settings` comment): "so the administrator reads and writes it without an id". Lines 120-123: delete the `public_registration` column and its comment, and reword the asset limit comment to "until the administrator sets it". Line 443: `-- Who acted: admin, holder …`; line 438-439: remove `registration_opened, registration_closed` from the list; line 450: remove "opening and closing registration".

Then `cargo test -p message-crate-server fresh_database_has_complete_current_schema`: the schema contract test and `tests/fixtures/schema/current-schema.json` may need `node scripts/sync-schema.mjs` (read the script's header for its use) and the `schema_column_comments` integration test will tell you if a comment rule is broken.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p message-crate-server -- schema::tests account_profile`
Expected: PASS, including `the_owner_row_holds_no_permission_and_other_rows_hold_all_three` once renamed to `the_admin_row_holds_no_permission_and_other_rows_hold_all_three`.

- [ ] **Step 6: Commit**

```bash
git add schema/sql/accounts.sql crates/server/server/src/db tests/fixtures/schema
git commit -m "Make the admin row with the database, and drop public registration from the schema

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Only the Demo Account logs in without a password

> Amended 2026-10-25: the maintainer ruled the other way on the pull request.
> The administrator alone must have a password; every other account with none
> logs in with an empty password, the Demo Account among them. The login
> passes `!is_admin(account_id)` as `no_password_login`. Read this task as
> history, not as the rule.

**Files:**
- Modify: `crates/server/server/src/credentials.rs:200-230`
- Modify: `crates/server/server/src/session_api.rs:255-262`
- Test: `crates/server/server/src/session_api/tests.rs`

**Interfaces:**
- Produces: `pub(crate) async fn verify_login_password(password_hash: Option<&str>, password: &str, no_password_login: bool) -> Result<bool, ApiError>` and `pub(crate) async fn hash_admin_password(password: &str) -> Result<String, ApiError>` (renamed from `hash_owner_password`; message "the administrator must have a password").

- [ ] **Step 1: Write the failing tests**

In `session_api/tests.rs` add:

```rust
/// Before setup the administrator has no password, and that is not a way
/// in: a login as `admin` with an empty password is refused like a wrong
/// one. Only the Demo Account enters without a password.
#[tokio::test]
async fn the_admin_cannot_log_in_with_an_empty_password_before_setup() {
    let state = test_fixture().await.state;
    assert_eq!(login_status(&state, "admin", "").await, StatusCode::UNAUTHORIZED);
    let entries = crate::test_support::refused_logins(&state).await;
    assert_eq!(entries.last().unwrap()["reason"], "wrong_password");
}
```

If no `refused_logins` helper exists, assert the status alone, then add a second assertion through the Audit Trail route once the admin has a password (Task 4 provides `log_in_as_admin`); keep the test minimal and honest.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p message-crate-server the_admin_cannot_log_in_with_an_empty_password_before_setup`
Expected: FAIL, status 200 (or 201) instead of 401, because a NULL hash accepts an empty password.

- [ ] **Step 3: Implement**

In `credentials.rs`, change `verify_login_password`:

```rust
/// Check `password` against `password_hash`. A `None` or empty hash means
/// the account has no password: it accepts an empty password only when
/// `no_password_login` is true, which the login sets for the Demo Account
/// alone. Argon2 runs against a dummy hash in that branch too, so the time a
/// refusal takes does not say whether the account has a password.
pub(crate) async fn verify_login_password(
    password_hash: Option<&str>,
    password: &str,
    no_password_login: bool,
) -> Result<bool, ApiError> {
    let (hash, password) = (password_hash.map(str::to_owned), password.to_owned());
    run_argon2("password check", move || {
        Ok(match hash.as_deref() {
            None | Some("") => {
                let _ = verify_password(dummy_password_hash(), &password);
                no_password_login && password.is_empty()
            }
            Some(hash) => verify_password(hash, &password),
        })
    })
    .await
}
```

Rename `hash_owner_password` to `hash_admin_password` with the message `"the administrator must have a password"`.

In `session_api.rs` line 262:

```rust
    let verified = verify_login_password(
        password_hash.as_deref(),
        password,
        account_profile::is_demo_account(account_id),
    )
    .await?;
```

and the unknown-username call becomes `verify_login_password(None, password, false)`. Fix every other caller the compiler names (tests pass `false`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p message-crate-server session_api credentials`
Expected: PASS, including the existing demo login tests.

- [ ] **Step 5: Commit**

```bash
git add crates/server/server/src/credentials.rs crates/server/server/src/session_api.rs crates/server/server/src/session_api/tests.rs
git commit -m "Let only the Demo Account log in without a password

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Server state, setup route, and settings without registration

**Files:**
- Modify: `crates/server/server/src/server_api.rs:1-10,53-130,160-260,295-410`
- Modify: `crates/server/server/src/db/server_settings.rs` (drop `public_registration`, `set_public_registration`)
- Modify: `crates/server/server/src/db/audit_trail.rs:82-84,130-131,151-190`
- Modify: `crates/server/server/src/api_error.rs:69-71,174-175`, `crates/server/server/src/problem.rs:128-135,234-239`
- Modify: `crates/server/server/src/lib.rs` or wherever routes are registered (`grep -n 'server/claim' crates/server/server/src/*.rs`)
- Test: `crates/server/server/src/server_api/tests.rs`

**Interfaces:**
- Produces: `ServerState::{Setup, Ready}` serialized `setup`/`ready`; `pub struct SetUpServerRequest { pub password: String }`; handler `set_up_server`; `AuditActor::Admin` serialized `"admin"`; `ApiError::NotTheAdmin`, `ProblemType::NotTheAdmin` (`not-the-admin`, 403, "Not the administrator").
- Removes: `ClaimServerRequest`, `claim_server`, `UpdateServerSettingsRequest.public_registration`, `ServerSettings.public_registration`, `AuditAction::RegistrationOpened/Closed`, `ApiError::RegistrationClosed`, `ProblemType::RegistrationClosed`.

- [ ] **Step 1: Rewrite the state tests**

Replace in `server_api/tests.rs` the tests `an_unowned_server_reports_unclaimed`, `public_registration_does_not_make_an_unowned_server_open`, `a_claimed_server_is_closed_until_registration_is_opened`, `a_claimed_server_with_registration_on_is_open`, `registration_is_refused_while_the_server_is_closed`, `the_owner_can_open_and_close_registration`, and the `close_registration` helper with:

```rust
/// A new Message Crate is in setup: its administrator exists but has no
/// password, so the login screen offers setting one and nothing else.
#[tokio::test]
async fn a_new_server_reports_setup() {
    let state = test_fixture().await.state;
    let body = get_json(&state, "/v1/server", None).await;
    assert_eq!(body["state"], "setup");
}

/// Once the administrator has a password the Message Crate is ready, and
/// stays ready: there is no other state.
#[tokio::test]
async fn a_set_up_server_reports_ready() {
    let state = test_fixture().await.state;
    let _admin = log_in_as_admin(&state, "Admin-Pw-7q2Lx9Vb").await;
    let body = get_json(&state, "/v1/server", None).await;
    assert_eq!(body["state"], "ready");
}

/// Setup sets the administrator's password and signs the person in as
/// `admin`, answering the Session as the resource it made.
#[tokio::test]
async fn setting_up_the_server_gives_admin_a_password_and_signs_them_in() {
    let state = test_fixture().await.state;
    let (status, headers, body) = post_json_raw(
        &state,
        "/v1/server/setup",
        None,
        &serde_json::json!({ "password": "Admin-Pw-7q2Lx9Vb" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers.get("location").unwrap(), "/v1/session");
    assert_eq!(body["account_id"], crate::db::account_profile::ADMIN_ACCOUNT_ID);
    assert_eq!(body["username"], "admin");
    assert!(body["token"].as_str().is_some());
    assert_eq!(login_status(&state, "admin", "Admin-Pw-7q2Lx9Vb").await, StatusCode::OK);
}

/// A second setup is refused: the password is set once here, and changed
/// afterwards by the administrator under Settings or by
/// `set-admin-password`.
#[tokio::test]
async fn a_server_can_only_be_set_up_once() {
    let state = test_fixture().await.state;
    let _admin = log_in_as_admin(&state, "Admin-Pw-7q2Lx9Vb").await;
    let (status, _, body) = post_json_raw(
        &state,
        "/v1/server/setup",
        None,
        &serde_json::json!({ "password": "another" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    expect_problem(&body, ProblemType::StateConflict);
    assert_eq!(login_status(&state, "admin", "Admin-Pw-7q2Lx9Vb").await, StatusCode::OK);
}

/// Setup needs a password of one character or more.
#[tokio::test]
async fn setup_needs_a_password_of_one_character_or_more() {
    let state = test_fixture().await.state;
    let (status, _, _) = post_json_raw(
        &state,
        "/v1/server/setup",
        None,
        &serde_json::json!({ "password": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(get_json(&state, "/v1/server", None).await["state"], "setup");
}
```

Use the raw/JSON helpers `test_support` already exports (`get_json`, `post_json_raw`, `expect_problem`, `login_status`); adapt names to the ones that exist. Rename `claiming_an_unowned_server_creates_the_owner_and_signs_them_in`, `a_server_can_only_be_claimed_once`, `a_claim_that_loses_a_race_answers_conflict` (keep as a race of two setups), `claiming_needs_a_password_of_one_character_or_more`, `claiming_the_server_creates_exactly_one_owner` (now: setup adds no account row, `count_accounts` is unchanged), `the_claimed_owner_row_holds_no_permission`, `the_owner_owes_no_profile_setup`, and delete `a_claim_under_a_taken_username_answers_username_taken_as_the_reference_says` (no username). Rename every `the_owner_…` test to `the_admin_…`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p message-crate-server server_api::tests`
Expected: compile errors for `log_in_as_admin` (Task 4 adds it; add a temporary local helper now that calls the setup route and returns the token, then delete it in Task 4), and 404 on `/v1/server/setup`.

- [ ] **Step 3: Implement the state and the setup route**

In `server_api.rs` replace `ServerState`:

```rust
/// What state a Message Crate is in, from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServerState {
    /// The administrator has no password yet. The only thing to do is set
    /// one (`POST /v1/server/setup`).
    Setup,
    /// The administrator has a password. Log in.
    Ready,
}
```

`Server.state` doc: "`setup` shows the Set Up card alone; `ready` shows Login." Replace `ClaimServerRequest` with:

```rust
/// Body for setting up a Message Crate: the administrator's first password.
// `max_length` takes only a literal; `the_reference_states_the_password_cap`
// holds each to `MAX_PASSWORD_CHARS`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetUpServerRequest {
    /// Password for `admin`: one character or more, at most 128 characters,
    /// counted as characters rather than bytes and not trimmed. Empty or
    /// over 128 is refused with `422 Unprocessable Entity`.
    #[schema(max_length = 128)]
    pub password: String,
}
```

`state_on_conn`:

```rust
async fn state_on_conn(conn: &mut sqlx::SqliteConnection) -> Result<ServerState, ApiError> {
    Ok(if account_profile::admin_has_password(conn).await? {
        ServerState::Ready
    } else {
        ServerState::Setup
    })
}
```

Replace `claim_server` with:

```rust
/// Set up a Message Crate by giving its administrator a password.
///
/// Unauthenticated, because a Message Crate in setup has no credential that
/// could authorize this: `admin` exists but cannot log in. Whoever reaches
/// one in setup first sets the password: Message Crate is self-hosted, so
/// whoever installs the software sets it up and then publishes the port, in
/// that order and at times of their choosing. One in setup holds no
/// messages, so a lost race destroys nothing and announces itself at once.
#[utoipa::path(
    post,
    path = "/v1/server/setup",
    tag = "Server",
    request_body = SetUpServerRequest,
    responses(
        (
            status = 201,
            description = "Set up; the administrator's Session is made",
            body = crate::session_api::CreateSessionResponse,
            headers(("Location" = String, description = "`/v1/session`, the Session setup made"))
        ),
        crate::problem::openapi::StateConflict
    )
)]
pub async fn set_up_server(
    State(db): State<SqlitePool>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetUpServerRequest>,
) -> Result<Created<crate::session_api::CreateSessionResponse>, ApiError> {
    let password_hash = crate::credentials::hash_admin_password(&req.password).await?;

    let mut conn = db.acquire().await?;
    // The check and the write share a write transaction: two requests racing
    // to set up one Message Crate must not both believe they did, and the
    // second waits for the first and then finds the password set.
    let mut tx = crate::db::begin_write(&mut conn).await?;
    if account_profile::admin_has_password(&mut tx).await? {
        return Err(ApiError::StateConflict(
            "this Message Crate's administrator already has a password".into(),
        ));
    }
    let now = Utc::now();
    account_profile::update_password_hash(
        &mut tx,
        account_profile::ADMIN_ACCOUNT_ID,
        Some(&password_hash),
    )
    .await?;
    crate::db::audit_trail::record_about(
        &mut tx,
        crate::db::audit_trail::AuditAction::PasswordSet,
        crate::db::audit_trail::AuditActor::Admin,
        account_profile::ADMIN_ACCOUNT_ID,
        crate::db::audit_trail::Details::default(),
        now,
    )
    .await?;
    let token = crate::db::session_tokens::open_session(
        &mut tx,
        account_profile::ADMIN_ACCOUNT_ID,
        account_profile::ADMIN_USERNAME,
        crate::auth::connecting_app(&headers).as_ref(),
        now,
    )
    .await
    .map_err(ApiError::Internal)?;
    account_profile::record_login(&mut tx, account_profile::ADMIN_ACCOUNT_ID, now).await?;
    tx.commit().await?;

    // Setup makes the administrator's Session, and that is the resource it names.
    Ok(Created {
        location: "/v1/session".to_string(),
        body: crate::session_api::CreateSessionResponse {
            token,
            account_id: account_profile::ADMIN_ACCOUNT_ID,
            username: account_profile::ADMIN_USERNAME.to_string(),
        },
    })
}
```

Register it where `claim_server` was registered (grep `server/claim`), as `/v1/server/setup`. Add `set_up_server` to the utoipa paths list in `openapi.rs` where `claim_server` was.

- [ ] **Step 4: Drop registration from settings, audit, and problems**

- `db/server_settings.rs`: remove the `public_registration` field, its default, its column in `load`, and `set_public_registration`. Module doc: "(`asset_max_bytes` at 512 MiB)". "the owner" → "the administrator".
- `server_api.rs`: remove `public_registration` from `UpdateServerSettingsRequest` and the `if let Some(enabled) = req.public_registration` block. Module doc lines 1-10: reword to the two states.
- `db/audit_trail.rs`: remove `RegistrationOpened` and `RegistrationClosed` variants and their string arms; rename `AuditActor::Owner` to `Admin` with `"admin"` both ways; doc "The administrator, through Message Crate."; `CommandLine` doc names `set-admin-password`. Fix the `from_str`-style parser and any test listing actions (`db/audit_trail/tests.rs`).
- `api_error.rs`: rename `NotTheOwner(String)` → `NotTheAdmin(String)`; remove `RegistrationClosed(String)` and its match arms.
- `problem.rs`: `NotTheAdmin => ("not-the-admin", FORBIDDEN, "Not the administrator") from ApiError::NotTheAdmin(_)`; remove `RegistrationClosed`; text for `NotTheAdmin`: "This route belongs to the administrator: creating accounts, changing server settings, or anything else only the administrator may do. Ask the administrator to do it, or to give you what you need."; in `StateConflict`'s text replace "a Message Crate that already has an owner" with "a Message Crate whose administrator already has a password"; in the body-cap text replace `POST /v1/server/claim` with `POST /v1/server/setup` and "the owner" with "the administrator".
- `ApiError::RegistrationClosed` callers: `accounts_api.rs` `require_registration_open` is removed in Task 5; for now delete it and the two calls, and make `by_owner` a hard requirement (`let auth = auth.ok_or(ApiError::AuthenticationRequired(...))?` as the auth module spells the 401 variant). Task 5 finishes that handler.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p message-crate-server server_api audit_trail problem api_error`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/server/server/src
git commit -m "Report setup or ready, set the admin password with POST /v1/server/setup, and drop registration

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Auth, accounts API, test support and the credential matrix say admin

**Files:**
- Modify: `crates/server/server/src/auth.rs:28-120,345-360,605-620`
- Modify: `crates/server/server/src/accounts_api.rs:70-145,215-240,313-470`
- Modify: `crates/server/server/src/test_support/http.rs:195-220`, `crates/server/server/src/test_support.rs:42`
- Modify: `crates/server/server/src/openapi/credential_matrix.rs:13,31,55-125,217-260,335-345`
- Modify: every `security((), ("session" = ["owner"]))` and `("session" = ["owner"])` (19 sites: `grep -rn '"owner"\]' crates/server/server/src`), every `Owner(_auth): Owner` extractor (11 sites)
- Test: `crates/server/server/src/accounts_api/tests.rs`, `crates/server/server/src/auth/tests.rs`

**Interfaces:**
- Produces: `AuthCapability::Admin`, `AuthIdentity::is_admin()`, `require_admin`, extractor `Admin`; profile field `is_admin: bool`; `pub async fn log_in_as_admin(state: &AppState, password: &str) -> RegisteredAccount` (sets the password through `set_up_server`'s database path: `update_password_hash` on `ADMIN_ACCOUNT_ID`, then `log_in`); `Credential::Admin` with scope `admin`; schema names `AdminImportRun`, `AdminImportRunSummary`.

- [ ] **Step 1: Write the failing tests**

In `accounts_api/tests.rs`:

```rust
/// Creating an account is the administrator's alone: no credential is
/// `401`, an ordinary account's Session is `403`, and the administrator's
/// Session makes the account and opens no Session on it.
#[tokio::test]
async fn only_the_admin_creates_accounts() {
    let state = test_fixture().await.state;
    let admin = log_in_as_admin(&state, "Admin-Pw-7q2Lx9Vb").await;
    let body = serde_json::json!({ "username": "alice", "password": "Alice-Pw-3kN8wZr4" });
    let (status, _, _) = post_json_raw(&state, "/v1/accounts", None, &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, created) = post_json_raw(&state, "/v1/accounts", Some(&admin.token), &body).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(created.get("token").is_none(), "the administrator's creation opens no Session");
    let alice = log_in(&state, "alice", "Alice-Pw-3kN8wZr4").await;
    let bob = serde_json::json!({ "username": "bob", "password": "Bob-Pw-9qW2eRt6" });
    let (status, _, problem) =
        post_json_raw(&state, "/v1/accounts", Some(alice["token"].as_str().unwrap()), &bob).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    expect_problem(&problem, ProblemType::NotTheAdmin);
}

/// `admin` is the administrator's name and nobody else's, like `demo`.
#[tokio::test]
async fn a_new_account_cannot_be_named_admin() {
    let state = test_fixture().await.state;
    let admin = log_in_as_admin(&state, "Admin-Pw-7q2Lx9Vb").await;
    let body = serde_json::json!({ "username": "Admin", "password": "x" });
    let (status, _, problem) = post_json_raw(&state, "/v1/accounts", Some(&admin.token), &body).await;
    assert_eq!(status, StatusCode::CONFLICT);
    expect_problem(&problem, ProblemType::UsernameTaken);
}

/// The profile says who the administrator is, so the web app can route them.
#[tokio::test]
async fn the_profile_reports_is_admin() {
    let state = test_fixture().await.state;
    let admin = log_in_as_admin(&state, "Admin-Pw-7q2Lx9Vb").await;
    let profile = get_json(&state, "/v1/accounts/1", Some(&admin.token)).await;
    assert_eq!(profile["is_admin"], true);
    assert_eq!(profile["username"], "admin");
}
```

The `admin` username is taken because the row exists, so `require_username_free` already refuses it; the test proves it.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p message-crate-server only_the_admin_creates_accounts a_new_account_cannot_be_named_admin the_profile_reports_is_admin`
Expected: compile errors (`log_in_as_admin`, `ProblemType::NotTheAdmin`, `is_admin`).

- [ ] **Step 3: Implement**

`test_support/http.rs`: replace `claim_as_owner` with:

```rust
/// Give the administrator `password` and log in as `admin`, as setup does.
pub async fn log_in_as_admin(state: &AppState, password: &str) -> RegisteredAccount {
    let hash = crate::credentials::hash_password(password).expect("hash the admin password");
    let mut conn = state.db.acquire().await.expect("acquire for setup");
    crate::db::account_profile::update_password_hash(
        &mut conn,
        crate::db::account_profile::ADMIN_ACCOUNT_ID,
        Some(&hash),
    )
    .await
    .expect("set the admin password");
    drop(conn);

    let body = log_in(state, crate::db::account_profile::ADMIN_USERNAME, password).await;
    RegisteredAccount {
        account_id: crate::db::account_profile::ADMIN_ACCOUNT_ID,
        username: crate::db::account_profile::ADMIN_USERNAME.to_string(),
        token: body["token"].as_str().unwrap().to_string(),
    }
}
```

Export it from `test_support.rs` in place of `claim_as_owner`, then `sed -i 's/claim_as_owner(&state, "[^"]*", /log_in_as_admin(\&state, /g'` over `crates/server/server/src` and fix the stragglers by hand (the old helper took a username; the new one does not).

`auth.rs`: rename `AuthCapability::Owner` → `Admin` (doc: "Logged-in administrator. Carries no permissions at all…"), `is_owner` → `is_admin`, `require_owner` → `require_admin` with message "this endpoint requires the administrator's session" and `ApiError::NotTheAdmin`, the extractor `Owner` → `Admin`. Replace `Owner(_auth): Owner` with `Admin(_auth): Admin` everywhere, and `("session" = ["owner"])` with `("session" = ["admin"])`.

`accounts_api.rs`: field `is_owner` → `is_admin` (doc: "True for the administrator, who holds no messages"); `create_account` takes `Admin(auth): Admin` instead of `Option<AuthIdentity>`, drops `by_owner`, the registration checks and the `token` field from `CreateAccountResponse` (the struct becomes the `Account` alone; if utoipa needs the wrapper, keep `CreateAccountResponse { #[serde(flatten)] account }` without `token`); doc comment: "Create an account, as the administrator. The administrator picks the first password, and the account holder keeps it until they change it under Settings." `Admits::Owner` → `Admits::Admin`. Rename `OwnerImportRun`/`OwnerImportRunSummary` → `AdminImportRun`/`AdminImportRunSummary` (grep `OwnerImportRun` across the crate).

`credential_matrix.rs`: `Credential::Owner` → `Admin`, scope `"admin"`, display "admin session", `names_owner` → `names_admin`, `owner_session` → `admin_session`; comments say administrator.

- [ ] **Step 4: Run the whole server test suite**

Run: `cargo test -p message-crate-server`
Expected: PASS except the stale-document tests in `openapi.rs`, `cli_docs.rs` and `error_docs.rs`, which Task 7 regenerates. If anything else fails, fix it in this task.

- [ ] **Step 5: Commit**

```bash
git add crates/server/server/src
git commit -m "Name the administrator in auth, the accounts API, the credential matrix and the test support

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: `set-admin-password` replaces the two owner commands

**Files:**
- Rename: `crates/server/server/src/owner_cli.rs` → `crates/server/server/src/admin_cli.rs` (`git mv`), and the `mod owner_cli;` line in `lib.rs`
- Modify: `crates/server/server/src/cli.rs:73-110,334-335,355-390`
- Modify: `scripts/run-dev.sh:6,21,37-64,80,144-179,241-273`
- Modify: `crates/server/server/tests/common/claim.rs` → `tests/common/setup.rs` (`git mv`, and `mod claim;` → `mod setup;` in `tests/common/mod.rs`)
- Test: `crates/server/server/src/admin_cli.rs` inline tests, `crates/server/server/src/cli/tests.rs`

**Interfaces:**
- Produces: `pub async fn set_admin_password(opened: &OpenDb, password: &str) -> Result<SetAdminPasswordOutcome>` where `pub enum SetAdminPasswordOutcome { FirstPassword, Replaced }`; clap command `SetAdminPassword(SetAdminPasswordArgs)` with `--config` only; `tests/common/setup.rs` exports `ADMIN_PASSWORD`, `ALICE_PASSWORD`, `pub struct SetUp { admin_token, alice_id, alice_token }`, `pub async fn set_up_with_account(base: &str) -> SetUp` (setup, then create alice as admin, then log alice in).

- [ ] **Step 1: Write the failing tests**

In `admin_cli.rs`'s test module (keep `the_password_is_the_first_line_without_its_line_ending`), add:

```rust
/// In setup the command sets the first password and the Message Crate
/// becomes ready; when ready it replaces the password and ends the
/// administrator's Sessions, which is the way back in when it is lost.
#[tokio::test]
async fn set_admin_password_works_in_setup_and_when_ready() {
    let fixture = test_fixture().await;
    let state = fixture.state;
    let opened = fixture.opened_db(); // or however the fixture exposes an `OpenDb`
    assert!(matches!(
        set_admin_password(&opened, "first").await.unwrap(),
        SetAdminPasswordOutcome::FirstPassword
    ));
    assert_eq!(get_raw(&state, "/v1/server", None).await.1["state"], "ready");
    let admin = log_in_raw(&state, "admin", "first").await;
    let token = admin["token"].as_str().unwrap();
    assert!(matches!(
        set_admin_password(&opened, "second").await.unwrap(),
        SetAdminPasswordOutcome::Replaced
    ));
    assert_eq!(get_status(&state, "/v1/session", Some(token)).await, StatusCode::UNAUTHORIZED);
    assert_eq!(login_status(&state, "admin", "first").await, StatusCode::UNAUTHORIZED);
    assert_eq!(login_status(&state, "admin", "second").await, StatusCode::OK);
}

/// An empty password is refused in both states.
#[tokio::test]
async fn set_admin_password_refuses_an_empty_password() {
    let fixture = test_fixture().await;
    let opened = fixture.opened_db();
    let error = set_admin_password(&opened, "").await.unwrap_err().to_string();
    assert!(error.contains("must have a password"), "{error}");
}
```

Read the existing tests at the bottom of `owner_cli.rs` to see how they get an `OpenDb` from the fixture, and use that.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p message-crate-server admin_cli`
Expected: FAIL to compile (`set_admin_password` missing).

- [ ] **Step 3: Implement**

`git mv crates/server/server/src/owner_cli.rs crates/server/server/src/admin_cli.rs`; in `lib.rs` `mod owner_cli;` → `mod admin_cli;`. Replace the module doc and the two functions `create_owner` and `reset_owner_password` with:

```rust
//! The one shell command that reaches the administrator's credentials.
//!
//! `set-admin-password` gives the administrator a password. On a Message
//! Crate in setup that is the first password, and the Message Crate becomes
//! ready. On a ready one it replaces the password and ends the
//! administrator's Sessions, which is the way back in when the password is
//! lost: nothing inside the product can reset it, because no account stands
//! above the administrator. See
//! `docs/adr/0008-the-administrator-holds-no-messages.md`.
//!
//! The password is read from standard input ([`read_password`]) and never
//! from an argument, because a command's arguments are visible to every
//! local user through `ps` while it runs and stay in the shell's history
//! afterwards (#2094).

/// What `set_admin_password` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetAdminPasswordOutcome {
    /// The Message Crate was in setup; it is ready now.
    FirstPassword,
    /// The password was replaced and the administrator's Sessions ended.
    Replaced,
}

/// Set the administrator's password: the first one in setup, a replacement
/// when ready.
///
/// # Errors
///
/// Fails when the password is empty or over the cap, or when a statement
/// fails.
pub async fn set_admin_password(
    opened: &OpenDb,
    password: &str,
) -> Result<SetAdminPasswordOutcome> {
    // Hashed before a connection is taken, so none is held for the hash.
    let hash = match crate::credentials::hash_admin_password(password).await {
        Ok(hash) => hash,
        Err(e) => bail!("{e}"),
    };
    let mut conn = opened.conn().await?;
    // The password, its record and the end of the Sessions the old password
    // opened land together.
    let mut tx = crate::db::begin_write(&mut conn).await?;
    let had_password = account_profile::admin_has_password(&mut tx).await?;
    let now = Utc::now();
    account_profile::update_password_hash(&mut tx, account_profile::ADMIN_ACCOUNT_ID, Some(&hash))
        .await?;
    audit_trail::record_about(
        &mut tx,
        AuditAction::PasswordSet,
        AuditActor::CommandLine,
        account_profile::ADMIN_ACCOUNT_ID,
        audit_trail::Details::default(),
        now,
    )
    .await?;
    if had_password {
        // The old password is gone, so every Session it opened should be too.
        crate::db::session_tokens::revoke_account_sessions(
            &mut tx,
            account_profile::ADMIN_ACCOUNT_ID,
            AuditActor::CommandLine,
            now,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(if had_password {
        SetAdminPasswordOutcome::Replaced
    } else {
        SetAdminPasswordOutcome::FirstPassword
    })
}
```

`cli.rs`: replace the `CreateOwner` and `ResetOwnerPassword` variants with:

```rust
    /// Set the administrator's password. On a Message Crate in setup this is
    /// the first password; on a ready one it replaces the password and ends
    /// the administrator's sessions. The password is read from standard
    /// input: typed without echo at a terminal, or else the first line.
    SetAdminPassword(SetAdminPasswordArgs),
```

```rust
/// Options for `set-admin-password`. The password is read from standard
/// input (`admin_cli::read_password`), never taken as an argument.
#[derive(Debug, Args)]
pub struct SetAdminPasswordArgs {
    /// Path to config.toml
    #[arg(long, default_value = "config/config.toml")]
    pub config: PathBuf,
}
```

Dispatch `Commands::SetAdminPassword(args) => run_set_admin_password(args).await,` and:

```rust
/// Set the administrator's password from standard input.
async fn run_set_admin_password(args: SetAdminPasswordArgs) -> Result<()> {
    let password = crate::admin_cli::read_password("Administrator password: ")?;
    set_admin_password_with(args, &password).await
}

/// Set the administrator's password and report what happened.
async fn set_admin_password_with(args: SetAdminPasswordArgs, password: &str) -> Result<()> {
    let cfg = Config::load(&args.config)?;
    let opened = OpenDb::open(cfg).await?;
    let outcome = crate::admin_cli::set_admin_password(&opened, password).await?;
    opened.close().await;
    match outcome {
        crate::admin_cli::SetAdminPasswordOutcome::FirstPassword => {
            println!("Message Crate set up. Log in as admin.");
        }
        crate::admin_cli::SetAdminPasswordOutcome::Replaced => {
            println!("Administrator password set. Log in as admin.");
        }
    }
    Ok(())
}
```

Fix `cli/tests.rs` for the renamed commands.

`scripts/run-dev.sh`: `--owner` → `--admin`, `OWNER=` → `ADMIN=`, `refuse_owner` → `refuse_admin`, `require_loopback_bind_for_owner` → `require_loopback_bind_for_admin`; the call becomes `printf 'admin\n' | server_cli set-admin-password --config "${CONFIG}" || echo "set-admin-password did not set the password, so this Message Crate is left as it is"`; usage text: "Empty Message Crate in setup: the web UI opens on Set Up" and "Set the administrator's password to admin". Every "claim", "unclaimed", "Create Owner", "owner" in the script goes.

`tests/common/setup.rs`: rewrite `claimed_with_account` as `set_up_with_account`: `POST /v1/server/setup {password: ADMIN_PASSWORD}` → `201`, take `token` as `admin_token`; `POST /v1/accounts` with the admin token `{username: "alice", password: ALICE_PASSWORD}` → `201`, take `id`; `POST /v1/session {username: "alice", password}` → take `alice_token`. Update every integration test that calls it (`grep -rn 'claimed_with_account\|owner_token\|OWNER_PASSWORD' crates/server/server/tests`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p message-crate-server` and `shellcheck scripts/run-dev.sh` if installed.
Expected: PASS except the stale generated-document tests.

- [ ] **Step 5: Commit**

```bash
git add -A crates/server/server scripts/run-dev.sh
git commit -m "Replace create-owner and reset-owner-password with set-admin-password

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: The rest of the server crate says administrator

**Files:**
- Modify: every file under `crates/server/server/src` and `crates/server/server/tests` that `grep -rn -i 'owner' crates/server/server | grep -v -i 'owner_identity\|owner_handle\|owner_display\|owner_only\|owner_sender\|to_owner\|data_dir_owner\|the_owner_s_alone\|file owner'` lists: `reset_demo.rs`, `startup.rs`, `db/account_profile.rs` comments, `db/api_tokens.rs`, `db/erase.rs`, `db/trash.rs`, `audit_trail_api.rs`, `exports_api.rs`, `imports_api/*`, `server_api/log_*`, `middleware.rs`, `limits.rs`, `openapi/document_rules.rs`, `openapi/shared_parts.rs`, and their tests.

**Interfaces:** none new. Pure wording and test-name changes.

- [ ] **Step 1: List the remaining sites**

Run:

```bash
grep -rn -i 'owner\|claim\|registration' crates/server/server/src crates/server/server/tests \
  | grep -v -i 'owner_identity\|owner_handle\|owner_display\|owner_only\|owner_sender\|to_owner\|data_dir_owner\|owner.s alone\|owner.s bit\|claimed SHA\|claim_copy_name'
```

Expected: a list of comment lines, string literals and test names about the administrator, claiming, or registration.

- [ ] **Step 2: Reword every site**

For each: "the owner" → "the administrator", "owner's" → "administrator's", "Owner Home" → "Admin Home", "claimed"/"unclaimed" → "ready"/"in setup", "claiming" → "setup", "registration"/"registers" → gone or "the administrator creates the account". Test names `the_owner_…` → `the_admin_…`. The test `the_copy_a_rebuild_keeps_is_the_owner_s_alone` and the `open_db.rs` tests named `…_the_owner_s_alone` are about the file owner and stay. The `schema.rs` function `claim_copy_name` is about claiming a file name and stays.

- [ ] **Step 3: Verify nothing is left**

Run the grep from Step 1 again. Expected: no output.
Run: `cargo fmt -p message-crate-server && cargo clippy -p message-crate-server --all-targets -- -D warnings && cargo doc -p message-crate-server --no-deps --document-private-items` (rustdoc at `-D warnings`: `RUSTDOCFLAGS="-D warnings"`).
Expected: clean; a broken intra-doc link to `OWNER_ACCOUNT_ID` or `owner_cli` fails here.

- [ ] **Step 4: Commit**

```bash
git add -A crates/server/server
git commit -m "Say administrator throughout the server crate

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Regenerate the OpenAPI document, the error pages and the CLI reference

**Files:**
- Regenerate: `docs/src/assets/openapi.json`, `docs/src/content/docs/docs/developer/reference/errors/*.md`, `docs/src/content/docs/docs/developer/reference/server-cli.md`
- Delete: `docs/src/content/docs/docs/developer/reference/errors/registration-closed.md`, `.../errors/not-the-owner.md` (the dump writes `not-the-admin.md`; check whether the dump removes stale pages or whether you must `git rm` them, and whether `errors/index.md` is generated or hand-kept)
- Modify: `docs/architecture/rules/http-api.md` (see Task 12)

- [ ] **Step 1: Regenerate**

```bash
cargo run -p message-crate-server -- dump-openapi --output docs/src/assets/openapi.json
cargo run -p message-crate-server -- dump-error-docs --output docs/src/content/docs/docs/developer/reference/errors
cargo run -p message-crate-server -- dump-cli-docs --output docs/src/content/docs/docs/developer/reference/server-cli.md
git status --short docs
```

Expected: `openapi.json` changed; `not-the-admin.md` added; `server-cli.md` changed. Remove `registration-closed.md` and `not-the-owner.md` with `git rm` if they remain, and fix `errors/index.md` if it is hand-kept.

- [ ] **Step 2: Run the stale-document tests**

Run: `cargo test -p message-crate-server -- openapi cli_docs error_docs`
Expected: PASS.

- [ ] **Step 3: Check the document for leftovers**

Run: `grep -n -i 'owner\|claim\|registration' docs/src/assets/openapi.json | grep -v -i 'owner_identity\|owner_handle\|owner_display'`
Expected: no output. If a line remains, it is a doc comment in the server; fix it there and regenerate.

- [ ] **Step 4: Commit**

```bash
git add -A docs/src/assets/openapi.json docs/src/content/docs/docs/developer/reference
git commit -m "Regenerate the OpenAPI document, the error pages and the CLI reference for the administrator

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Web types, setup form, two-state login card

**Files:**
- Regenerate: `web/src/lib/serverApi.types.ts` (`cd web && npm run gen:api`)
- Modify: `web/src/lib/serverApi.ts:136-175`
- Rename: `web/src/screens/auth/ClaimForm.tsx` → `SetupForm.tsx`
- Delete: `web/src/screens/auth/CreateAccountForm.tsx`
- Modify: `web/src/screens/auth/LocalAuthTabs.tsx`, `web/src/screens/auth/useServerState.ts`, `web/src/screens/auth/LoginForm.tsx` (registration comment)
- Test: `web/src/screens/auth/LocalAuthTabs.test.tsx`

**Interfaces:**
- Produces: `export type ServerState = "setup" | "ready";`, `export function setUpServer(body: Schema["SetUpServerRequest"]): Promise<Schema["CreateSessionResponse"]>` posting to `/v1/server/setup`; `SetupForm({ serverUrl, disabled })`.
- Removes: `claimServer`, `ClaimForm`, `CreateAccountForm`.

- [ ] **Step 1: Regenerate the types**

```bash
cd web && npm run gen:api && npm run typecheck
```

Expected: type errors in `serverApi.ts` (`ClaimServerRequest`), `ClaimForm.tsx`, `useServerState.ts`, `ServerSettingsPanel.tsx` (`public_registration`), `useIsOwner.ts` (`is_owner`), `apiShapes.ts`. These are the sites the next steps and Task 9 fix.

- [ ] **Step 2: Rewrite the LocalAuthTabs tests**

Replace the five tests in `LocalAuthTabs.test.tsx` with:

```tsx
describe("LocalAuthTabs", () => {
  it("offers only Set Up on a Message Crate in setup", () => {
    renderTabs("setup");
    expect(screen.getByRole("heading", { name: "Set Up" })).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Username")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Password", { exact: true })).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeInTheDocument();
  });

  it("offers only Login on a ready Message Crate", () => {
    renderTabs("ready");
    expect(screen.getByRole("heading", { name: "Login" })).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByText("Create Account")).not.toBeInTheDocument();
  });

  it("explains that the password is the administrator's", () => {
    renderTabs("setup");
    expect(screen.getByText(/password for admin/i)).toBeInTheDocument();
  });
});
```

Keep the file's existing `renderTabs` helper, changing its state type. Run `npx vitest run src/screens/auth/LocalAuthTabs.test.tsx`; expected: FAIL.

- [ ] **Step 3: Implement**

`serverApi.ts`:

```ts
/**
 * What a logged-out visitor is offered: `setup` while the administrator has
 * no password, `ready` after. One value from the server, so the rule is
 * stated once. See `docs/adr/0008-the-administrator-holds-no-messages.md`.
 */
export function getServerState(opts?: RequestOptions): Promise<Server> { /* unchanged */ }

/** Set the administrator's password on a Message Crate in setup. Returns the administrator's session. */
export function setUpServer(
  body: Schema["SetUpServerRequest"],
): Promise<Schema["CreateSessionResponse"]> {
  return apiClient.post<Schema["CreateSessionResponse"]>("/v1/server/setup", body);
}
```

Remove `claimServer`. Reword the account-section comments ("the administrator").

`useServerState.ts`: `export type ServerState = "setup" | "ready";` and the doc comment: "The server answers with one value, `setup` or `ready`, so the rule 'does the administrator have a password' is stated once…".

`git mv ClaimForm.tsx SetupForm.tsx` and rewrite:

```tsx
import { type FormEvent, useState } from "react";
import AuthErrorFooter from "../../components/AuthErrorFooter";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { useAuth } from "../../hooks/useAuth";
import { setBaseUrl } from "../../lib/api";
import { setUpServer } from "../../lib/serverApi";
import CredentialFields from "./CredentialFields";
import KeepLoggedInSubmit from "./KeepLoggedInSubmit";
import { useKeepLoggedIn } from "./useKeepLoggedIn";

/**
 * Set the administrator's password, which is the only thing a Message Crate
 * in setup offers.
 *
 * The administrator is `admin`, made with the database, so there is no
 * username to choose: this form asks for the password twice and nothing
 * else. Submitting sets it and logs the person in as `admin`, who lands on
 * Admin Home.
 */
export default function SetupForm({
  serverUrl,
  disabled = false,
}: {
  serverUrl: string;
  disabled?: boolean;
}) {
  const { login } = useAuth();
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const keep = useKeepLoggedIn();
  const { busy, error, run } = useAsyncAction();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy || disabled) return;
    void run(async () => {
      // Only the mismatch is checked here. Length is the server's rule, so it
      // stays there rather than being restated and left to drift.
      if (password !== confirmPassword) {
        throw new Error("Passwords do not match.");
      }
      const url = serverUrl.trim();
      setBaseUrl(url);
      const res = await setUpServer({ password });
      await login(url, res.token, res.account_id, keep.lifetime);
    });
  };

  return (
    <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
      <p className="mb-5 text-[0.875rem] leading-relaxed text-muted">
        Choose the password for admin, the administrator who creates and manages accounts.
      </p>
      <CredentialFields
        password={{ value: password, onChange: setPassword }}
        autoComplete="new-password"
        confirmPassword={{ value: confirmPassword, onChange: setConfirmPassword }}
        disabled={disabled}
      />
      <KeepLoggedInSubmit choice={keep} disabled={busy || disabled}>
        {busy ? "Saving…" : "Continue"}
      </KeepLoggedInSubmit>
      <div className="mt-auto">
        <AuthErrorFooter error={error} className="h-16" />
      </div>
    </form>
  );
}
```

Check `CredentialFields.tsx`: if `username` is a required prop, make it optional and render the username field only when given.

`LocalAuthTabs.tsx`: remove the `Tabs` import, `CreateAccountForm`, and `authTabClass`; the component returns the "Set Up" heading with `<SetupForm>` for `"setup"` and the "Login" heading with `<LoginForm>` otherwise. Doc comment: the two states, and `docs/adr/0008-the-administrator-holds-no-messages.md`.

`git rm web/src/screens/auth/CreateAccountForm.tsx` (and its test if one exists). Remove the `/register` route comment "Registration is now the second tab…" in `App.tsx` and the route itself (no backwards compatibility).

- [ ] **Step 4: Run the tests**

Run: `cd web && npx vitest run src/screens/auth && npm run typecheck`
Expected: the auth tests pass; typecheck still fails in Task 9's files.

- [ ] **Step 5: Commit**

```bash
git add -A web/src/lib/serverApi.types.ts web/src/lib/serverApi.ts web/src/screens/auth web/src/App.tsx
git commit -m "Set up the administrator's password from the login card, with no username and no Create Account

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Admin Home, hooks, query keys, server settings, and the rest of `web/`

**Files:**
- Rename: `web/src/screens/OwnerHome.tsx` → `AdminHome.tsx` (+ test), `web/src/screens/owner/` → `web/src/screens/admin/`, inside it `OwnerAccountsPanel` → `AdminAccountsPanel`, `OwnerAuditTrailPanel` → `AdminAuditTrailPanel`, `OwnerDashboardPanel` → `AdminDashboardPanel`, `OwnerLogsPanel` → `AdminLogsPanel`, `ownerTableStyles.ts` → `adminTableStyles.ts` (+ tests)
- Rename: `web/src/hooks/useIsOwner.ts` → `useIsAdmin.ts`, `web/src/hooks/useOwnerAccounts.ts` → `useAdminAccounts.ts` (+ test)
- Modify: `web/src/App.tsx:16,24,37,44,51,60-65`, `web/src/lib/queryKeys.ts:20-22,137-171`, `web/src/lib/auditTrail.ts:160-166`, `web/src/lib/account.ts`, `web/src/lib/desktop/runLog.ts:20-66`, `web/src/screens/admin/ServerSettingsPanel.tsx:22-40,83-95`, `web/src/test/apiShapes.ts`, `web/src/screens/SettingsScreen.tsx`, `web/src/screens/settings/ProfileSettingsPanel.tsx`, `web/src/components/*` and every other file `grep -rn -i 'owner' web/src` lists outside `lib/importSources/`, `screens/import/`, `lib/backupIdentity*`, `lib/whatsapp*`, `lib/sbr*`, `lib/phone*`, `lib/chatBubbleShared*`, `components/messages/*`, `lib/desktop/extract.ts`, `lib/desktop/staging.ts` (those are the backup's owner)

**Interfaces:**
- Produces: `useIsAdmin(): { isAdmin: boolean; loading: boolean; error: string }` reading `profile?.is_admin`; `useAdminAccounts` (same shape as before); `keys.adminAccounts` with `["admin-accounts", …]`; route `/admin/:section?/:accountId?`; `runLog.ts` field `admin: boolean`.

- [ ] **Step 1: Rename with git so history follows**

```bash
cd web/src
git mv screens/OwnerHome.tsx screens/AdminHome.tsx
git mv screens/OwnerHome.test.tsx screens/AdminHome.test.tsx
git mv screens/owner screens/admin
for f in Accounts AuditTrail Dashboard Logs; do
  git mv screens/admin/Owner${f}Panel.tsx screens/admin/Admin${f}Panel.tsx
  [ -f screens/admin/Owner${f}Panel.test.tsx ] && git mv screens/admin/Owner${f}Panel.test.tsx screens/admin/Admin${f}Panel.test.tsx
done
git mv screens/admin/ownerTableStyles.ts screens/admin/adminTableStyles.ts
git mv hooks/useIsOwner.ts hooks/useIsAdmin.ts
git mv hooks/useOwnerAccounts.ts hooks/useAdminAccounts.ts
git mv hooks/useOwnerAccounts.test.tsx hooks/useAdminAccounts.test.tsx
```

- [ ] **Step 2: Rename identifiers**

```bash
cd web/src
grep -rl 'OwnerHome\|useIsOwner\|isOwner\|useOwnerAccounts\|ownerAccounts\|owner-accounts\|OwnerAccountsPanel\|OwnerAuditTrailPanel\|OwnerDashboardPanel\|OwnerLogsPanel\|ownerTableStyles\|OwnerAuditTrailParams\|OwnerImportRun\|is_owner\|"/owner' . \
  | xargs sed -i 's/OwnerHome/AdminHome/g; s/useIsOwner/useIsAdmin/g; s/isOwner/isAdmin/g; s/useOwnerAccounts/useAdminAccounts/g; s/ownerAccounts/adminAccounts/g; s/owner-accounts/admin-accounts/g; s/OwnerAccountsPanel/AdminAccountsPanel/g; s/OwnerAuditTrailPanel/AdminAuditTrailPanel/g; s/OwnerDashboardPanel/AdminDashboardPanel/g; s/OwnerLogsPanel/AdminLogsPanel/g; s/ownerTableStyles/adminTableStyles/g; s/OwnerAuditTrailParams/AdminAuditTrailParams/g; s/OwnerImportRun/AdminImportRun/g; s/is_owner/is_admin/g; s#"/owner#"/admin#g; s#screens/owner/#screens/admin/#g; s#\./owner/#./admin/#g'
```

Then `npm run typecheck` and fix what it names.

- [ ] **Step 3: Server settings and audit trail**

`screens/admin/ServerSettingsPanel.tsx`: delete the `save` mutation for `public_registration` and the `<Checkbox>` block with its caption; reword the doc comment (lines 22-32) to drop the registration sentence and say "the administrator". Update `ServerSettingsPanel.test.tsx` to drop the registration test(s).

`lib/auditTrail.ts`: remove the `registration_opened`/`registration_closed` cases; where the actor `"owner"` is rendered, render `"admin"` as "Administrator". Update `auditTrail.test.ts`.

`lib/desktop/runLog.ts`: field `owner: boolean` → `admin: boolean` with comment "The administrator reads every log…"; update the Tauri command's struct in Task 10 to match the serialized name (`admin`).

- [ ] **Step 4: Reword comments and copy**

Run `grep -rn -i 'owner' web/src | grep -v -i 'owner_identity\|ownerIdentity\|owner_display\|ownerDisplay\|owner_handle\|backup.s owner\|importSources\|backupIdentity\|whatsapp\|sbrExtract\|phoneTokens\|chatBubble\|screens/import\|desktop/extract\|desktop/staging\|phoneCountries\|nameRefusals\|toolStatusCopy\|uiStyles\|build.ts'` and reword each remaining line: "the owner" → "the administrator", "Owner Home" → "Admin Home". Check the excluded files too: any of them that says "the owner" meaning the administrator (for example `lib/account.ts:30,50,52`) is reworded; "the backup's owner" stays.

- [ ] **Step 5: Run the checks**

```bash
cd web && npm run typecheck && npx biome ci . && npx vitest run
```

Expected: all pass. Fix any test that asserted old copy ("Create Owner", "owner").

- [ ] **Step 6: Commit**

```bash
git add -A web/src
git commit -m "Rename Owner Home and its hooks to Admin Home, and drop the registration setting from the web app

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Desktop app wording

**Files:**
- Modify: `src-tauri/src/run_logs.rs:1-12,47-63,162,294`, `src-tauri/src/commands/run_logs.rs:33`, `src-tauri/src/tool_downloads/pins.rs:4` (docs path)

**Interfaces:**
- Produces: the run-log reader struct's field `admin: bool` (serialized `admin`), matching `web/src/lib/desktop/runLog.ts` from Task 9.

- [ ] **Step 1: Find the sites**

Run: `grep -rn -i 'owner' src-tauri/src | grep -v -i 'restrict_to_owner\|owner.s bit\|owner/name\|ownership\|owner_identity\|owner_display\|owner_handle\|the directories only the owner'`
Expected: `run_logs.rs` and `commands/run_logs.rs` lines about who reads logs, and the `pins.rs` docs path.

- [ ] **Step 2: Reword and rename**

In `run_logs.rs`: "Owner Home" → "Admin Home"; "the owner" → "the administrator"; field `pub owner: bool` → `pub admin: bool` with its accessor; the message "is the administrator's alone". In `commands/run_logs.rs:33`: "for the administrator". In `pins.rs:4`: `features/admin/troubleshooting.md`. Update the tests in `src-tauri/src/run_logs.rs` (or its `tests.rs`) that construct the struct.

- [ ] **Step 3: Check**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings && cargo test --manifest-path src-tauri/Cargo.toml run_logs
```

Expected: clean and passing.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src
git commit -m "Say administrator in the desktop app's run log reader

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: Glossary, ADRs, CLAUDE.md, AGENTS.md, CHANGELOG

**Files:**
- Modify: `CONTEXT.md:170-200,230-240,280,320-410,570-650`
- Rename: `docs/adr/0008-the-owner-holds-no-messages.md` → `docs/adr/0008-the-administrator-holds-no-messages.md`; update every link to it (`git grep -l 'adr/0008'` lists 29 files outside `crates/` and `web/`, plus the code comments already done)
- Modify: `docs/adr/0002-*.md`, `docs/adr/0016-*.md`, `docs/adr/0018-*.md`, `docs/adr/0020-*.md`
- Modify: `CLAUDE.md`, `AGENTS.md:459-467,622`, `CHANGELOG.md`, `README.md:94`, `docker/compose.yml:2,31`, `docker/compose.release.yml`, `config/config.toml.example:35`, `.cursor/rules` (check its owner line)

- [ ] **Step 1: CONTEXT.md**

Replace the **Owner** entry with:

```markdown
**Administrator**:
The one account that manages a Message Crate: its other accounts and its
server settings. Its username is `admin`, fixed, and it exists from the moment
the database is made; it cannot be deleted, and no other account can be given
its powers. It holds no messages of its own. The administrator creates,
disables and deletes accounts, resets their passwords, and deletes their
message data. It monitors the Message Crate through metadata: counts and
totals of messages, contacts and attachments, each account's imports and
exports, when it last logged in, and attachment file names and sizes. The
administrator never reads content: a message's text, an attachment's bytes,
or a contact's name and identities. The full line is in `docs/adr/0008`. In
prose the role is "the administrator"; the account and the login are `admin`.
_Avoid_: Owner, Vault Owner, Superuser, Root
```

Replace **Owner Home** with **Admin Home** (same body, "administrator" for "owner", `/admin` is not named in the glossary). Replace **Claiming** with:

```markdown
**Setup**:
The state of a Message Crate whose administrator has no password yet, and
the one act it offers: setting that password, from the login card or with
`set-admin-password`. A Message Crate is in setup or ready; nothing else.
_Avoid_: Claiming, Unclaimed, First run, Provisioning, Open, Closed
```

In **Audit Trail** and **Session**, "the owner" → "the administrator"; in Audit Trail drop "opening and closing registration" if present and name "the administrator's changes to accounts". Lines 177, 190, 574, 598, 601, 648: "the administrator". Lines 234-237 and 281 are the backup's owner and stay.

- [ ] **Step 2: ADR 0008**

`git mv docs/adr/0008-the-owner-holds-no-messages.md docs/adr/0008-the-administrator-holds-no-messages.md`. Retitle "# The administrator holds no messages". Rewrite the opening paragraph:

> A Message Crate has exactly one administrator, `admin`, at a fixed well-known account id, made when the database is made, and it has no store of its own: no conversations, no contacts, no import, no export, no trash, and no API tokens. The administrator sets the server settings, creates, disables and deletes accounts, resets their passwords, and deletes their message data. It monitors the installation: how much each account holds and what each account has been doing. It never reads a message. A Message Crate whose administrator has no password is in setup and offers only setting it; after that it is ready.

Throughout: "owner" → "administrator", "claim"/"unclaimed" → "setup", and remove every sentence about public registration (lines 12, 171, 235-237 and their neighbors), replacing the registration-default reasoning with one sentence: "Only the administrator creates accounts; there is no self-service registration (its former design is recorded in #3248)." Append:

```markdown
## Amended (2026-10-24)

The owner became the administrator, `admin`, a fixed username on the fixed id,
made with the database instead of by a claim; setup replaced claiming; and
public registration was removed. The design of registration as it was is
recorded in #3248.
```

Then `git grep -l 'adr/0008-the-owner' | xargs sed -i 's/0008-the-owner-holds-no-messages/0008-the-administrator-holds-no-messages/g'`.

- [ ] **Step 3: Other ADRs, CLAUDE.md, AGENTS.md, README, compose, config**

ADRs 0002, 0016, 0018, 0020: `grep -n -i owner` each and reword to administrator (ADR 0016 says "no owner" of the demo: "no administrator's powers" or as fits). `CLAUDE.md`: in the server bullet, "from the account and from the owner alike" → "from the account and from the administrator alike"; `AGENTS.md:459-467`: the dev-run block becomes

```
./scripts/run-dev.sh --reset         # wipe data/, start empty and in setup (UI opens on Set Up)
./scripts/run-dev.sh --reset --admin # wipe data/, set the administrator's password to admin
```

and the paragraph: "Neither sets the administrator's password; add `--admin` to either for that. `--admin` refuses to run unless…", "After `--admin`, log in as `admin` / `admin`. Otherwise set the password in the UI." Line 622: "Shown in Admin Home → Server Settings." `README.md:94`, `docker/compose.yml:2,31`, `docker/compose.release.yml`: `features/owner/` → `features/admin/`. `config/config.toml.example:35`: "the administrator sets it". `.cursor/rules`: reword its owner line if it means the administrator.

- [ ] **Step 4: CHANGELOG**

Under the in-development heading, in **Design**:

```markdown
- 2026-10-24 The owner is now the administrator, `admin`. Every Message Crate
  has it from the start; the first visit sets its password instead of
  creating an owner, and `set-admin-password` sets or resets it from the
  server. Only the administrator creates accounts: the setting that let
  anyone create their own is gone, with its Create Account tab. Admin Home is
  at `/admin`.
```

Add under **Upgrading** for this version: "The database is rebuilt empty at this version (the schema changed), and the server has no `create-owner` or `reset-owner-password` command; run `set-admin-password`." Run `./scripts/check-changelog.sh`.

- [ ] **Step 5: Verify and commit**

Run: `grep -rn -i 'owner' CONTEXT.md CLAUDE.md AGENTS.md README.md CHANGELOG.md docker config docs/adr | grep -v -i 'owner_identity\|owner_handle\|owner_display\|backup.s owner\|owner address\|repos/{owner}\|0015-the-owner\|owner is recorded'`
Expected: only lines in released CHANGELOG sections (history, left alone) and ADR 0015.

```bash
git add -A CONTEXT.md CLAUDE.md AGENTS.md README.md CHANGELOG.md docker config docs/adr .cursor
git commit -m "Say administrator in the glossary, the ADRs, the agent guides and the changelog

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: Architecture pages and rules

**Files:**
- Modify: `docs/architecture/README.md`, `docs/architecture/http-api.md`, `docs/architecture/rules/http-api.md:141-144,236-247,312-322,515-535,610,657-658,968-1130,1182,1341-1364`, `docs/architecture/server-log.md`, `docs/architecture/rules/server-log.md`, `docs/architecture/import-run-logs.md`, `docs/architecture/rules/import-run-logs.md`, `docs/architecture/media.md`, `docs/architecture/rules/media.md`, `docs/architecture/contacts-identities-and-messages.md`, `docs/architecture/rules/contacts-identities-and-messages.md`

- [ ] **Step 1: Rules for the HTTP API**

In `rules/http-api.md`: keep every rule id. Line 141-144: "`/v1/accounts` is one collection for the administrator and for the account itself; there is no `/v1/admin/` and no `/v1/account`. Rejected: a role prefix such as `/v1/admin/`…". Line 247: "a setup that makes the administrator's Session". Line 1182: "`GET /v1/server` and `POST /v1/server/setup` take no credential." Credentials section (968-1130): "A **Session** is one per logged-in account or administrator…", the `admin` scope, "no token acts as the administrator". Lines 657-658: the remedy sentence. Where a rule describes `POST /v1/accounts` taking no credential while registration is open, rewrite to "takes the administrator's Session only". Where the two problem types are listed, `not-the-admin` replaces `not-the-owner` and `registration-closed` is removed.

- [ ] **Step 2: The other pages**

`grep -rn -i 'owner\|claim\|registration' docs/architecture | grep -v -i 'owner_identity\|owner_handle\|owner_display\|backup.s owner'` and reword each line. In `contacts-identities-and-messages*.md` most matches are the backup's owner and stay; read each.

- [ ] **Step 3: Check the docs build**

```bash
cd docs && npm run check 2>/dev/null || npx astro check; npx astro build
```

(Use the commands AGENTS.md "Build, format, and test" names for `docs/`.) Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add docs/architecture
git commit -m "Say administrator in the architecture pages and rules

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 13: User docs and the sidebar

**Files:**
- Rename: `docs/src/content/docs/docs/user/features/owner/` → `features/admin/`; `owner-home.md` → `admin-home.md`
- Rename: `docs/src/content/docs/docs/user/your-messages/create-the-owner-and-an-account.md` → `set-up-the-administrator-and-create-an-account.md`
- Modify: `docs/astro.config.mjs:41,92-98`, `docs/scripts/user-guide-screenshots.cjs:60-66`, every page under `docs/src/content/docs` that `grep -rln -i 'owner\|claim\|registration\|Create Account' docs/src/content/docs` lists

- [ ] **Step 1: Move and relink**

```bash
cd docs/src/content/docs/docs/user
git mv features/owner features/admin
git mv features/admin/owner-home.md features/admin/admin-home.md
git mv your-messages/create-the-owner-and-an-account.md your-messages/set-up-the-administrator-and-create-an-account.md
cd ../../../../..
grep -rl 'features/owner\|owner-home\|create-the-owner-and-an-account' docs src web README.md docker crates src-tauri | xargs sed -i 's#features/owner/owner-home#features/admin/admin-home#g; s#features/owner#features/admin#g; s#create-the-owner-and-an-account#set-up-the-administrator-and-create-an-account#g'
```

`astro.config.mjs`: label `'1. Set up the administrator and create an account'`, sidebar group label `'Administrator'`.

- [ ] **Step 2: Rewrite the pages**

`set-up-the-administrator-and-create-an-account.md`: the first visit shows Set Up; choose the password for `admin` twice and Continue; you land on Admin Home; open User Accounts and add an account with a username and password; log out and log in as that account. `admin-home.md`: "Admin Home", "the administrator", no registration checkbox under Server Settings. `troubleshooting.md`: "lost the administrator's password" → `set-admin-password`. `run-with-docker.md`, `run-on-another-machine.md`, `update.md`: wording. Every other page the grep lists: "the owner" → "the administrator", remove any mention of Create Account on the login card or of letting anyone create an account.

`docs/scripts/user-guide-screenshots.cjs:60-66`: the setup flow fills Password and Confirm Password only, no Username.

- [ ] **Step 3: Build the docs and check links**

```bash
cd docs && npx astro check && npx astro build
./scripts/audit-docs.sh
```

Expected: clean; a broken link to a moved page fails here.

- [ ] **Step 4: Commit**

```bash
git add -A docs README.md docker
git commit -m "Move the owner's user docs to the administrator's, and describe setup in place of claiming

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 14: Whole-repo sweep and the full check

**Files:** whatever the sweep finds.

- [ ] **Step 1: Sweep**

```bash
grep -rIn -i 'owner\|unclaimed\|claimed\|claiming\|public_registration\|registration' . \
  --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git --exclude-dir=dist --exclude-dir=web-next --exclude-dir=vendor \
  | grep -v -i 'owner_identity\|owner_handle\|owner_display\|owner_sender\|owner_only\|to_owner\|restrict_to_owner\|owner.s alone\|owner.s bit\|backup.s owner\|repos/{owner}\|repos/<owner>\|owner/name\|ownership\|LICENSE\|0015-the-owner\|claimed SHA\|claim_copy\|package-lock\|Cargo.lock\|CHANGELOG.md:\|#3248\|whatsapp-exporter/src/owner'
```

Read every line. Each is one of: the backup's owner (stays), a file owner (stays), third-party text (stays), released changelog history (stays), or the administrator (fix). `docs/gui/web-next.md` and `docs/gui/web-vs-web-next.md` describe web-next and stay. `docs/superpowers/specs/2026-09-30-rename-to-message-crate-design.md` is a dated spec and stays.

- [ ] **Step 2: Run the full local CI**

```bash
./scripts/check-all.sh
```

Expected: every check passes. Fix and re-run until it does.

- [ ] **Step 3: Try it in the browser**

```bash
./scripts/run-dev.sh --reset
```

With the Playwright MCP: open `http://localhost:5173`, see "Set Up" with two password fields and Explore Demo Account absent (`--reset` seeds no demo), set `admin`, land on `/admin`, open Server Settings and confirm no registration checkbox, open User Accounts and create `alice`, log out, see Login alone, log in as `alice`. Stop the server. Then `./scripts/run-dev.sh --reset --admin` and confirm `admin` / `admin` logs in.

- [ ] **Step 4: Commit the sweep**

```bash
git add -A
git commit -m "Finish the sweep: no owner left where it meant the administrator

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

- [ ] **Step 5: Open the pull request**

Push the branch and open one PR with the feature template AGENTS.md "Submitting Work" names, titled "The administrator replaces the owner". The body links the spec, #3248, and lists: fixed `admin` made with the database; setup replaces claiming (`POST /v1/server/setup`, `set-admin-password`); public registration removed; Admin Home at `/admin`; ADR 0008 rewritten; schema fingerprint changed. End with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
