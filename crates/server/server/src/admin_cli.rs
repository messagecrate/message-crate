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

use std::io::{BufRead, IsTerminal as _};

use anyhow::{Context as _, Result, bail};
use chrono::Utc;

use crate::db::account_profile;
use crate::db::audit_trail::{self, AuditAction, AuditActor};
use crate::open_db::OpenDb;

/// The administrator's password from standard input. At a terminal it is asked for
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

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::problem::ProblemType;
    use crate::test_support::{
        expect_problem, expect_problem_for, get_json, get_raw, log_in, log_in_raw, login_status,
        test_fixture,
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

    /// In setup the command sets the first password and the Message Crate
    /// becomes ready; when ready it replaces the password and ends the
    /// administrator's Sessions, which is the way back in when it is lost.
    #[tokio::test]
    async fn set_admin_password_works_in_setup_and_when_ready() {
        let fixture = test_fixture().await;
        let state = fixture.state.clone();
        let opened = OpenDb {
            cfg: (*state.cfg).clone(),
            db: state.db.clone(),
            wal_error: None,
        };
        assert_eq!(
            set_admin_password(&opened, "first").await.unwrap(),
            SetAdminPasswordOutcome::FirstPassword
        );
        let server: serde_json::Value = get_json(&state, "/v1/server", "").await;
        assert_eq!(server["state"], "ready");
        let admin = log_in(&state, account_profile::ADMIN_USERNAME, "first").await;
        let token = admin["token"].as_str().unwrap();

        assert_eq!(
            set_admin_password(&opened, "second").await.unwrap(),
            SetAdminPasswordOutcome::Replaced
        );
        let (status, text) = get_raw(&state, "/v1/session", token).await;
        expect_problem_for(
            "the Session the old password opened",
            status,
            &text,
            ProblemType::AuthenticationRequired,
        );
        let (status, text) = log_in_raw(&state, account_profile::ADMIN_USERNAME, "first").await;
        expect_problem(status, &text, ProblemType::InvalidCredentials);
        assert_eq!(
            login_status(&state, account_profile::ADMIN_USERNAME, "second").await,
            StatusCode::CREATED
        );
    }

    /// An empty password is refused in both states.
    #[tokio::test]
    async fn set_admin_password_refuses_an_empty_password() {
        let fixture = test_fixture().await;
        let state = fixture.state.clone();
        let opened = OpenDb {
            cfg: (*state.cfg).clone(),
            db: state.db.clone(),
            wal_error: None,
        };
        let error = set_admin_password(&opened, "")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("must have a password"), "{error}");
        set_admin_password(&opened, "first").await.unwrap();
        let error = set_admin_password(&opened, "")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("must have a password"), "{error}");
    }
}
