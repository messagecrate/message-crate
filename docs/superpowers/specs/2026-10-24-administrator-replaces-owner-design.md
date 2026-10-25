# The administrator replaces the owner

Date: 2026-10-24

## Decision

A Message Crate no longer has an owner and is never claimed. It has one
administrator account, `admin`, at the fixed account id 1, made when the
database is made. A new Message Crate is in **setup** until `admin` has a
password, and **ready** after. Only the administrator creates accounts:
public registration is removed, and its design is recorded in #3248 for the
day anyone wants it back.

The word "owner" leaves every place where it meant the administrator: code,
comments, tests, the HTTP interface, the web app, the desktop app, the
glossary, the ADRs, the architecture pages and rules, the user docs, the
scripts, and AGENTS.md. It stays where it means something else:

- the backup's owner, the phone's own user in an export, recorded on each
  message as `owner_identity` and `owner_handle_id` (ADR 0015), and the
  WhatsApp exporter's `owner` module;
- the operating-system user the server runs as (`owner_only.rs`,
  `make_data_dir_owner_only`, `file_io::owner_only`).

## Vocabulary (CONTEXT.md)

- **Administrator** replaces **Owner**. The entry keeps the Owner's line:
  one account, holds no messages, creates, disables and deletes accounts,
  resets their passwords, deletes their message data, monitors through
  metadata, never reads content. It adds: its username is `admin` and is
  fixed, it exists from the moment the database is made, and it cannot be
  deleted. In prose the role is "the administrator"; the account and the
  login name are `admin`. _Avoid_: Owner, Vault Owner, Superuser, Root.
- **Admin Home** replaces **Owner Home**, same screen, same contents, at
  `/admin`. _Avoid_: Owner Home, Console, Dashboard, Admin panel.
- **Setup** replaces **Claiming**: the state of a Message Crate whose
  administrator has no password yet, and the one act it offers, setting
  that password. A Message Crate is in setup or ready; nothing else.
  _Avoid_: Claiming, Unclaimed, First run, Provisioning, Open, Closed.
- The **Audit Trail** and **Session** entries say administrator where they
  said owner. The Audit Trail entry drops opening and closing registration
  from what it records.

## Server

### Database and seeding

- `ADMIN_ACCOUNT_ID: i64 = 1` replaces `OWNER_ACCOUNT_ID`; `is_admin`
  replaces `is_server_owner`; `is_claimed` becomes `admin_has_password`.
- The `admin` row (id 1, username `admin`, NULL password hash, no
  permissions) is inserted when the database is created, so
  `create-database` and `serve` on a new database both make it. The Demo
  Account is still added only by `serve`'s first start and by `reset-demo`.
  The `accounts.sql` comments that explain ids 1 and 2 say so.
- `server_settings.public_registration` is dropped. The Schema Fingerprint
  changes; that is expected and is not avoided.
- `AuditAction::RegistrationOpened` and `RegistrationClosed` go, in the enum,
  the `accounts.sql` comment that lists actions, and `web/src/lib/auditTrail.ts`.
  The actor value `owner` becomes `admin` in the `actor` column's comment and
  in the code that writes and reads it.
- A username check refuses `admin` for a new account the way it refuses
  `demo` today, if it does not already.

### Login

- The administrator is the one account that must have a password. Every
  other account with a NULL hash, the Demo Account among them, logs in with
  an empty password. `admin` in setup, with its NULL hash, is refused with
  `invalid-credentials` after the usual wait, and the refusal is recorded in
  the Audit Trail as a wrong password. (Amended 2026-10-25: this said the
  no-password login was the Demo Account's alone, which locked out an
  account whose holder pressed Reset password.)

### Routes

- `GET /v1/server`: `state` is `setup` or `ready`. The `demo_account` flag
  and the other fields are unchanged. The doc comment says what each state
  shows on the login screen.
- `POST /v1/server/setup` replaces `POST /v1/server/claim`. Body
  `{ "password": "…" }`, one to 128 characters, no username, no credential.
  Answers `201 Created` with the administrator's Session and `Location:
  /v1/session`, as the claim does today. Answers `409 Conflict`
  (`state-conflict`, "this Message Crate's administrator already has a
  password") when `admin` has one. The check and the write share the write
  transaction, as today.
- `PUT /v1/accounts/1/password` keeps working for the administrator
  changing its own password with the current one.
- `PATCH /v1/server/settings` loses `public_registration`; the response
  `ServerSettings` loses it too.
- `POST /v1/accounts` takes the administrator's Session only. The no-credential
  path, `require_registration_open`, `ApiError::RegistrationClosed`, the
  `registration-closed` problem type and its docs page go. The response loses
  `token`, which only a stranger's registration filled; the administrator's
  creation opens no Session.
- The OpenAPI scope `owner` becomes `admin`. `Credential::Owner` becomes
  `Credential::Admin` in the credential matrix. `ApiError::NotTheOwner` and
  the problem type `not-the-owner` become `NotTheAdmin` and `not-the-admin`.
- `OwnerImportRun`, `OwnerImportRunSummary` and any other `Owner…` schema
  names become `Admin…`. `docs/src/assets/openapi.json` and
  `web/src/lib/serverApi.types.ts` are regenerated, never edited by hand.

### Command line

- `set-admin-password` replaces `create-owner` and `reset-owner-password`.
  It takes `--config`/`--db` as they do, reads the password from standard
  input (`admin_cli::read_password`, renamed from `owner_cli`), and works in
  both states: in setup it sets the first password; when ready it replaces
  the password and ends the administrator's Sessions. It has no `--username`.
  It reports the username to log in with, `admin`.
- `scripts/run-dev.sh`: `--owner` becomes `--admin`, calling
  `set-admin-password` with `admin`; the loopback-bind refusal and its
  messages are kept and reworded. The usage text says "setup" where it said
  "unclaimed" and "Create Owner".

### Renames in the server crate

`owner_cli.rs` → `admin_cli.rs`; `auth.is_owner()` → `is_admin()`;
`AuthIdentity` and the guards' doc comments; the test support fixture's
`owner_session` → `admin_session`; `tests/common/claim.rs` → `setup.rs`.
Every comment that says owner for the administrator is reworded. The grep
`owner` over `crates/server/server/src` afterwards matches only the backup
owner (`owner_handle_id`, `owner_identity`) and the file owner.

## Web app

- `screens/OwnerHome.tsx` → `screens/AdminHome.tsx`; `screens/owner/` →
  `screens/admin/`; route `/owner/:section?/:accountId?` → `/admin/…`;
  the redirect at login sends the administrator to `/admin`.
- `hooks/useIsOwner.ts` → `useIsAdmin.ts`; `hooks/useOwnerAccounts.ts` →
  `useAdminAccounts.ts`; query keys named `owner…` → `admin…`.
- `screens/auth/ClaimForm.tsx` → `SetupForm.tsx`: title "Set the
  administrator's password", password and confirmation, a Continue button,
  no username field. It posts to `/v1/server/setup` and logs in with the
  returned Session. `useServerState`'s type is `"setup" | "ready"`.
- `LocalAuthTabs.tsx`: setup shows `SetupForm` alone; ready shows Login
  alone. The Create Account tab, `CreateAccountForm.tsx` and the tab
  switching go. `useCreateAccountForm.ts` stays for
  `screens/settings/NewAccountPanel.tsx`.
- `ServerSettingsPanel.tsx` loses the registration checkbox and its caption.
- `lib/account.ts`, `lib/auditTrail.ts`, `AppAccountMenu`, `AppHeader`,
  `AuthGuard`, `ImportExportRoute`, the log viewer: comments and labels say
  administrator; code paths are unchanged.
- The remaining `owner` matches in `web/src` are the backup's owner in the
  import screens and `lib/importSources/`, and stay.
- `web-next/` is untouched.

## Desktop app

`src-tauri/src/` is searched for "owner"; any match that means the
administrator (expected in comments about the local server or the login)
is reworded, and the backup-owner and file-owner matches stay.

## Docs

- `docs/src/content/docs/docs/user/features/owner/` →
  `features/admin/`; its pages (Owner Home → Admin Home, run with Docker,
  run on another machine, troubleshooting) are reworded; links from
  `README.md`, `docker/compose.yml`, `docker/compose.release.yml` and the
  Astro sidebar follow.
- `user/your-messages/create-the-owner-and-an-account.md` is rewritten as
  "Set up the administrator and create an account": set the password on
  first visit, land on Admin Home, add an account from User Accounts.
- `developer/reference/errors/registration-closed.md` is removed and the
  errors index loses its row; the `not-the-owner` page becomes
  `not-the-admin`. The setup conflict is the existing `state-conflict`, as
  the claim conflict is today, so no page is added.
- `docs/adr/0008-the-owner-holds-no-messages.md` is renamed
  `0008-the-administrator-holds-no-messages.md` and rewritten in place for
  the fixed `admin` account, setup, and no registration, with an "Amended"
  note at the end naming this change and #3248. ADRs 0002, 0016, 0018 and
  0020 get the word change. ADR 0015 is untouched: it is about the backup's
  owner.
- `docs/architecture/*.md` and `docs/architecture/rules/*.md` get the word
  change with their rule ids unchanged. `rules/http-api.md` describes
  `POST /v1/server/setup`, the two states, the `admin` scope and the
  `not-the-admin` problem, and its line "there is no `/v1/owner/`" becomes
  "there is no `/v1/admin/`" with the same reason.
- `CONTEXT.md` as in Vocabulary. `CLAUDE.md` says administrator where it
  says owner. `AGENTS.md`'s "Run the server (development)" describes
  `--admin` and `set-admin-password`, and its schema-fingerprint row names
  Admin Home.
- `CHANGELOG.md`: one entry under the heading in development.

## Testing

- Server: a NULL-hash login is refused for `admin` and accepted for `demo`;
  `POST /v1/server/setup` answers `201` once and `409` after;
  `GET /v1/server` reads `setup` then `ready`; `POST /v1/accounts` with no
  credential is `401` and with a non-admin Session `403`; a new account
  named `admin` is refused; `set-admin-password` sets the first password and
  later replaces it and ends the Sessions; `create-database` yields a database
  with `admin` and no `demo`; the credential matrix passes with the `admin`
  scope.
- Web: `LocalAuthTabs.test.tsx` covers the two states; `SetupForm` posts
  without a username; renamed hooks and screens keep their tests.
- The whole run is `./scripts/check-all.sh`.

## Delivery

One branch, one pull request: the rename only makes sense whole. The
schema change needs no migration (`docs/adr/0017` and the rule that nothing
is backwards compatible). The PR uses the feature template.
