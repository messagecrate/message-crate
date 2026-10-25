//! The two shell commands that reach the owner's credentials.
//!
//! Claiming a Message Crate and getting back into one are different jobs, so they are
//! different commands: `create-owner` refuses a Message Crate that already has an
//! owner, `reset-owner-password` refuses one that does not. Neither can be
//! mistaken for the other, so setting up a Message Crate cannot silently overwrite a
//! live owner's password.
//!
//! A shell on the server is the right credential for both. Nothing inside the
//! product can reset the owner's password, because no account stands above
//! the owner. See `docs/adr/0008-the-owner-holds-no-messages.md`.
//!
//! Both read the password from standard input ([`read_password`]) and never
//! from an argument, because a command's arguments are visible to every
//! local user through `ps` while it runs and stay in the shell's history
//! afterwards (#2094).

use std::io::{BufRead, IsTerminal as _};

use anyhow::{Context as _, Result, bail};
use chrono::Utc;

use crate::db::account_profile;
use crate::db::audit_trail::{self, AuditAction, AuditActor, NewEntry};
use crate::open_db::OpenDb;

/// The owner's password from standard input. At a terminal it is asked for
/// with `prompt` and typed without echo; otherwise the first line is read
/// ([`read_password_line`]), so a script can pipe it in.
///
/// # Errors
///
/// Fails when standard input cannot be read or holds no line.
pub fn read_password(prompt: &str) -> Result<String> {
    if std::io::stdin().is_terminal() {
        rpassword::prompt_password(prompt).context("could not read the password from the terminal")
    } else {
        read_password_line(&mut std::io::stdin().lock())
    }
}

/// The first line of `input`, without its line ending: the password exactly
/// as written, spaces included.
///
/// # Errors
///
/// Fails when `input` cannot be read, or ends before any line, which names
/// how to give the password.
pub fn read_password_line(input: &mut impl BufRead) -> Result<String> {
    let mut line = String::new();
    let read = input
        .read_line(&mut line)
        .context("could not read the password from standard input")?;
    if read == 0 {
        bail!(
            "no password on standard input: type it when asked, or write it on the first line \
             of standard input"
        );
    }
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

/// Create the owner, claiming an unclaimed Message Crate.
///
/// # Errors
///
/// Fails when this Message Crate already has an owner, when the username is malformed
/// or taken, or when the password is empty.
pub async fn create_owner(opened: &OpenDb, username: &str, password: &str) -> Result<String> {
    let username = match crate::credentials::require_valid_username(username) {
        Ok(username) => username,
        Err(e) => bail!("{e}"),
    };
    // Hashed before a connection is taken, so none is held for the hash.
    let hash = match crate::credentials::hash_admin_password(password).await {
        Ok(hash) => hash,
        Err(e) => bail!("{e}"),
    };
    let mut conn = opened.conn().await?;

    if account_profile::admin_has_password(&mut conn).await? {
        bail!(
            "this Message Crate already has an owner; use `reset-owner-password` to set a new password for it"
        );
    }
    if let Err(e) = crate::credentials::require_username_free(&mut conn, &username).await {
        bail!("{e}");
    }

    account_profile::insert_account_at(
        &mut conn,
        account_profile::ADMIN_ACCOUNT_ID,
        &username,
        Some(&hash),
        None,
    )
    .await?;
    audit_trail::record(
        &mut conn,
        &NewEntry::about(
            AuditAction::AccountCreated,
            AuditActor::CommandLine,
            (account_profile::ADMIN_ACCOUNT_ID, &username),
        ),
        Utc::now(),
    )
    .await?;

    Ok(username)
}

/// Set a new password for an existing owner and end their sessions.
///
/// Returns the owner's username, which is as easy to forget as the password
/// and just as unreachable from inside the product.
///
/// # Errors
///
/// Fails when this Message Crate has no owner, or when the password is empty.
pub async fn reset_owner_password(opened: &OpenDb, password: &str) -> Result<String> {
    // Hashed before a connection is taken, so none is held for the hash.
    let hash = match crate::credentials::hash_admin_password(password).await {
        Ok(hash) => hash,
        Err(e) => bail!("{e}"),
    };
    let mut conn = opened.conn().await?;
    if !account_profile::admin_has_password(&mut conn).await? {
        bail!("this Message Crate has no owner yet; use `create-owner` to claim it");
    }

    // The password, its record and the end of the sessions the old password
    // opened land together.
    let mut tx = crate::db::begin_write(&mut conn).await?;
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
    // The old password is gone, so every session it opened should be too.
    crate::db::session_tokens::revoke_account_sessions(
        &mut tx,
        account_profile::ADMIN_ACCOUNT_ID,
        AuditActor::CommandLine,
        now,
    )
    .await?;
    tx.commit().await?;

    let username =
        account_profile::username_for_account(&mut conn, account_profile::ADMIN_ACCOUNT_ID)
            .await?
            .unwrap_or_else(|| account_profile::ADMIN_ACCOUNT_ID.to_string());

    Ok(username)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::problem::ProblemType;
    use crate::test_support::{
        claim_as_owner, expect_problem, expect_problem_for, get_raw, get_status, log_in_raw,
        login_status, test_fixture,
    };

    /// The first line is the password exactly as written: its line ending
    /// is dropped, `\r\n` as well as `\n`, and nothing else is, so spaces at
    /// either end stay part of it.
    #[test]
    fn the_password_is_the_first_line_without_its_line_ending() {
        for (input, password) in [
            ("hunter2hunter2\n", "hunter2hunter2"),
            ("hunter2hunter2\r\n", "hunter2hunter2"),
            ("hunter2hunter2", "hunter2hunter2"),
            (" spaced out \nsecond line\n", " spaced out "),
            ("\n", ""),
        ] {
            assert_eq!(
                read_password_line(&mut input.as_bytes()).unwrap(),
                password,
                "{input:?}"
            );
        }
    }

    /// Standard input that ends before any line says how to give the
    /// password, instead of setting an empty one.
    #[test]
    fn no_line_on_standard_input_is_refused() {
        let err = read_password_line(&mut "".as_bytes()).unwrap_err();
        assert!(
            err.to_string().starts_with("no password on standard input"),
            "{err}"
        );
    }

    /// A session opened before the reset is refused after it, so whoever
    /// held the old password is signed out; the new password logs in and the
    /// old one does not.
    #[tokio::test]
    async fn resetting_the_owner_password_signs_out_the_sessions_it_opened() {
        let fixture = test_fixture().await;
        let state = fixture.state.clone();
        let owner = claim_as_owner(&state, "keeper", "hunter2hunter2").await;
        assert_eq!(
            get_status(&state, "/v1/session", &owner.token).await,
            StatusCode::OK
        );

        let shell = OpenDb {
            cfg: (*state.cfg).clone(),
            db: state.db.clone(),
            wal_error: None,
        };
        let username = reset_owner_password(&shell, "a new owner password")
            .await
            .unwrap();
        assert_eq!(username, "keeper");

        let (status, text) = get_raw(&state, "/v1/session", &owner.token).await;
        expect_problem_for(
            "the session the old password opened",
            status,
            &text,
            ProblemType::AuthenticationRequired,
        );
        let (status, text) = log_in_raw(&state, "keeper", "hunter2hunter2").await;
        expect_problem(status, &text, ProblemType::InvalidCredentials);
        assert_eq!(
            login_status(&state, "keeper", "a new owner password").await,
            StatusCode::CREATED
        );
    }
}
