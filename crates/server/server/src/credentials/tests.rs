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

#[test]
fn auth_rate_limit_forgets_idle_buckets() {
    let limits = AuthRateLimits::default();
    let start = Instant::now();
    limits.count_at(&RateBucket::Password(1), start).unwrap();
    limits.count_at(&RateBucket::Password(2), start).unwrap();
    assert_eq!(limits.lock().unwrap().len(), 2);

    let later = start + AUTH_RATE_WINDOW + Duration::from_secs(1);
    limits.count_at(&RateBucket::Password(3), later).unwrap();
    let map = limits.lock().unwrap();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key(&RateBucket::Password(3).key()));
}

#[test]
fn auth_rate_limit_trips_after_max() {
    let limits = AuthRateLimits::frozen_at(Instant::now());
    let bucket = RateBucket::Register;
    for _ in 0..AUTH_RATE_MAX {
        limits.count(&bucket).unwrap();
    }
    let err = limits.count(&bucket).unwrap_err();
    match err {
        ApiError::RateLimited { retry_after_secs } => {
            assert!((1..=AUTH_RATE_WINDOW.as_secs()).contains(&retry_after_secs));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[test]
fn auth_rate_limits_do_not_cross_servers() {
    let now = Instant::now();
    let one = AuthRateLimits::frozen_at(now);
    let two = AuthRateLimits::frozen_at(now);
    let bucket = RateBucket::Register;
    for _ in 0..AUTH_RATE_MAX {
        one.count(&bucket).unwrap();
    }
    one.count(&bucket).unwrap_err();
    two.count(&bucket)
        .expect("a second server's limiter must not see the first server's hits");
}

/// A hit given back no longer counts, so attempts that are each given back
/// never fill the bucket, and a refused attempt is not counted at all.
#[test]
fn a_hit_given_back_no_longer_counts() {
    let limits = AuthRateLimits::frozen_at(Instant::now());
    let bucket = RateBucket::Password(TEST_ACCOUNT);
    for _ in 0..=AUTH_RATE_MAX {
        let hit = limits.count(&bucket).unwrap();
        limits.give_back(&bucket, hit).unwrap();
    }
    assert!(limits.lock().unwrap().is_empty(), "nothing is left counted");

    let hits: Vec<Hit> = (0..AUTH_RATE_MAX)
        .map(|_| limits.count(&bucket).unwrap())
        .collect();
    limits.count(&bucket).unwrap_err();
    limits.give_back(&bucket, hits[0]).unwrap();
    limits
        .count(&bucket)
        .expect("one hit given back leaves room for one attempt");
    limits.count(&bucket).unwrap_err();
}

/// Logins as unknown usernames share one count across the server, which
/// holds more than an account's, and each username keeps its own count
/// beneath it.
#[test]
fn unknown_usernames_share_one_larger_count_across_the_server() {
    let limits = AuthRateLimits::frozen_at(Instant::now());
    for n in 0..UNKNOWN_USERNAME_RATE_MAX {
        limits.count(&RateBucket::UnknownUsernames).unwrap();
        limits
            .count(&RateBucket::unknown_username(&format!("nobody{n}")))
            .unwrap();
    }
    limits.count(&RateBucket::UnknownUsernames).unwrap_err();
    assert_eq!(
        RateBucket::unknown_username("NoBody"),
        RateBucket::unknown_username("nobody"),
        "the spellings of one username share its count"
    );
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
    let old_session = session_tokens::insert_account_session_token(&mut conn, TEST_ACCOUNT)
        .await
        .unwrap();
    let first_api_token = api_tokens::create_api_token(
        &mut conn,
        TEST_ACCOUNT,
        "backup client",
        Permissions::all(),
        None,
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

    let new_session = change_password_on_conn(&mut conn, TEST_ACCOUNT, Some(&new_hash))
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
        session_tokens::lookup_session(&mut conn, &old_session)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        session_tokens::lookup_session(&mut conn, &new_session)
            .await
            .unwrap()
            .map(|s| s.account_id),
        Some(TEST_ACCOUNT)
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &api_token)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &other_account_token)
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
        change_password_on_conn(&mut conn, TEST_ACCOUNT, Some(&new_hash))
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
        session_tokens::lookup_session(&mut conn, &old_session)
            .await
            .unwrap()
            .map(|s| s.account_id),
        Some(TEST_ACCOUNT)
    );
    for api_token in api_tokens {
        assert!(
            crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &api_token)
                .await
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(
        crate::db::api_tokens::lookup_account_for_api_token(&mut conn, &other_account_token)
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

    // The maximum is in bytes, not characters, because that is what Argon2
    // costs. Exactly the limit is accepted; one byte more is not.
    let at_limit = "a".repeat(MAX_PASSWORD_BYTES);
    let owner_at_limit = hash_owner_password(&at_limit)
        .await
        .expect("exactly the limit is allowed");
    assert!(verify_password(&owner_at_limit, &at_limit));
    hash_user_password(&at_limit)
        .await
        .expect("exactly the limit is allowed");
    let over = "a".repeat(MAX_PASSWORD_BYTES + 1);
    for refused in [
        hash_owner_password(&over).await.map(Some),
        hash_user_password(&over).await,
    ] {
        assert!(
            matches!(
                refused,
                Err(ApiError::ValidationFailed(ref m)) if m == &["password is too long"]
            ),
            "one byte over: {refused:?}"
        );
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
/// `is_valid_username` had no test either. A username is the name an account
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
        assert!(is_valid_username(good), "{good:?} must be accepted");
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
        assert!(!is_valid_username(bad), "{bad:?} must be refused");
    }

    // Letters from other scripts are refused. The doc comment on
    // `is_valid_username` gives the reason.
    for other_script in ["аdmin", "mätt", "матт", "ｍatt", "matt\u{0301}", "٣"] {
        assert!(
            !is_valid_username(other_script),
            "{other_script:?} must be refused"
        );
    }

    // The length boundary, in both directions.
    assert!(is_valid_username(&"a".repeat(128)), "128 is allowed");
    assert!(!is_valid_username(&"a".repeat(129)), "129 is not");

    // Surrounding whitespace is trimmed before the rule is applied, so a
    // pasted name with a trailing newline is accepted and stored trimmed.
    assert!(is_valid_username("  matt  "));
    assert_eq!(normalize_username("  matt\n"), "matt");
}
