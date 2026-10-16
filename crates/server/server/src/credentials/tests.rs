use super::*;
use crate::db::engine;
use crate::db::permissions::Permissions;
use crate::db::schema;

const TEST_ACCOUNT: i64 = 7;
const OTHER_ACCOUNT: i64 = 8;

/// Passwords hashed before the argon2 0.6 upgrade must still let people in.
///
/// These three strings were produced by argon2 0.5.3, the version the server
/// shipped with, using the salt scheme it used then: sixteen bytes from the
/// system RNG, base64-encoded, passed to `Argon2::default()`. If a future
/// upgrade stops them verifying, every account created before that upgrade
/// is locked out, and no other test in this suite would notice — the rest
/// all hash and verify with the same version in the same process.
#[test]
fn hashes_written_by_argon2_0_5_still_verify() {
    const LEGACY: &[(&str, &str)] = &[
        (
            "hunter2hunter2",
            "$argon2id$v=19$m=19456,t=2,p=1$/ic5l4xy5HAgEHBiuv0t3A$iI1c7vmoqfa79pGmE3/iquM09ezwKoYA8U1dxtWH/rg",
        ),
        (
            "",
            "$argon2id$v=19$m=19456,t=2,p=1$Mp73mogaQlz3ZqmokZzY/A$CVBX4QUiJTjS5u4sLQ7rvMEuSo8e6c1izYXZTtJo+RQ",
        ),
        (
            "pässwörd with spaces 🔐",
            "$argon2id$v=19$m=19456,t=2,p=1$l9S0iAuO7kK+iuZf99EN9g$FQ35n77YztPzFZAYqhZnyRLK6TUg7nuXUGiO7Nx1s90",
        ),
    ];
    for (password, hash) in LEGACY {
        assert!(
            verify_password(hash, password),
            "argon2 0.5 hash must still verify for {password:?}"
        );
        assert!(
            !verify_password(hash, "wrong-password"),
            "the wrong password must still be refused for {password:?}"
        );
    }
}

/// The parameters the server writes must not drift silently. A weaker
/// memory or time cost would be a security regression that still passes
/// every round-trip test, because hashing and verifying would agree.
#[test]
fn a_new_hash_keeps_argon2id_and_its_default_cost() {
    let hash = hash_password("hunter2hunter2").expect("hash");
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "unexpected argon2 parameters: {hash}"
    );
    assert!(verify_password(&hash, "hunter2hunter2"));
    assert!(!verify_password(&hash, "hunter2hunter3"));
}

/// Two hashes of the same password must differ, or the salt is not doing
/// its job. argon2 0.6 generates the salt itself now, so this is the check
/// that the generated salt is actually random rather than fixed.
#[test]
fn the_same_password_hashes_differently_every_time() {
    let a = hash_password("hunter2hunter2").expect("hash");
    let b = hash_password("hunter2hunter2").expect("hash");
    assert_ne!(a, b, "a repeated hash means the salt is not random");
    assert!(verify_password(&a, "hunter2hunter2"));
    assert!(verify_password(&b, "hunter2hunter2"));
}

#[test]
fn a_username_is_trimmed_and_checked() {
    assert_eq!(require_valid_username("  alice ").unwrap(), "alice");
    assert!(matches!(
        require_valid_username("no spaces here"),
        Err(ApiError::ValidationFailed(_))
    ));
    assert!(matches!(
        require_valid_username("   "),
        Err(ApiError::ValidationFailed(_))
    ));
}

/// A username over its cap is refused in the wording every length cap
/// shares (`text_caps`), counted after the trim like every other.
#[test]
fn a_long_username_is_refused_in_the_wording_of_every_length_cap() {
    assert_eq!(
        require_valid_username(&format!(" {} ", "a".repeat(128))).unwrap(),
        "a".repeat(128)
    );
    match require_valid_username(&"a".repeat(129)) {
        Err(ApiError::ValidationFailed(errors)) => assert_eq!(
            errors,
            ["username: must be at most 128 characters".to_string()]
        ),
        other => panic!("expected validation-failed, got {other:?}"),
    }
}

/// `serve`'s wait sleeps [`REFUSAL_WAIT`] before it answers, and the answer
/// is `invalid-credentials` with the sentence it was given. The clock is
/// tokio's paused one, so the test takes no time and reads no wall clock.
#[tokio::test(start_paused = true)]
async fn a_refusal_sleeps_the_refusal_wait_then_answers_invalid_credentials() {
    let start = tokio::time::Instant::now();
    let err = RefusalWait::default().refuse("nope").await;
    assert!(start.elapsed() >= REFUSAL_WAIT, "{:?}", start.elapsed());
    assert!(
        matches!(&err, ApiError::InvalidCredentials(detail) if detail == "nope"),
        "{err:?}"
    );
}

/// The test harness's wait counts each refusal and does not sleep.
#[tokio::test(start_paused = true)]
async fn a_counted_wait_counts_each_refusal_without_sleeping() {
    let wait = RefusalWait::counted();
    let start = tokio::time::Instant::now();
    wait.refuse("one").await;
    wait.refuse("two").await;
    assert_eq!(start.elapsed(), Duration::ZERO);
    assert_eq!(wait.waits(), 2);
    assert_eq!(wait.clone().waits(), 2, "a clone shares the count");
}

async fn password_change_setup() -> (
    tempfile::TempDir,
    sqlx::pool::PoolConnection<sqlx::Sqlite>,
    String,
    Vec<String>,
    String,
) {
    let (pool, dir) = engine::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    schema::ensure_schema(&mut conn).await.unwrap();
    let old_hash = hash_password("old-password").unwrap();
    account_profile::insert_account_at(&mut conn, TEST_ACCOUNT, "alice", Some(&old_hash), None)
        .await
        .unwrap();
    account_profile::insert_account_at(&mut conn, OTHER_ACCOUNT, "bob", Some(&old_hash), None)
        .await
        .unwrap();
    let old_session = session_tokens::insert_account_session_token(
        &mut conn,
        TEST_ACCOUNT,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();
    let first_api_token = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "backup client",
        Permissions::all(),
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap()
    .token;
    let second_api_token = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "export client",
        Permissions {
            import: false,
            export: true,
            delete: false,
        },
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap()
    .token;
    let other_account_token = api_tokens::create_api_token(
        &mut conn,
        OTHER_ACCOUNT,
        "other account client",
        Permissions::all(),
        None,
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap()
    .token;
    (
        dir,
        conn,
        old_session,
        vec![first_api_token, second_api_token],
        other_account_token,
    )
}

#[tokio::test]
async fn change_password_transaction_updates_all_credentials() {
    let (_dir, mut conn, old_session, api_tokens, other_account_token) =
        password_change_setup().await;
    let new_hash = hash_password("new-password").unwrap();

    let new_session = change_password_on_conn(
        &mut conn,
        TEST_ACCOUNT,
        Some(&new_hash),
        crate::test_support::fixed_instant(),
    )
    .await
    .unwrap();

    let stored_hash = account_profile::load_password_hash(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap()
        .unwrap();
    assert!(
        passwords_match(Some(&stored_hash), "new-password")
            .await
            .unwrap()
    );
    assert!(
        session_tokens::lookup_session(
            &mut conn,
            &old_session,
            crate::test_support::fixed_instant()
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        session_tokens::lookup_session(
            &mut conn,
            &new_session,
            crate::test_support::fixed_instant()
        )
        .await
        .unwrap()
        .map(|s| s.account_id),
        Some(TEST_ACCOUNT)
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(
                &mut conn,
                &api_token,
                crate::test_support::fixed_instant()
            )
            .await
            .unwrap()
            .is_none()
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(
            &mut conn,
            &other_account_token,
            crate::test_support::fixed_instant()
        )
        .await
        .unwrap()
        .unwrap()
        .account_id,
        OTHER_ACCOUNT
    );
}

#[tokio::test]
async fn change_password_transaction_rolls_back_every_credential() {
    let (_dir, mut conn, old_session, api_tokens, other_account_token) =
        password_change_setup().await;
    sqlx::query(
        "CREATE TRIGGER fail_session_rotation
         BEFORE UPDATE ON account_session_tokens
         BEGIN
             SELECT RAISE(FAIL, 'injected session rotation failure');
         END",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    let new_hash = hash_password("new-password").unwrap();

    assert!(
        change_password_on_conn(
            &mut conn,
            TEST_ACCOUNT,
            Some(&new_hash),
            crate::test_support::fixed_instant()
        )
        .await
        .is_err()
    );

    let stored_hash = account_profile::load_password_hash(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap()
        .unwrap();
    assert!(
        passwords_match(Some(&stored_hash), "old-password")
            .await
            .unwrap()
    );
    assert_eq!(
        session_tokens::lookup_session(
            &mut conn,
            &old_session,
            crate::test_support::fixed_instant()
        )
        .await
        .unwrap()
        .map(|s| s.account_id),
        Some(TEST_ACCOUNT)
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(
                &mut conn,
                &api_token,
                crate::test_support::fixed_instant()
            )
            .await
            .unwrap()
            .is_some()
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(
            &mut conn,
            &other_account_token,
            crate::test_support::fixed_instant()
        )
        .await
        .unwrap()
        .unwrap()
        .account_id,
        OTHER_ACCOUNT
    );
}

/// The password length rules, at their edges.
///
/// The owner must have a password and one character is enough. A user
/// account has no minimum at all: an empty password is an account with no
/// password. Both share the maximum, the only thing stopping a megabyte of
/// text reaching Argon2, which would hash it and make every login slow for
/// everybody.
///
/// The hashes are made and checked on the blocking thread pool, and the
/// refusals must still come back as the `validation-failed` they were.
#[tokio::test]
async fn the_owner_needs_one_character_and_a_user_needs_none() {
    assert!(
        matches!(
            hash_owner_password("").await,
            Err(ApiError::ValidationFailed(ref m)) if m == &["the owner must have a password"]
        ),
        "an owner with no password"
    );
    let owner_hash = hash_owner_password("a")
        .await
        .expect("one character is the minimum");
    assert!(verify_password(&owner_hash, "a"));
    assert!(passwords_match(Some(&owner_hash), "a").await.unwrap());
    assert!(!passwords_match(Some(&owner_hash), "b").await.unwrap());

    assert_eq!(
        hash_user_password("").await.expect("empty is allowed"),
        None,
        "an empty password is no password"
    );
    let user_hash = hash_user_password("a")
        .await
        .expect("one character is allowed")
        .expect("and is a real password");
    assert!(verify_password(&user_hash, "a"));
    assert!(verify_login_password(Some(&user_hash), "a").await.unwrap());
    assert!(!verify_login_password(Some(&user_hash), "b").await.unwrap());

    // The maximum is in characters, as a username's is, not bytes: 128
    // letters pass whether each takes one byte, two or four. One more is
    // refused.
    for letter in ["a", "é", "🔑"] {
        let at_limit = letter.repeat(MAX_PASSWORD_CHARS);
        let owner_at_limit = hash_owner_password(&at_limit)
            .await
            .expect("exactly the limit is allowed");
        assert!(verify_password(&owner_at_limit, &at_limit));
        hash_user_password(&at_limit)
            .await
            .expect("exactly the limit is allowed");
        let over = letter.repeat(MAX_PASSWORD_CHARS + 1);
        for refused in [
            hash_owner_password(&over).await.map(Some),
            hash_user_password(&over).await,
        ] {
            assert!(
                matches!(
                    refused,
                    Err(ApiError::ValidationFailed(ref m))
                        if m == &["password: must be at most 128 characters"]
                ),
                "one {letter:?} over: {refused:?}"
            );
        }
    }
}

/// A login to an account with no password takes the empty password only.
#[tokio::test]
async fn a_login_to_an_account_with_no_password_takes_only_the_empty_one() {
    for no_hash in [None, Some("")] {
        assert!(verify_login_password(no_hash, "").await.unwrap());
        assert!(!verify_login_password(no_hash, "anything").await.unwrap());
    }
}

/// An Argon2 run holds its permit for as long as it runs, so no more runs
/// go at once than there are permits, and gives it back when it ends.
#[tokio::test]
async fn an_argon2_run_holds_its_permit_while_it_runs() {
    let permits = Arc::new(Semaphore::new(1));
    let seen_by_the_run = Arc::clone(&permits);
    let free_during_the_run = run_argon2_under(&permits, "test run", move || {
        Ok(seen_by_the_run.available_permits())
    })
    .await
    .unwrap();
    assert_eq!(free_during_the_run, 0, "the run holds the only permit");
    assert_eq!(permits.available_permits(), 1, "and gives it back");
}

/// A request dropped while its Argon2 run goes on keeps the permit taken
/// until the run itself ends, so a client that disconnects cannot start more
/// runs than there are permits.
#[tokio::test]
async fn a_dropped_request_keeps_its_permit_until_the_run_ends() {
    let permits = Arc::new(Semaphore::new(1));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let request_permits = Arc::clone(&permits);
    let request = crate::request_id::spawn_in_current_span(async move {
        run_argon2_under(&request_permits, "test run", move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
            Ok(())
        })
        .await
    });
    started_rx.await.expect("the run starts");
    request.abort();
    assert!(request.await.is_err(), "the request is dropped");
    assert_eq!(
        permits.available_permits(),
        0,
        "the run still holds its permit"
    );

    release_tx.send(()).expect("the run is still waiting");
    // Taken only once the run gives it back.
    let _permit = permits.acquire().await.expect("the permit comes back");
}

/// The username rule, at both its edges and over its character set.
///
/// The username check had no test either. A username is the name an account
/// is looked up by at login, so accepting a blank one, or one with a slash
/// or a space in it, makes an account that is awkward or impossible to reach.
#[test]
fn a_username_is_one_to_128_characters_of_a_known_set() {
    for good in [
        "matt",
        "Matt",
        "matt.beisser",
        "matt-beisser",
        "matt_beisser",
        "m",
        "user123",
    ] {
        assert!(
            require_valid_username(good).is_ok(),
            "{good:?} must be accepted"
        );
    }

    for bad in [
        "",
        "   ",
        "matt beisser",
        "matt/beisser",
        "matt@example.com",
        "matt:beisser",
        "matt\nbeisser",
    ] {
        assert!(
            require_valid_username(bad).is_err(),
            "{bad:?} must be refused"
        );
    }

    // Letters from other scripts are refused. The doc comment on
    // `require_valid_username` gives the reason.
    for other_script in ["аdmin", "mätt", "матт", "ｍatt", "matt\u{0301}", "٣"] {
        assert!(
            require_valid_username(other_script).is_err(),
            "{other_script:?} must be refused"
        );
    }

    // The length boundary, in both directions.
    assert!(
        require_valid_username(&"a".repeat(128)).is_ok(),
        "128 is allowed"
    );
    assert!(
        require_valid_username(&"a".repeat(129)).is_err(),
        "129 is not"
    );

    // Surrounding whitespace is trimmed before the rule is applied, so a
    // pasted name with a trailing newline is accepted and stored trimmed.
    assert!(require_valid_username("  matt  ").is_ok());
    assert_eq!(normalize_username("  matt\n"), "matt");
}

/// The API reference states the password cap on every password a request
/// sends, as `maxLength` and in words, so a client learns it before the
/// server refuses one. `max_length` takes only a literal, so this holds each
/// to [`MAX_PASSWORD_CHARS`]. `password_confirmation` must equal `password`,
/// so its description says that instead.
#[test]
fn the_reference_states_the_password_cap() {
    let doc: serde_json::Value =
        serde_json::from_str(&crate::openapi::dump_openapi_json()).unwrap();
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let mut found = Vec::new();
    for (schema, body) in schemas {
        let Some(properties) = body["properties"].as_object() else {
            continue;
        };
        for (field, property) in properties {
            let is_string = property["type"] == "string"
                || property["type"]
                    .as_array()
                    .is_some_and(|types| types.iter().any(|t| t == "string"));
            if !field.contains("password") || !is_string {
                continue;
            }
            found.push(format!("{schema}.{field}"));
            assert_eq!(
                property["maxLength"], MAX_PASSWORD_CHARS,
                "{schema}.{field}: {property}"
            );
            if field != "password_confirmation" {
                let described = property["description"]
                    .as_str()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                assert!(
                    described.contains("128 characters"),
                    "{schema}.{field} does not state the cap: {described}"
                );
            }
        }
    }
    found.sort();
    assert_eq!(
        found,
        [
            "ClaimServerRequest.password",
            "CreateAccountRequest.password",
            "CreateSessionRequest.password",
            "DeleteAccountRequest.current_password",
            "ReplaceAccountPasswordRequest.current_password",
            "ReplaceAccountPasswordRequest.password",
            "ReplaceAccountPasswordRequest.password_confirmation",
        ],
        "every password field a request sends"
    );
}
