//! Completing an Import Run: the message count it records, and the
//! Saved Search and Contact Group it makes.

use super::*;

/// The account that owns Import Run `import_id`.
async fn run_account(state: &crate::server::AppState, import_id: i64) -> i64 {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar("SELECT account_id FROM imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap()
}

/// Finishing a run that stored messages leaves two shortcuts behind: a
/// saved search whose query is the run's id, and a Contact Group holding
/// exactly the contacts the run recorded touching. Both carry the source
/// and the day the run finished in their names, and both are marked
/// `import` so the sidebar can tell them from what a person made.
#[tokio::test]
async fn completing_an_import_with_messages_creates_its_saved_search_and_contact_group() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        wipe_test_batch("whatsapp", &["g-1", "g-2"]),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    let date = complete_run(&state, &token, import_id).await;
    let (searches, groups) = shortcuts(&state, run_account(&state, import_id).await).await;

    assert_eq!(
        searches,
        [(
            format!("Import whatsapp {date}"),
            format!("import:#{import_id}"),
            "import".to_string()
        )]
    );

    let mut conn = state.db.acquire().await.unwrap();
    let touched = crate::db::import_contacts::contact_ids(&mut conn, import_id)
        .await
        .unwrap();
    assert!(!touched.is_empty(), "the run must have touched a contact");
    assert_eq!(
        groups,
        [(
            format!("whatsapp import {date}"),
            "import".to_string(),
            touched
        )]
    );
    drop(conn);

    // The stored query has to be one the search accepts, and it has to
    // answer with this run's messages only. A second run's message is the
    // one it must leave out (#950).
    import_one_batch(
        &state,
        &token,
        "sms-backup-restore",
        "append",
        wipe_test_batch("sms-backup-restore", &["g-later"]),
    )
    .await;
    let stored = searches[0].1.replace(':', "%3A").replace('#', "%23");
    let page: serde_json::Value =
        get_json(&state, &format!("/v1/messages?q={stored}"), &token).await;
    let mut texts: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    texts.sort_unstable();
    assert_eq!(texts, ["g-1", "g-2"], "{page}");
}

/// The counts of a finished run are the server's own, never the client's.
/// A resumed Upload's report counts only what the resume sent, which
/// is nothing when the run's first `/complete` was refused after every
/// message landed. A count from the client would record that run as
/// holding no messages, and give it no Saved Search.
#[tokio::test]
async fn completing_a_run_counts_its_messages_whatever_the_body_says() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        wipe_test_batch("whatsapp", &["g-1", "g-2"]),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    let _: serde_json::Value = post_json(
        &state,
        &format!("/v1/imports/{import_id}/complete"),
        &token,
        serde_json::json!({ "status": "completed", "message_count": 0, "attachment_count": 0 }),
    )
    .await;

    let completed: serde_json::Value =
        get_json(&state, &format!("/v1/imports/{import_id}"), &token).await;
    assert_eq!(completed["message_count"], 2, "{completed}");
    let (searches, _) = shortcuts(&state, run_account(&state, import_id).await).await;
    assert_eq!(searches.len(), 1, "the run's saved search: {searches:?}");
}

/// A run that stored nothing gets no saved search: one matching no
/// messages would only clutter the sidebar, and the run stays visible in
/// Import History regardless. With no contacts touched there is no Contact
/// Group either.
#[tokio::test]
async fn completing_an_import_with_no_messages_creates_no_saved_search() {
    let (state, _fixture, token) = importer().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &state,
        "/v1/imports",
        &token,
        serde_json::json!({ "source": "whatsapp" }),
    )
    .await;
    let import_id = created["id"].as_i64().unwrap();

    complete_run(&state, &token, import_id).await;
    let completed: serde_json::Value =
        get_json(&state, &format!("/v1/imports/{import_id}"), &token).await;
    assert_eq!(completed["message_count"], 0, "{completed}");

    let (searches, groups) = shortcuts(&state, run_account(&state, import_id).await).await;
    assert!(
        searches.is_empty(),
        "no saved search for an empty run: {searches:?}"
    );
    assert!(
        groups.is_empty(),
        "no Contact Group for an empty run: {groups:?}"
    );
}

/// Each Import Run gets a Contact Group of its own. A second run from the
/// same source on the same day takes the next free name, and neither group
/// holds the other run's contacts (#956).
#[tokio::test]
async fn two_import_runs_from_one_source_on_one_day_make_two_contact_groups() {
    let (state, _fixture, token) = importer().await;
    let (first, first_date) = completed_run(
        &state,
        &token,
        "whatsapp",
        wipe_test_batch("whatsapp", &["g-1"]),
    )
    .await;
    // A different person, so the second run has a contact of its own to record.
    let second_body = wipe_test_batch("whatsapp", &["g-2"]).replace("+15555550107", "+15555550108");
    let (second, second_date) = completed_run(&state, &token, "whatsapp", second_body).await;

    let mut conn = state.db.acquire().await.unwrap();
    let first_touched = crate::db::import_contacts::contact_ids(&mut conn, first)
        .await
        .unwrap();
    let second_touched = crate::db::import_contacts::contact_ids(&mut conn, second)
        .await
        .unwrap();
    drop(conn);
    assert!(!first_touched.is_empty() && !second_touched.is_empty());
    assert!(
        first_touched.iter().all(|id| !second_touched.contains(id)),
        "the runs must touch different contacts: {first_touched:?} {second_touched:?}"
    );

    // The suffix appears when both runs finished on one UTC day, which is
    // every run of this test except one that straddles midnight.
    let second_name = if second_date == first_date {
        format!("whatsapp import {second_date} 2")
    } else {
        format!("whatsapp import {second_date}")
    };
    let (_, groups) = shortcuts(&state, run_account(&state, first).await).await;
    assert_eq!(
        groups,
        [
            (
                format!("whatsapp import {first_date}"),
                "import".to_string(),
                first_touched
            ),
            (second_name, "import".to_string(), second_touched),
        ]
    );
}

/// A Contact Group a person made is theirs, even under the name an import
/// would use: the import leaves its kind and its members alone and takes
/// the next free name for its own group. A name is taken whatever its
/// case, so the hand-made ` 2` in capitals sends the import to ` 3` (#956).
#[tokio::test]
async fn an_import_leaves_a_hand_made_contact_group_with_its_name_alone() {
    let (state, _fixture, token) = importer().await;
    let path = batches_path(&state, &token, "whatsapp").await;
    let import_id: i64 = path
        .trim_start_matches("/v1/imports/")
        .trim_end_matches("/batches")
        .parse()
        .unwrap();
    let account = run_account(&state, import_id).await;
    let (status, text) = crate::test_support::post_raw(
        &state,
        &path,
        &token,
        "application/jsonl",
        wipe_test_batch("whatsapp", &["g-1"]),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{text}");

    // The hand-made groups are named for today and for tomorrow, so the
    // test holds when the run finishes just past midnight UTC.
    let spec = crate::db::named_membership::group_spec();
    let mut conn = state.db.acquire().await.unwrap();
    let friend = crate::db::contacts::create_contact(
        &mut conn,
        account,
        "Friend",
        crate::db::contacts::Origin::User,
    )
    .await
    .unwrap();
    let today = chrono::Utc::now().date_naive();
    for day in [today, today.succ_opt().unwrap()] {
        for name in [
            format!("whatsapp import {day}"),
            format!("WHATSAPP IMPORT {day} 2"),
        ] {
            let (group, _) =
                crate::db::named_membership::create_set(spec, &mut conn, account, &name)
                    .await
                    .unwrap();
            crate::db::named_membership::patch_members(
                spec,
                &mut conn,
                account,
                group,
                &[friend],
                &[],
            )
            .await
            .unwrap();
        }
    }
    drop(conn);

    let date = complete_run(&state, &token, import_id).await;

    let mut conn = state.db.acquire().await.unwrap();
    let touched = crate::db::import_contacts::contact_ids(&mut conn, import_id)
        .await
        .unwrap();
    drop(conn);
    assert!(!touched.is_empty() && !touched.contains(&friend));
    let (_, groups) = shortcuts(&state, account).await;
    for name in [
        format!("whatsapp import {date}"),
        format!("WHATSAPP IMPORT {date} 2"),
    ] {
        let hand_made = groups
            .iter()
            .find(|(group, _, _)| *group == name)
            .expect("the hand-made group is still there");
        assert_eq!(
            (hand_made.1.as_str(), &hand_made.2),
            ("manual", &vec![friend]),
            "{groups:?}"
        );
    }
    let imported: Vec<_> = groups
        .iter()
        .filter(|(_, kind, _)| kind == "import")
        .collect();
    assert_eq!(
        imported,
        [&(
            format!("whatsapp import {date} 3"),
            "import".to_string(),
            touched
        )],
        "{groups:?}"
    );
}

/// The SQL form of an Import Run's Contact Group name is the name the run's
/// group is given, before and after the run finishes.
#[tokio::test]
async fn the_sql_form_of_an_import_groups_name_is_the_name_the_group_is_given() {
    let (fixture, account) = fixture_with_account().await;
    let (_, created): (String, serde_json::Value) = post_created_json(
        &fixture.state,
        "/v1/imports",
        &account.token,
        serde_json::json!({ "source": "imessage" }),
    )
    .await;
    let import_id = created["id"]
        .as_i64()
        .expect("the created Import Run has an id");
    let mut conn = fixture.state.db.acquire().await.unwrap();
    for finished_at in [None, Some("2031-02-03T04:05:06Z")] {
        sqlx::query("UPDATE imports SET finished_at = $1 WHERE id = $2")
            .bind(finished_at)
            .bind(import_id)
            .execute(&mut *conn)
            .await
            .unwrap();
        let row = crate::db::imports::get_owned_import(&mut conn, account.account_id, import_id)
            .await
            .unwrap();
        let from_sql: String = sqlx::query_scalar(&format!(
            "SELECT {IMPORT_CONTACT_GROUP_NAME_SQL} FROM imports i WHERE i.id = $1"
        ))
        .bind(import_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(from_sql, import_contact_group_name(&row), "{finished_at:?}");
    }
}
