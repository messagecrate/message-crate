//! Attachments: copying and converting their Assets, counting the Assets
//! an import stores, and the files an import refuses or cannot find.

use super::*;

/// One file attached to two messages is stored once, so the storage figure
/// counts its size once.
#[tokio::test]
async fn a_file_two_messages_name_counts_once_in_storage() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let bytes = b"one clip forwarded twice";
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/clip.mov"), bytes).unwrap();
    let message = |guid: &str| {
        message_line(guid, "look")
            .sender("+15555550123")
            .attachment(attachment(
                "attachments/clip.mov",
                "clip.mov",
                "video/quicktime",
            ))
    };
    let path = write_jsonl(
        tmp.path(),
        "forwarded.jsonl",
        &format!(
            "{}\n{}\n{}\n",
            conversation_header("imessage", "+15555550123").participant("+15555550123", None),
            message("g-forward-1"),
            message("g-forward-2"),
        ),
    );
    let options = ImportOptions::fixed(FixedImportArgs {
        assets_dir: &assets,
        asset_root: tmp.path(),
        mode: ImportMode::Append,
        source: "imessage",
        account_id: TEST_ACCOUNT,
        import_id: None,
        phone_country: None,
    });
    import_jsonl_files(&db, std::slice::from_ref(&path), &options)
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE sha256 = $1")
        .bind(assets_api::sha256_hex(bytes))
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(rows, 2, "both messages name the file");
    let size = i64::try_from(bytes.len()).unwrap();
    for scope in [
        crate::db::storage::Scope::Account(TEST_ACCOUNT),
        crate::db::storage::Scope::AllAccounts,
    ] {
        assert_eq!(
            crate::db::storage::attachment_bytes(&mut conn, scope)
                .await
                .unwrap(),
            size,
            "{scope:?}"
        );
    }
}

#[tokio::test]
async fn media_none_skips_attachment_copy() {
    use crate::config::PathsConfig;
    use media::MediaMode;

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let data_dir = tmp.path().join("data");
    let paths = PathsConfig {
        db: db.clone(),
        data_dir,
        assets_dir: "assets".into(),
        assets_converted_dir: "assets_converted".into(),
    };
    let assets_dir = paths.assets_dir_for_account(TEST_ACCOUNT);
    fs::create_dir_all(tmp.path().join("media")).unwrap();
    fs::write(tmp.path().join("media/photo.jpg"), b"jpeg-bytes").unwrap();

    let path = write_jsonl(
        tmp.path(),
        "c.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms", "+15555550100").participant("+15555550100", None),
            message_line("g1", "hi")
                .sms()
                .sender("+15555550100")
                .attachment(attachment("media/photo.jpg", "photo.jpg", "image/jpeg"))
                .line()
        ),
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions {
            assets_dir: &assets_dir,
            asset_root: tmp.path(),
            mode: ImportMode::Replace,
            source: "",
            account_id: TEST_ACCOUNT,
            import_id: None,
            source_from_jsonl: true,
            media: MediaMode::Disabled,
            wipe_sources: Some(vec!["sms".into()]),
            phone_country: None,
            progress: Progress::Log,
        },
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.attachments, 0);
    assert_eq!(stats.assets_copied, 0);
}

/// `import --media convert` stores the converted file in place of the
/// original: a PNG goes in and the attachment the database holds is a JPEG.
/// Runs the real ffmpeg, so the guard is taken outside the async block, as
/// Clippy's `await_holding_lock` asks.
#[test]
fn media_convert_stores_the_converted_file_not_the_original() {
    use crate::config::PathsConfig;
    use media::MediaMode;
    use media::testutil::PNG_1X1_RGB;

    let Some(_tools) = media::testutil::real_ffmpeg_test_guard() else {
        return;
    };
    let test = async {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("messagecrate.db");
        let paths = PathsConfig {
            db: db.clone(),
            data_dir: tmp.path().join("data"),
            assets_dir: "assets".into(),
            assets_converted_dir: "assets_converted".into(),
        };
        let assets_dir = paths.assets_dir_for_account(TEST_ACCOUNT);
        fs::create_dir_all(tmp.path().join("attachments")).unwrap();
        fs::write(tmp.path().join("attachments/photo.png"), PNG_1X1_RGB).unwrap();
        let path = write_jsonl(
            tmp.path(),
            "convert.jsonl",
            &conversation_with_attachments(&["attachments/photo.png"])
                .replace("application/octet-stream", "image/png"),
        );

        import_jsonl_files(
            &db,
            &[path],
            &ImportOptions {
                assets_dir: &assets_dir,
                asset_root: tmp.path(),
                mode: ImportMode::Replace,
                source: "",
                account_id: TEST_ACCOUNT,
                import_id: None,
                source_from_jsonl: true,
                media: MediaMode::Convert,
                wipe_sources: Some(vec!["imessage".into()]),
                phone_country: None,
                progress: Progress::Log,
            },
        )
        .await
        .unwrap();

        let (_pool, mut conn) = open_verify(&db).await;
        let (mime_type, assets_path): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT mime_type, assets_path FROM attachments")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(mime_type.as_deref(), Some("image/jpeg"));
        let stored = paths
            .assets_dir_for_account(TEST_ACCOUNT)
            .join(assets_path.expect("the attachment is stored"));
        let bytes = fs::read(&stored).unwrap();
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG starts with SOI");
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}

#[tokio::test]
async fn persists_missing_reason_with_null_sha256() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(&assets).unwrap();

    let path = write_jsonl(
        tmp.path(),
        "missing-att.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+15555550123")
                .participant("+15555550123", None),
            message_line("g-missing", "see attached")
                .mms()
                .sender("+15555550123")
                .attachment(IrAttachment {
                    missing_reason: Some("too_large".into()),
                    size_bytes: Some(999),
                    ..attachment(
                        "attachments/gone.bin",
                        "gone.bin",
                        "application/octet-stream"
                    )
                })
                .line()
        ),
    );
    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            import_id: None,
            phone_country: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(stats.attachments, 1);

    let (_pool, mut conn) = open_verify(&db).await;
    let (sha256, missing_reason, size_bytes, original_name): (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT sha256, missing_reason, size_bytes, original_name FROM attachments LIMIT 1",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert!(sha256.is_none());
    assert_eq!(missing_reason.as_deref(), Some("too_large"));
    assert_eq!(size_bytes, Some(999));
    assert_eq!(original_name.as_deref(), Some("gone.bin"));
}

/// One incoming message from `+15555550123` in its own conversation file,
/// carrying an attachment for each path in `paths`. The records give no
/// sha256 and no size, as an iMessage or WhatsApp export does.
fn conversation_with_attachments(paths: &[&str]) -> String {
    let message = paths.iter().fold(
        message_line("g-att", "see attached").sender("+15555550123"),
        |message, path| {
            message.attachment(IrAttachment {
                original_name: None,
                ..attachment(path, "", "application/octet-stream")
            })
        },
    );
    format!(
        "{}\n{message}\n",
        conversation_header("imessage", "+15555550123").participant("+15555550123", None),
    )
}

#[tokio::test]
async fn an_attachment_with_no_size_in_the_export_is_stored_with_the_size_of_its_file() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/photo.bin"), b"twelve bytes").unwrap();
    let path = write_jsonl(
        tmp.path(),
        "sized.jsonl",
        &conversation_with_attachments(&["attachments/photo.bin"]),
    );

    import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();

    let (_pool, mut conn) = open_verify(&db).await;
    let size_bytes: Option<i64> = sqlx::query_scalar("SELECT size_bytes FROM attachments")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(size_bytes, Some(12));
}

/// Three attachments on one message: a file, the same bytes under another
/// name, and a file that is not there. The result counts one of each.
#[tokio::test]
async fn an_import_counts_the_files_it_copied_already_held_and_could_not_find() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    fs::create_dir_all(tmp.path().join("attachments")).unwrap();
    fs::write(tmp.path().join("attachments/photo.bin"), b"same bytes").unwrap();
    fs::write(tmp.path().join("attachments/copy.bin"), b"same bytes").unwrap();
    let path = write_jsonl(
        tmp.path(),
        "counted.jsonl",
        &conversation_with_attachments(&[
            "attachments/photo.bin",
            "attachments/copy.bin",
            "attachments/gone.bin",
        ]),
    );

    let stats = import_jsonl_files(&db, &[path], &replace_opts(&assets, tmp.path(), "imessage"))
        .await
        .unwrap();

    assert_eq!(
        (
            stats.assets_copied,
            stats.assets_deduped,
            stats.assets_missing
        ),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn claimed_import_rejects_corrupt_existing_asset() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let sha = assets_api::Sha256::of_bytes(b"expected-asset");
    let corrupt = assets.join(assets_api::shard_rel_path(&sha, ""));
    fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    fs::write(&corrupt, b"corrupt-asset").unwrap();

    let message = message_line("g-corrupt-asset", "missing asset")
        .sender("+15555550123")
        .attachment(IrAttachment {
            digest_sha256: Some(sha.to_string()),
            ..attachment(
                "attachments/missing.bin",
                "missing.bin",
                "application/octet-stream",
            )
        });
    let jsonl = format!(
        "{}\n{}\n",
        conversation_header("imessage", "+15555550123").participant("+15555550123", None),
        message
    );
    let path = write_jsonl(tmp.path(), "corrupt-existing.jsonl", &jsonl);

    let stats = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: tmp.path(),
            mode: ImportMode::Append,
            source: "imessage",
            account_id: TEST_ACCOUNT,
            import_id: None,
            phone_country: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(stats.assets_deduped, 0);
    assert_eq!(stats.assets_missing, 1);
    let (_pool, mut conn) = open_verify(&db).await;
    let assets_path: Option<String> =
        sqlx::query_scalar("SELECT assets_path FROM attachments LIMIT 1")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert!(assets_path.is_none());
}

#[tokio::test]
async fn rejects_attachment_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("messagecrate.db");
    let assets = tmp.path().join("assets");
    let export_dir = tmp.path().join("export");
    fs::create_dir_all(&assets).unwrap();
    fs::create_dir_all(&export_dir).unwrap();
    fs::write(tmp.path().join("secret.txt"), b"secret-bytes").unwrap();

    let path = write_jsonl(
        &export_dir,
        "traverse.jsonl",
        &format!(
            "{}\n{}",
            conversation_header("sms-backup-restore", "+15555550123")
                .participant("+15555550123", None),
            // The path is the input under test, so the line stays written out.
            r#"{"guid":"g-trav","timestamp_unix_ms":1426183462000,"time_precision":"milliseconds","direction":"incoming","service":"sms","message_kind":"mms","sender_identity":"+15555550123","sender_display_name":null,"subject":null,"text":"x","attachments":[{"path":"../secret.txt","original_name":"secret.txt","mime_type":"text/plain","digest_sha256":null,"is_sticker":false,"transcription":null,"sticker_effect":null,"size_bytes":12,"missing_reason":null}],"imessage":null,"source":null}
"#
        ),
    );
    let err = import_jsonl_files(
        &db,
        &[path],
        &ImportOptions::fixed(FixedImportArgs {
            assets_dir: &assets,
            asset_root: &export_dir,
            mode: ImportMode::Append,
            source: "sms-backup-restore",
            account_id: TEST_ACCOUNT,
            import_id: None,
            phone_country: None,
        }),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains(message_ir::UNSAFE_ATTACHMENT_PATH),
        "expected path rejection, got: {err}"
    );
}
