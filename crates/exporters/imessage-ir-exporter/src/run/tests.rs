use super::*;
use message_crate_core::testutil::names_in;
use message_crate_core::{AppleConfig, MediaConfig, OutputFormat};
use std::{fs, path::Path};

fn apple_cfg(input: &Path, apple: AppleConfig) -> ExporterConfig {
    ExporterConfig {
        inputs: vec![input.to_path_buf()],
        output: input.with_extension("export_out"),
        cache_dir: input.with_extension("cache"),
        timezone: None,
        obfuscate: Default::default(),
        media: MediaConfig::default(),
        cancel: None,
        log: None,
        progress: None,
        output_format: OutputFormat::Jsonl,
        resume: false,
        source: SourceConfig::Apple(apple),
    }
}

/// A backup password only unlocks an iPhone backup, so a Mac `chat.db`
/// with one is refused rather than exported with the password ignored.
#[test]
fn a_backup_password_is_refused_for_a_mac_chat_db() {
    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let err = options_from_export_config(&apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            backup_password: Some("secret".into()),
            ..AppleConfig::default()
        },
    ))
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("it can only be used with iOS backups"),
        "{err}"
    );
}

/// An input left empty means this Mac's own Messages database.
#[test]
fn an_empty_input_is_the_macs_own_database() {
    let chat = Path::new("/backups/chat.db");
    assert_eq!(db_path_for(Some(chat)), chat);
    assert_eq!(db_path_for(Some(Path::new(""))), default_macos_db_path());
    assert_eq!(db_path_for(None), default_macos_db_path());
    assert!(default_macos_db_path().ends_with("Library/Messages/chat.db"));
}

#[test]
fn missing_chat_db_uses_locked_copy() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("chat.db");
    let err = options_from_export_config(&apple_cfg(
        &missing,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    ))
    .unwrap_err();
    assert_eq!(err.to_string(), MESSAGES_DATABASE_MISSING);
}

#[test]
fn missing_attachment_folder_uses_locked_copy() {
    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let err = options_from_export_config(&apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            attachment_root: Some(dir.path().join("no-such-root").display().to_string()),
            ..AppleConfig::default()
        },
    ))
    .unwrap_err();
    assert_eq!(err.to_string(), ATTACHMENT_FOLDER_MISSING);
}

#[test]
fn missing_apple_contacts_uses_locked_copy() {
    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let err = options_from_export_config(&apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            apple_contacts: Some(dir.path().join("no-such.abcddb")),
            ..AppleConfig::default()
        },
    ))
    .unwrap_err();
    assert_eq!(err.to_string(), APPLE_CONTACTS_MISSING);
}

#[test]
fn empty_folder_is_not_an_iphone_backup() {
    let dir = tempfile::tempdir().unwrap();
    let err = options_from_export_config(&apple_cfg(
        dir.path(),
        AppleConfig {
            platform: Some(ApplePlatform::Ios),
            ..AppleConfig::default()
        },
    ))
    .unwrap_err();
    assert_eq!(err.to_string(), NOT_AN_IPHONE_BACKUP);
}

#[test]
fn unencrypted_backup_missing_messages_uses_locked_copy() {
    let dir = tempfile::tempdir().unwrap();
    // Manifest.plist present, IsEncrypted false, hashed sms.db missing.
    fs::write(
        dir.path().join("Manifest.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>IsEncrypted</key><false/></dict></plist>"#,
    )
    .unwrap();
    let err = options_from_export_config(&apple_cfg(
        dir.path(),
        AppleConfig {
            platform: Some(ApplePlatform::Ios),
            ..AppleConfig::default()
        },
    ))
    .unwrap_err();
    assert_eq!(err.to_string(), NOT_AN_IPHONE_BACKUP);
}

/// An iPhone backup that is not encrypted, with its manifest and the
/// Messages database at its hashed path, is accepted as one.
#[test]
fn an_unencrypted_backup_with_its_messages_database_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Manifest.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>IsEncrypted</key><false/></dict></plist>"#,
    )
    .unwrap();
    let hashed = dir.path().join(MESSAGES_DB_IN_IOS_BACKUP);
    fs::create_dir_all(hashed.parent().unwrap()).unwrap();
    fs::write(&hashed, b"sqlite").unwrap();
    let options = options_from_export_config(&apple_cfg(
        dir.path(),
        AppleConfig {
            platform: Some(ApplePlatform::Ios),
            ..AppleConfig::default()
        },
    ))
    .unwrap();
    assert_eq!(options.source.platform, Platform::Ios);
    assert_eq!(options.source.db_path, dir.path());
}

/// The writer cleans its output folder before it writes, so an output
/// that is or holds the backup or `chat.db` would delete what is being
/// read.
#[test]
fn an_output_that_is_or_contains_the_input_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let backup = tmp.path().join("backup");
    fs::create_dir_all(&backup).unwrap();
    fs::write(
        backup.join("Manifest.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>IsEncrypted</key><false/></dict></plist>"#,
    )
    .unwrap();
    let hashed = backup.join(MESSAGES_DB_IN_IOS_BACKUP);
    fs::create_dir_all(hashed.parent().unwrap()).unwrap();
    fs::write(&hashed, b"sqlite").unwrap();
    let mac = tmp.path().join("mac");
    fs::create_dir_all(&mac).unwrap();
    let chat = mac.join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();

    for (input, platform, output) in [
        (&backup, ApplePlatform::Ios, backup.clone()),
        (&backup, ApplePlatform::Ios, tmp.path().to_path_buf()),
        (&chat, ApplePlatform::MacOs, mac.clone()),
    ] {
        let mut config = apple_cfg(
            input,
            AppleConfig {
                platform: Some(platform),
                ..AppleConfig::default()
            },
        );
        config.output = output;
        let Err(err) = options_from_export_config(&config) else {
            panic!("{} was accepted", config.output.display());
        };
        assert!(
            err.to_string()
                .contains("must not be the same as, or contain, the input"),
            "{}: {err}",
            config.output.display()
        );
    }
}

#[test]
fn auto_detects_a_backup_folder_by_its_hashed_database() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(detect_platform(dir.path()).unwrap(), Platform::MacOs);

    let hashed = dir.path().join(MESSAGES_DB_IN_IOS_BACKUP);
    fs::create_dir_all(hashed.parent().unwrap()).unwrap();
    fs::write(&hashed, b"sqlite").unwrap();
    assert_eq!(detect_platform(dir.path()).unwrap(), Platform::Ios);

    let err = detect_platform(&hashed).unwrap_err();
    assert!(
        err.to_string().contains("choose the backup folder"),
        "{err}"
    );
}

#[test]
fn options_carry_the_request_the_program_receives() {
    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let options = options_from_export_config(&apple_cfg(
        &chat,
        AppleConfig {
            platform: None,
            copy_method: "disabled".into(),
            use_caller_id: false,
            ..AppleConfig::default()
        },
    ))
    .unwrap();
    assert_eq!(options.source.platform, Platform::MacOs);
    assert_eq!(options.source.db_path, chat);
    assert!(!options.use_caller_id);
    let Request::Export(request) = options.export_request(Path::new("/scratch")) else {
        panic!("an export run sends an export request");
    };
    assert_eq!(request.scratch_dir, Path::new("/scratch"));
    assert_eq!(options.attachment_embed, AttachmentEmbed::Disabled);
    assert!(options.export_path.ends_with("chat.export_out"));
}

#[cfg(unix)]
#[test]
fn rows_the_program_skipped_are_counted_in_the_run_result() {
    use imessage_reader_protocol::PROTOCOL_VERSION;
    use ios_backup::testutil::{fake_helper, source_line, spawn_fake};

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let body = format!(
        "{}\necho '{{\"event\":\"export_done\",\"messages_seen\":5,\"failures\":2}}'",
        source_line(PROTOCOL_VERSION)
    );
    let program = fake_helper(dir.path(), &body);
    let config = apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    );

    let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
    assert!(
        result
            .messages
            .iter()
            .any(|line| line == &format!("  {}: 2", convert::SKIPPED_UNREADABLE_MESSAGE)),
        "{:#?}",
        result.messages
    );
}

#[cfg(unix)]
#[test]
fn a_run_with_no_skipped_rows_says_nothing_about_them() {
    use imessage_reader_protocol::PROTOCOL_VERSION;
    use ios_backup::testutil::{fake_helper, source_line, spawn_fake};

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let body = format!(
        "{}\necho '{{\"event\":\"export_done\",\"messages_seen\":5,\"failures\":0}}'",
        source_line(PROTOCOL_VERSION)
    );
    let program = fake_helper(dir.path(), &body);
    let config = apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    );

    let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
    assert!(
        !result
            .messages
            .iter()
            .any(|line| line.contains(convert::SKIPPED_UNREADABLE_MESSAGE)),
        "{:#?}",
        result.messages
    );
}

/// A handwriting message's SVG comes from the program as text, not a
/// file. A JSON export writes it under `attachments/`, and an attachment
/// with no file says it is missing.
#[cfg(unix)]
#[test]
fn inline_and_missing_attachments_reach_a_file_backed_export() {
    use imessage_reader_protocol::{
        Attachment, AttachmentSource, Conversation, Event, Message, PROTOCOL_VERSION,
    };
    use ios_backup::testutil::{fake_helper, source_line, spawn_fake};
    use message_ir_format::read_conversation_json;

    const SVG: &str = "<svg></svg>";
    let attachment = |source| Attachment {
        original_name: None,
        mime_type: Some("image/svg+xml".into()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        source,
    };
    let events = [
        Event::Conversation(Conversation {
            chat_identifier: "+15555550122".into(),
            conversation_type: "individual".into(),
            group_title: None,
            participants: Vec::new(),
        }),
        Event::Message(Box::new(Message {
            chat_identifier: "+15555550122".into(),
            guid: "g1".into(),
            timestamp_unix_ms: 1_609_459_200_000,
            outgoing: false,
            service: "iMessage".into(),
            message_kind: "imessage".into(),
            sender_handle: Some("+15555550122".into()),
            sender_display_name: None,
            subject: None,
            text: String::new(),
            owner_handle: "+15555550100".into(),
            owner_display_name: None,
            imessage: None,
            attachments: vec![
                attachment(AttachmentSource::Inline { text: SVG.into() }),
                attachment(AttachmentSource::Missing),
            ],
        })),
        Event::ExportDone {
            messages_seen: 1,
            failures: 0,
        },
    ];
    let mut body = source_line(PROTOCOL_VERSION);
    for event in &events {
        body.push_str(&format!(
            "\necho '{}'",
            serde_json::to_string(event).unwrap()
        ));
    }

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let program = fake_helper(dir.path(), &body);
    let config = ExporterConfig {
        output_format: OutputFormat::Json,
        ..apple_cfg(
            &chat,
            AppleConfig {
                platform: Some(ApplePlatform::MacOs),
                ..AppleConfig::default()
            },
        )
    };
    let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
    assert!(
        result.messages.iter().any(|l| l == "  saved 1 attachments"),
        "{:#?}",
        result.messages
    );

    let doc = read_conversation_json(&config.output.join("+15555550122.json")).unwrap();
    let attachments = &doc.messages[0].attachments;
    let path = attachments[0].path.as_deref().expect("the SVG was staged");
    assert_eq!(fs::read_to_string(config.output.join(path)).unwrap(), SVG);
    assert_eq!(attachments[1].path, None);
    assert_eq!(
        attachments[1].missing_reason.as_deref(),
        Some("file_missing")
    );
}

/// The desktop app's Staging counts are the run result's counts, so a
/// JSON Lines export reports the messages it wrote, not zero.
#[cfg(unix)]
#[test]
fn a_jsonl_run_reports_its_conversations_and_messages() {
    use imessage_reader_protocol::{Conversation, Event, Message, PROTOCOL_VERSION};
    use ios_backup::testutil::{fake_helper, source_line, spawn_fake};

    let message = |guid: &str, timestamp_unix_ms| {
        Event::Message(Box::new(Message {
            chat_identifier: "+15555550122".into(),
            guid: guid.into(),
            timestamp_unix_ms,
            outgoing: false,
            service: "iMessage".into(),
            message_kind: "imessage".into(),
            sender_handle: Some("+15555550122".into()),
            sender_display_name: None,
            subject: None,
            text: "hello".into(),
            owner_handle: "+15555550100".into(),
            owner_display_name: None,
            imessage: None,
            attachments: Vec::new(),
        }))
    };
    let events = [
        Event::Conversation(Conversation {
            chat_identifier: "+15555550122".into(),
            conversation_type: "individual".into(),
            group_title: None,
            participants: Vec::new(),
        }),
        message("g1", 1_609_459_200_000),
        message("g2", 1_609_459_260_000),
        Event::ExportDone {
            messages_seen: 2,
            failures: 0,
        },
    ];
    let mut body = source_line(PROTOCOL_VERSION);
    for event in &events {
        body.push_str(&format!(
            "\necho '{}'",
            serde_json::to_string(event).unwrap()
        ));
    }

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let program = fake_helper(dir.path(), &body);
    let config = apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    );

    let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
    assert_eq!((result.conversations, result.message_count), (1, 2));
}

/// The program's script for an encrypted export of one message with a
/// video and a photo. It keeps the request in `<dir>/request.json`,
/// then answers the attachment request for the video with `video` and
/// the one for the photo with `photo`.
#[cfg(unix)]
fn encrypted_export_script(dir: &Path, video: &str, photo: &str) -> String {
    use imessage_reader_protocol::{
        Attachment, AttachmentSource, Conversation, Event, Message, PROTOCOL_VERSION,
    };

    let attachment = |name: &str, mime: &str| Attachment {
        original_name: Some(name.into()),
        mime_type: Some(mime.into()),
        is_sticker: false,
        transcription: None,
        sticker_effect: None,
        source: AttachmentSource::Path {
            path: format!("/backup/{name}").into(),
            size_hint: Some(10),
        },
    };
    let events = [
        Event::Conversation(Conversation {
            chat_identifier: "+15555550122".into(),
            conversation_type: "individual".into(),
            group_title: None,
            participants: Vec::new(),
        }),
        Event::Message(Box::new(Message {
            chat_identifier: "+15555550122".into(),
            guid: "g1".into(),
            timestamp_unix_ms: 1_609_459_200_000,
            outgoing: false,
            service: "iMessage".into(),
            message_kind: "imessage".into(),
            sender_handle: Some("+15555550122".into()),
            sender_display_name: None,
            subject: None,
            text: String::new(),
            owner_handle: "+15555550100".into(),
            owner_display_name: None,
            imessage: None,
            attachments: vec![
                attachment("IMG_0001.MOV", "video/quicktime"),
                attachment("IMG_0002.JPG", "image/jpeg"),
            ],
        })),
        Event::ExportDone {
            messages_seen: 1,
            failures: 0,
        },
    ];
    let mut body = format!(
        r#"printf '%s' "$request" > '{}/request.json'
scratch=$(printf '%s' "$request" | sed 's/.*"scratch_dir":"\([^"]*\)".*/\1/')
echo '{{"event":"source","protocol_version":{PROTOCOL_VERSION},"encrypted":true}}'"#,
        dir.display()
    );
    for event in &events {
        body.push_str(&format!(
            "\necho '{}'",
            serde_json::to_string(event).unwrap()
        ));
    }
    body.push_str(&format!(
        r#"
while read -r line; do
  case "$line" in
    *IMG_0001*) {video} ;;
    *) {photo} ;;
  esac
done"#
    ));
    body
}

/// An attachment the program could not decrypt is counted on its own,
/// with its reason, and not passed off as one the backup does not hold
/// (#1134). The photo the backup does not hold is not counted with it.
#[cfg(unix)]
#[test]
fn an_attachment_that_fails_to_decrypt_is_counted_apart_from_missing_ones() {
    use ios_backup::testutil::{fake_helper, spawn_fake};

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let body = encrypted_export_script(
        dir.path(),
        r#"echo '{"event":"attachment","outcome":"failed","reason":"write the decrypted file: No space left on device"}'"#,
        r#"echo '{"event":"attachment","outcome":"missing"}'"#,
    );
    let program = fake_helper(dir.path(), &body);
    let config = apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    );

    let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
    assert!(
        result
            .messages
            .iter()
            .any(|line| line == &format!("  {}: 1", convert::ATTACHMENT_NOT_DECRYPTED)),
        "{:#?}",
        result.messages
    );
    assert!(
        result
            .messages
            .iter()
            .any(|line| line.contains("IMG_0001.MOV") && line.contains("No space left on device")),
        "{:#?}",
        result.messages
    );
    assert!(
        !result
            .messages
            .iter()
            .any(|line| line.contains("IMG_0002.JPG")),
        "{:#?}",
        result.messages
    );
    // The Import Run lists the video, and only the video, as an issue.
    assert_eq!(
        result.issues,
        [message_crate_core::RunIssue {
            kind: "error".into(),
            step: "attachments".into(),
            item: "/backup/IMG_0001.MOV".into(),
            reason: "could not be decrypted: write the decrypted file: No space left on device"
                .into(),
        }]
    );
}

/// A reader that stops partway through the attachments stops the run
/// with an error that says so, through the write queue (JSON Lines) and
/// through the staging step (CSV) alike. Before, the run recorded the
/// photo the reader died on, and every attachment after it,
/// `file_missing`, and ended looking like a finished run (#1442).
#[cfg(unix)]
#[test]
fn a_reader_that_stops_during_the_attachments_stops_the_run() {
    use ios_backup::testutil::{fake_helper, spawn_fake};

    for format in [OutputFormat::Jsonl, OutputFormat::Csv] {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let body = encrypted_export_script(
            dir.path(),
            r#"echo video > "$scratch/video.mov"; echo "{\"event\":\"attachment\",\"outcome\":\"ready\",\"path\":\"$scratch/video.mov\"}""#,
            "exit 3",
        );
        let program = fake_helper(dir.path(), &body);
        let config = ExporterConfig {
            output_format: format,
            ..apple_cfg(
                &chat,
                AppleConfig {
                    platform: Some(ApplePlatform::MacOs),
                    ..AppleConfig::default()
                },
            )
        };

        let err = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request)))
            .expect_err("a run whose reader stopped fails");
        let text = format!("{err:#}");
        assert!(
            text.contains(
                "attachment /backup/IMG_0002.JPG: \
                 imessage-reader stopped before finishing (exit status: 3)"
            ),
            "{format:?}: {text}"
        );
        let written: Vec<_> = fs::read_dir(&config.output)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == format.as_str()))
            .collect();
        assert!(
            written.is_empty(),
            "{format:?}: no conversation file records the photo missing: {written:?}"
        );
    }
}

/// The program decrypts into a folder under the app's cache folder,
/// beside the identities request's, never into the output folder, and the
/// folder is gone once the run ends (#1402).
#[cfg(unix)]
#[test]
fn the_scratch_folder_is_under_the_cache_folder_and_deleted_after() {
    use ios_backup::testutil::{fake_helper, spawn_fake};

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let body = encrypted_export_script(
        dir.path(),
        r#"echo video > "$scratch/video.mov"; echo "{\"event\":\"attachment\",\"outcome\":\"ready\",\"path\":\"$scratch/video.mov\"}""#,
        r#"echo '{"event":"attachment","outcome":"missing"}'"#,
    );
    let program = fake_helper(dir.path(), &body);
    let config = apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    );

    let result = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request))).unwrap();
    assert!(
        result.messages.iter().any(|l| l == "  saved 1 attachments"),
        "{:#?}",
        result.messages
    );

    let sent = fs::read_to_string(dir.path().join("request.json")).unwrap();
    let Ok(Request::Export(sent)) = serde_json::from_str::<Request>(&sent) else {
        panic!("the program was sent an export request: {sent}");
    };
    let reader_root = config
        .cache_dir
        .join(message_crate_core::IMESSAGE_READER_FOLDER);
    assert_eq!(
        sent.scratch_dir.parent(),
        Some(reader_root.as_path()),
        "{} is not in {}",
        sent.scratch_dir.display(),
        reader_root.display()
    );
    assert!(!sent.scratch_dir.exists());
    assert!(names_in(&reader_root).is_empty());
    let mut left: Vec<String> = fs::read_dir(&config.output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        ["+15555550122.jsonl", ".message-crate-export", "attachments"]
    );
}

/// A run that fails removes its scratch folder, as one that completes
/// does, and never leaves one in the output folder (#1402).
#[cfg(unix)]
#[test]
fn a_failed_run_leaves_no_scratch_folder() {
    use ios_backup::testutil::{fake_helper, spawn_fake};

    let dir = tempfile::tempdir().unwrap();
    let chat = dir.path().join("chat.db");
    fs::write(&chat, b"sqlite").unwrap();
    let body = encrypted_export_script(
        dir.path(),
        r#"echo video > "$scratch/video.mov"; echo "{\"event\":\"attachment\",\"outcome\":\"ready\",\"path\":\"$scratch/video.mov\"}""#,
        "exit 3",
    );
    let program = fake_helper(dir.path(), &body);
    let config = apple_cfg(
        &chat,
        AppleConfig {
            platform: Some(ApplePlatform::MacOs),
            ..AppleConfig::default()
        },
    );

    run_with(&config, |request, _, _| Ok(spawn_fake(&program, request)))
        .expect_err("a run whose reader stopped fails");

    assert!(
        !config.output.join(".imessage-reader").exists(),
        "{:?}",
        names_in(&config.output)
    );
    let reader_root = config
        .cache_dir
        .join(message_crate_core::IMESSAGE_READER_FOLDER);
    assert!(names_in(&reader_root).is_empty());
}

/// The databases the reader decrypts out of an encrypted backup are
/// counted against the disk that holds the cache folder before the reader
/// starts, so a short disk fails the run with the space it needs rather
/// than part-way through a decrypt (#1402).
#[cfg(unix)]
#[test]
fn a_decrypt_the_cache_disk_cannot_hold_is_refused_before_the_reader_starts() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("backup");
    fs::create_dir_all(&backup).unwrap();
    fs::write(
        backup.join("Manifest.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>IsEncrypted</key><true/></dict></plist>"#,
    )
    .unwrap();
    let hashed = backup.join(MESSAGES_DB_IN_IOS_BACKUP);
    fs::create_dir_all(hashed.parent().unwrap()).unwrap();
    // A sparse file: it says 8 TiB and takes no room.
    fs::File::create(&hashed).unwrap().set_len(8 << 40).unwrap();
    let config = apple_cfg(
        &backup,
        AppleConfig {
            platform: Some(ApplePlatform::Ios),
            backup_password: Some("secret".into()),
            ..AppleConfig::default()
        },
    );

    let err = run_with(&config, |_, _, _| panic!("the reader is not started"))
        .expect_err("the decrypt does not fit");

    assert!(
        err.to_string().starts_with(
            "Not enough space on the disk that holds the app's cache folder: \
             reading this backup needs about "
        ),
        "{err:#}"
    );
    let reader_root = config
        .cache_dir
        .join(message_crate_core::IMESSAGE_READER_FOLDER);
    assert!(names_in(&reader_root).is_empty());
}

/// Every format the Apple Messages export writes checks the staging disk
/// for room before it writes an attachment, as the JSON Lines queue does
/// (#1421).
#[cfg(unix)]
#[test]
fn every_format_refuses_attachments_the_staging_disk_cannot_hold() {
    use ios_backup::testutil::{fake_helper, spawn_fake};

    for format in [
        OutputFormat::Csv,
        OutputFormat::Json,
        OutputFormat::Eml,
        OutputFormat::Mbox,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat.db");
        fs::write(&chat, b"sqlite").unwrap();
        let body = encrypted_export_script(
            dir.path(),
            r#"echo '{"event":"attachment","outcome":"missing"}'"#,
            r#"echo '{"event":"attachment","outcome":"missing"}'"#,
        )
        .replace(r#""size_hint":10"#, r#""size_hint":4611686018427387903"#);
        let program = fake_helper(dir.path(), &body);
        let config = ExporterConfig {
            output_format: format,
            ..apple_cfg(
                &chat,
                AppleConfig {
                    platform: Some(ApplePlatform::MacOs),
                    ..AppleConfig::default()
                },
            )
        };

        let err = run_with(&config, |request, _, _| Ok(spawn_fake(&program, request)))
            .expect_err("the attachments do not fit");

        assert!(
            format!("{err:#}")
                .contains("Not enough space on the staging disk: this backup needs about "),
            "{format:?}: {err:#}"
        );
        assert_eq!(
            names_in(&config.output),
            [".message-crate-export", "attachments"],
            "{format:?}"
        );
        assert!(
            names_in(&config.output.join("attachments")).is_empty(),
            "{format:?}"
        );
    }
}
