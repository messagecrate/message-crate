//! Conversations: message order across batches, copies of one
//! conversation merged into one, and the title and participants a
//! merged conversation keeps.

use super::*;

/// One incoming message of the one-to-one chat with +15555550119, whose text
/// is its guid.
fn same_second_message(guid: &str, ms: i64) -> MessageLine {
    message_line(guid, guid).at(ms).sender("+15555550119")
}

/// The header of the one-to-one chat with +15555550119.
fn same_second_header() -> ConversationHeaderLine {
    conversation_header("imessage", "+15555550119").participant("+15555550119", Some("Bob"))
}

/// The account's message guids in the order the conversation page and every
/// export read them.
async fn guids_in_conversation_order(state: &crate::server::AppState) -> Vec<String> {
    let mut conn = state.db.acquire().await.unwrap();
    sqlx::query_scalar("SELECT guid FROM messages ORDER BY timestamp, sort_order, id")
        .fetch_all(&mut *conn)
        .await
        .unwrap()
}

/// Post each body as one batch of a single Import Run.
async fn post_batches_of_one_run(
    state: &crate::server::AppState,
    token: &str,
    bodies: Vec<String>,
) {
    let path = batches_path(state, token, "imessage").await;
    for body in bodies {
        let (status, text) =
            crate::test_support::post_raw(state, &path, token, "application/jsonl", body).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{text}");
    }
}

/// #1168: a conversation split across two batches keeps its order where the
/// split falls inside one second.
#[tokio::test]
async fn a_conversation_split_across_batches_keeps_its_order_within_a_second() {
    let (state, _fixture, token) = importer().await;
    post_batches_of_one_run(
        &state,
        &token,
        vec![
            format!(
                "{}\n{}\n{}\n",
                same_second_header(),
                same_second_message("m1", 1_426_183_462_100),
                same_second_message("m2", 1_426_183_462_200)
            ),
            format!(
                "{}\n{}\n",
                same_second_header(),
                same_second_message("m3", 1_426_183_462_300)
            ),
        ],
    )
    .await;
    assert_eq!(
        guids_in_conversation_order(&state).await,
        ["m1", "m2", "m3"]
    );
}

/// #1168: three messages with one whole-second time, split across two
/// batches, read back in the source's order. Sources that record whole
/// seconds (iMazing, OpenExtract, GO SMS Pro) leave nothing but the source's
/// order to tell them apart.
#[tokio::test]
async fn messages_of_one_whole_second_split_across_batches_keep_the_source_order() {
    let (state, _fixture, token) = importer().await;
    post_batches_of_one_run(
        &state,
        &token,
        vec![
            format!(
                "{}\n{}\n{}\n",
                same_second_header(),
                same_second_message("m1", 1_426_183_462_000),
                same_second_message("m2", 1_426_183_462_000)
            ),
            format!(
                "{}\n{}\n",
                same_second_header(),
                same_second_message("m3", 1_426_183_462_000)
            ),
        ],
    )
    .await;
    assert_eq!(
        guids_in_conversation_order(&state).await,
        ["m1", "m2", "m3"]
    );
}

/// #1168: a later append that adds a message in a second the conversation
/// already holds reads it back after the stored ones.
#[tokio::test]
async fn an_append_in_a_second_already_held_sorts_after_the_stored_messages() {
    let (state, _fixture, token) = importer().await;
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{}\n{}\n{}\n",
            same_second_header(),
            same_second_message("m1", 1_426_183_462_000),
            same_second_message("m2", 1_426_183_462_000)
        ),
    )
    .await;
    import_one_batch(
        &state,
        &token,
        "imessage",
        "append",
        format!(
            "{}\n{}\n",
            same_second_header(),
            same_second_message("m3", 1_426_183_462_000)
        ),
    )
    .await;
    assert_eq!(
        guids_in_conversation_order(&state).await,
        ["m1", "m2", "m3"]
    );
}

/// The header of an Apple Messages one-to-one chat keyed by `chat`, whose
/// one participant is written as `chat` too.
fn one_to_one_header(chat: &str) -> String {
    conversation_header("imessage", chat)
        .participant(chat, None)
        .to_string()
}

/// #1173: two conversations whose chat ids differ as written and normalise
/// to one identity, in one batch, become one conversation holding both
/// conversations' messages, as they do when they arrive in separate batches.
/// Both messages share one second, so the second conversation's message
/// takes the next `sort_order` rather than repeating the first's.
#[tokio::test]
async fn two_conversations_on_one_identity_in_one_batch_become_one() {
    let (state, _fixture, token) = importer().await;
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        one_to_one_header("(555) 555-0119"),
        same_second_message("m1", 1_426_183_462_000),
        one_to_one_header("5555550119"),
        same_second_message("m2", 1_426_183_462_000),
    );
    post_batches_of_one_run(&state, &token, vec![body]).await;

    let mut conn = state.db.acquire().await.unwrap();
    let conversations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversations")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(conversations, 1);
    let participants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM participants")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(participants, 1, "the one person is listed once");
    let messages: Vec<(String, i64)> =
        sqlx::query_as("SELECT guid, sort_order FROM messages ORDER BY sort_order, id")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        messages,
        [("m1".to_string(), 0), ("m2".to_string(), 1)],
        "both messages are in the one conversation, in the batch's order"
    );
}

/// The header of the Apple Messages group `chat1000000408`, titled `title`
/// (no title when `None`), with one participant.
fn titled_group_header(title: Option<&str>) -> String {
    let header = conversation_header("imessage", "chat1000000408").group();
    let header = match title {
        Some(title) => header.title(title),
        None => header,
    };
    header.participant("+15555550119", None).to_string()
}

/// One copy of the group `chat1000000408`: its header titled `title`, and
/// one message `guid` sent at `ms`.
fn titled_group_copy(title: Option<&str>, guid: &str, ms: i64) -> String {
    format!(
        "{}\n{}\n",
        titled_group_header(title),
        same_second_message(guid, ms)
    )
}

/// The title the account's one conversation ends with.
async fn the_one_group_title(state: &crate::server::AppState) -> Option<String> {
    let mut conn = state.db.acquire().await.unwrap();
    let titles: Vec<Option<String>> = sqlx::query_scalar("SELECT group_title FROM conversations")
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(titles.len(), 1, "the two copies are one conversation");
    titles.into_iter().next().unwrap()
}

/// #1408: two copies of one group, "Old" whose messages end earlier and
/// "New" whose messages end later, end titled "New" whether they arrive in
/// one batch or in two, in either order.
#[tokio::test]
async fn a_merged_group_takes_the_title_of_the_copy_whose_messages_end_later() {
    let old = titled_group_copy(Some("Old"), "m-old", 1_426_183_462_000);
    let new = titled_group_copy(Some("New"), "m-new", 1_426_269_862_000);
    let arrivals = [
        ("one batch, Old first", vec![format!("{old}{new}")]),
        ("one batch, New first", vec![format!("{new}{old}")]),
        ("two batches, Old first", vec![old.clone(), new.clone()]),
        ("two batches, New first", vec![new.clone(), old.clone()]),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("New"),
            "{arrival}"
        );
    }
}

/// #1408: a copy whose messages end later and that has no title keeps the
/// title the group already has, in one batch and in two.
#[tokio::test]
async fn a_later_copy_with_no_title_keeps_the_stored_title() {
    let old = titled_group_copy(Some("Old"), "m-old", 1_426_183_462_000);
    let untitled = titled_group_copy(None, "m-new", 1_426_269_862_000);
    let arrivals = [
        ("one batch, titled first", vec![format!("{old}{untitled}")]),
        (
            "one batch, untitled first",
            vec![format!("{untitled}{old}")],
        ),
        (
            "two batches, titled first",
            vec![old.clone(), untitled.clone()],
        ),
        (
            "two batches, untitled first",
            vec![untitled.clone(), old.clone()],
        ),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("Old"),
            "{arrival}"
        );
    }
}

/// #1408: a later copy titled only with spaces has no title, so it keeps the
/// title the group already has, in one batch and in two.
#[tokio::test]
async fn a_later_copy_titled_with_spaces_keeps_the_stored_title() {
    let old = titled_group_copy(Some("Old"), "m-old", 1_426_183_462_000);
    let blank = titled_group_copy(Some("   "), "m-new", 1_426_269_862_000);
    let arrivals = [
        ("one batch", vec![format!("{old}{blank}")]),
        ("two batches", vec![old.clone(), blank.clone()]),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("Old"),
            "{arrival}"
        );
    }
}

/// #1408: three copies of one group, "Y" ending first, "Z" ending later,
/// and an untitled copy ending last, end titled "Z" in every order, in one
/// batch and in three. The untitled copy's later messages don't stop a newer
/// title from replacing an older one.
#[tokio::test]
async fn an_untitled_copy_ending_last_does_not_keep_an_older_title() {
    let y = titled_group_copy(Some("Y"), "m-y", 1_426_183_462_000);
    let z = titled_group_copy(Some("Z"), "m-z", 1_426_269_862_000);
    let untitled = titled_group_copy(None, "m-x", 1_426_356_262_000);
    let orders = [
        [&y, &z, &untitled],
        [&y, &untitled, &z],
        [&z, &y, &untitled],
        [&z, &untitled, &y],
        [&untitled, &y, &z],
        [&untitled, &z, &y],
    ];
    for (n, order) in orders.iter().enumerate() {
        let one_batch = vec![order.iter().map(|c| c.as_str()).collect::<String>()];
        let three_batches = order.iter().map(|c| (*c).clone()).collect::<Vec<_>>();
        for (arrival, bodies) in [("one batch", one_batch), ("three batches", three_batches)] {
            let (state, _fixture, token) = importer().await;
            post_batches_of_one_run(&state, &token, bodies).await;
            assert_eq!(
                the_one_group_title(&state).await.as_deref(),
                Some("Z"),
                "order {n}, {arrival}"
            );
        }
    }
}

/// #1408: two copies whose messages end at the same time keep the title
/// stored first, in one batch and in two.
#[tokio::test]
async fn copies_whose_messages_end_together_keep_the_stored_title() {
    let first = titled_group_copy(Some("First"), "m-first", 1_426_183_462_000);
    let second = titled_group_copy(Some("Second"), "m-second", 1_426_183_462_000);
    let arrivals = [
        ("one batch", vec![format!("{first}{second}")]),
        ("two batches", vec![first.clone(), second.clone()]),
    ];
    for (arrival, bodies) in arrivals {
        let (state, _fixture, token) = importer().await;
        post_batches_of_one_run(&state, &token, bodies).await;
        assert_eq!(
            the_one_group_title(&state).await.as_deref(),
            Some("First"),
            "{arrival}"
        );
    }
}

/// #1172: a group header that lists one person twice, under two spellings of
/// one number that normalise to one handle, is taken with `200 OK` and the
/// group lists that person once.
#[tokio::test]
async fn a_participant_listed_twice_under_one_identity_is_listed_once() {
    let (state, _fixture, token) = importer().await;
    let header = conversation_header("imessage", "chat1000000172")
        .group()
        .title("Trip")
        .participant("+1 (555) 555-0119", None)
        .participant("+15555550119", None);
    let body = format!(
        "{header}\n{}\n",
        same_second_message("m1", 1_426_183_462_000)
    );
    post_batches_of_one_run(&state, &token, vec![body]).await;

    let mut conn = state.db.acquire().await.unwrap();
    let participants: Vec<String> = sqlx::query_scalar(
        "SELECT h.normalized FROM participants p JOIN handles h ON h.id = p.handle_id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        participants,
        ["+15555550119"],
        "the one number is listed once"
    );
}
