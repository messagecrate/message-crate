use super::*;

#[test]
fn values_split_on_newline_comma_and_semicolon_but_not_spaces() {
    let cases: &[(&str, &[&str])] = &[
        ("", &[]),
        ("  \n , ; ", &[]),
        ("+15555550100", &["+15555550100"]),
        (
            "+15555550100\n+15555550101",
            &["+15555550100", "+15555550101"],
        ),
        (
            "+15555550100, +15555550101",
            &["+15555550100", "+15555550101"],
        ),
        (
            "+15555550100;+15555550101",
            &["+15555550100", "+15555550101"],
        ),
        (
            "+15555550100\r\n+15555550101,+15555550102 ; +15555550103",
            &[
                "+15555550100",
                "+15555550101",
                "+15555550102",
                "+15555550103",
            ],
        ),
        // A space stays inside one value: "+1 555-555-0119" is one
        // number, typed the way the Import screen's placeholder shows it.
        ("+1 555-555-0119", &["+1 555-555-0119"]),
        ("+1555 +1666", &["+1555 +1666"]),
        (
            "+1 555-555-0119, +44 20 7946 0958",
            &["+1 555-555-0119", "+44 20 7946 0958"],
        ),
        (
            "me@example.com;; you@example.com",
            &["me@example.com", "you@example.com"],
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(values(input), *expected, "values({input:?})");
    }
}

#[test]
fn imazing_passes_obfuscate() {
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        obfuscate: true,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Imazing, Path::new("/cache"))
        .unwrap();
    assert!(config.obfuscate.enabled);
    assert!(matches!(config.source, SourceConfig::Imazing(_)));
}

#[test]
fn seed_must_be_valid_hex() {
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        obfuscate_seed: "bad".into(),
        ..Form::default()
    };
    assert_eq!(
        form.to_config(Exporter::OpenExtract, Path::new("/cache"))
            .unwrap_err(),
        vec!["obfuscate seed must be exactly 64 hex characters, got 3".to_string()]
    );
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        obfuscate_seed: "01234567".into(),
        ..Form::default()
    };
    assert_eq!(
        form.to_config(Exporter::OpenExtract, Path::new("/cache"))
            .unwrap_err(),
        vec!["obfuscate seed must be exactly 64 hex characters, got 8".to_string()]
    );
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        obfuscate_seed: "g".repeat(64),
        ..Form::default()
    };
    assert_eq!(
        form.to_config(Exporter::OpenExtract, Path::new("/cache"))
            .unwrap_err(),
        vec!["obfuscate seed must contain only hex characters (0-9, a-f)".to_string()]
    );
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        obfuscate_seed: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::OpenExtract, Path::new("/cache"))
        .unwrap();
    assert!(config.obfuscate.enabled);
    assert_eq!(
        config.obfuscate.seed.as_deref(),
        Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
    );
}

#[test]
fn plus_verbose_and_owner_fields() {
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let form = Form {
        input: cwd,
        output: "out".into(),
        owner_phones: "+15555550100\n+15555550101".into(),
        owner_emails: "me@example.com".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::SmsBackupPlus, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.inputs.len(), 1);
    let SourceConfig::SmsBackupPlus(plus) = config.source else {
        panic!("expected SmsBackupPlus");
    };
    assert_eq!(plus.owner_phones.len(), 2);
    assert!(plus.verbose);
    assert!(plus.include_summary);
}

/// The form's phone country reaches SMS Backup+, which keys numbers by
/// it, and a code the phone table does not hold is refused (#1676).
#[test]
fn plus_carries_the_phone_country_and_an_unknown_one_is_refused() {
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let form = Form {
        input: cwd,
        output: "out".into(),
        owner_phones: "+447700900100".into(),
        owner_emails: "me@example.com".into(),
        phone_country: " gb ".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::SmsBackupPlus, Path::new("/cache"))
        .unwrap();
    let SourceConfig::SmsBackupPlus(plus) = config.source else {
        panic!("expected SmsBackupPlus");
    };
    assert_eq!(plus.phone_country.map(|c| c.code), Some("GB"));

    let unknown = Form {
        phone_country: "ZZ".into(),
        ..form
    };
    let errors = unknown
        .to_config(Exporter::SmsBackupPlus, Path::new("/cache"))
        .unwrap_err();
    assert_eq!(
        errors,
        ["Phone country \"ZZ\" is not a country code Message Crate knows."]
    );
}

#[test]
fn plus_rejects_multiple_inputs() {
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let form = Form {
        input: format!("{cwd}\n{cwd}"),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        owner_emails: "me@example.com".into(),
        ..Form::default()
    };
    let err = form
        .to_config(Exporter::SmsBackupPlus, Path::new("/cache"))
        .unwrap_err();
    assert!(err.iter().any(|e| e.contains("single file or directory")));
}

#[test]
fn imessage_requires_output_and_uses_caller_id() {
    let form = Form {
        output: String::new(),
        ..Form::default()
    };
    assert_eq!(
        form.to_config(Exporter::Imessage, Path::new("/cache"))
            .unwrap_err(),
        vec!["Output directory is required.".to_string()]
    );

    let form = Form {
        output: "out".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Imessage, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.output, PathBuf::from("out"));
    let SourceConfig::Apple(apple) = config.source else {
        panic!("expected Apple");
    };
    assert!(apple.use_caller_id);
    assert_eq!(apple.copy_method, "clone");
}

/// An Import Run reads one backup: SMS Backup & Restore writes each as
/// one file, GO SMS Pro as a directory.
#[test]
fn sbr_refuses_a_directory_and_go_sms_pro_takes_one() {
    let form = Form {
        input: env!("CARGO_MANIFEST_DIR").into(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        ..Form::default()
    };
    let errors = form
        .to_config(Exporter::SmsBackupRestore, Path::new("/cache"))
        .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("one SMS Backup & Restore .xml file, not a directory")),
        "{errors:?}"
    );
    assert!(
        form.to_config(Exporter::GoSmsPro, Path::new("/cache"))
            .is_ok()
    );
}

#[test]
fn sbr_passes_output_format() {
    let form = Form {
        input: concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        output_format: OutputFormat::Eml,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::SmsBackupRestore, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.output_format, OutputFormat::Eml);

    let go = form
        .to_config(Exporter::GoSmsPro, Path::new("/cache"))
        .unwrap();
    assert_eq!(go.output_format, OutputFormat::Eml);
}

#[test]
fn imessage_passes_output_format() {
    let form = Form {
        output: "out".into(),
        output_format: OutputFormat::Eml,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Imessage, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.output_format, OutputFormat::Eml);
}

#[test]
fn android_passes_media_mode() {
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        attachment_media: AttachmentMedia::Clone,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::GoSmsPro, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.media.mode, MediaMode::Clone);

    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        attachment_media: AttachmentMedia::Disabled,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::GoSmsPro, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.media.mode, MediaMode::Disabled);
}

#[test]
fn openextract_builds_its_own_source() {
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::OpenExtract, Path::new("/cache"))
        .unwrap();
    assert!(matches!(config.source, SourceConfig::OpenExtract(_)));
}

#[test]
fn imazing_passes_timezone_to_config() {
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        timezone: "UTC-05:00".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Imazing, Path::new("/cache"))
        .unwrap();
    let SourceConfig::Imazing(_) = &config.source else {
        panic!("expected Imazing");
    };
    assert_eq!(config.timezone.as_deref(), Some("UTC-05:00"));
}

#[test]
fn whatsapp_passes_platform_and_media() {
    let form = Form {
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        whatsapp_platform: WhatsappPlatform::Android,
        whatsapp_key: "abc123".into(),
        whatsapp_backup: "/tmp/backup".into(),
        whatsapp_media: "/tmp/media".into(),
        is_business_app: true,
        attachment_media: AttachmentMedia::Clone,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Whatsapp, Path::new("/cache"))
        .unwrap();
    assert!(config.inputs.is_empty());
    assert_eq!(config.media.mode, MediaMode::Clone);
    let SourceConfig::Whatsapp(wa) = config.source else {
        panic!("expected Whatsapp");
    };
    assert_eq!(wa.platform, Some(WhatsappPlatform::Android));
    assert_eq!(wa.key.as_deref(), Some("abc123"));
    assert_eq!(wa.backup, Some(PathBuf::from("/tmp/backup")));
    assert_eq!(wa.media, Some(PathBuf::from("/tmp/media")));
    assert_eq!(wa.owner_phone.as_deref(), Some("+15555550100"));
    assert!(wa.is_business_app);

    // iPhone reads the number from the backup, so the field may be empty.
    let ios = Form {
        output: "out".into(),
        whatsapp_platform: WhatsappPlatform::Ios,
        whatsapp_backup: "/tmp/ios-backup".into(),
        ..Form::default()
    };
    let ios_config = ios
        .to_config(Exporter::Whatsapp, Path::new("/cache"))
        .unwrap();
    let SourceConfig::Whatsapp(wa) = ios_config.source else {
        panic!("expected Whatsapp");
    };
    assert_eq!(wa.platform, Some(WhatsappPlatform::Ios));
    assert_eq!(wa.backup, Some(PathBuf::from("/tmp/ios-backup")));
    assert!(wa.key.is_none());
    assert!(wa.owner_phone.is_none());

    let ios_missing = Form {
        output: "out".into(),
        whatsapp_platform: WhatsappPlatform::Ios,
        ..Form::default()
    };
    let err = ios_missing
        .to_config(Exporter::Whatsapp, Path::new("/cache"))
        .unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.contains("Backup path is required for iOS"))
    );
}

/// An Android crypt backup does not carry the account holder's number,
/// so the form is the only source: an empty field is a validation
/// problem, not an import that runs and records no owner.
#[test]
fn whatsapp_android_requires_the_owner_number() {
    let form = Form {
        output: "out".into(),
        whatsapp_platform: WhatsappPlatform::Android,
        ..Form::default()
    };
    let err = form
        .to_config(Exporter::Whatsapp, Path::new("/cache"))
        .unwrap_err();
    assert!(
        err.iter()
            .any(|e| e == "Owner's WhatsApp number is required."),
        "{err:?}"
    );
}

#[test]
fn whatsapp_forwards_existing_input_as_search_root() {
    let dir = tempfile::tempdir().unwrap();
    let form = Form {
        input: dir.path().display().to_string(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Whatsapp, Path::new("/cache"))
        .unwrap();
    assert_eq!(config.inputs, vec![dir.path().to_path_buf()]);

    let missing = Form {
        input: "/does/not/exist-whatsapp-input".into(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        ..Form::default()
    };
    let err = missing
        .to_config(Exporter::Whatsapp, Path::new("/cache"))
        .unwrap_err();
    assert!(err.iter().any(|e| e.contains("does not exist")), "{err:?}");
}

#[test]
fn ensure_output_dir_creates_missing_path() {
    let out = std::env::temp_dir().join(format!(
        "message-crate-core-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    ensure_output_dir(&out).unwrap();
    assert!(out.is_dir());
    let _ = fs::remove_dir_all(&out);
}

const SEED: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn without_ffmpeg_only_unobfuscated_convert_and_compress_are_refused() {
    // Holds the tools lock, so a test that runs the real ffmpeg waits.
    let _hidden = media::testutil::hide_ffmpeg();
    assert!(!media::ffmpeg_available());

    let form = |attachment_media, obfuscate, seed: &str| Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        attachment_media,
        obfuscate,
        obfuscate_seed: seed.into(),
        ..Form::default()
    };

    for exporter in [Exporter::Imessage, Exporter::GoSmsPro] {
        for media in [AttachmentMedia::Convert, AttachmentMedia::Compress] {
            let err = form(media, false, "")
                .to_config(exporter, Path::new("/cache"))
                .unwrap_err();
            assert!(
                err.iter().any(|e| e.contains("ffmpeg")),
                "{exporter:?} {media:?}: {err:?}"
            );
            // Obfuscation replaces media with placeholders, so no ffmpeg.
            form(media, true, "")
                .to_config(exporter, Path::new("/cache"))
                .unwrap_or_else(|e| panic!("{exporter:?} {media:?} obfuscated: {e:?}"));
            form(media, false, SEED)
                .to_config(exporter, Path::new("/cache"))
                .unwrap_or_else(|e| panic!("{exporter:?} {media:?} seeded: {e:?}"));
        }
        for media in [AttachmentMedia::Clone, AttachmentMedia::Disabled] {
            form(media, false, "")
                .to_config(exporter, Path::new("/cache"))
                .unwrap_or_else(|e| panic!("{exporter:?} {media:?}: {e:?}"));
        }
    }

    let err = form(AttachmentMedia::Convert, false, "")
        .to_config(Exporter::Imessage, Path::new("/cache"))
        .unwrap_err();
    assert!(
        err.iter().any(|e| e == CONVERT_COMPRESS_FFMPEG_REQUIRED),
        "{err:?}"
    );
}

#[test]
fn imessage_with_attachments_disabled_does_not_copy_them() {
    let form = Form {
        output: "out".into(),
        attachment_media: AttachmentMedia::Disabled,
        ..Form::default()
    };
    let config = form
        .to_config(Exporter::Imessage, Path::new("/cache"))
        .unwrap();
    let SourceConfig::Apple(apple) = config.source else {
        panic!("expected Apple");
    };
    assert_eq!(apple.copy_method, "disabled");
    assert_eq!(config.media.mode, MediaMode::Disabled);
}

#[test]
fn attachment_media_parses_every_wire_name() {
    for (wire, media) in [
        ("clone", AttachmentMedia::Clone),
        ("convert", AttachmentMedia::Convert),
        ("compress", AttachmentMedia::Compress),
        ("disabled", AttachmentMedia::Disabled),
    ] {
        assert_eq!(AttachmentMedia::parse(wire), Some(media));
    }
    assert_eq!(AttachmentMedia::parse("copy-everything"), None);
}

#[test]
fn compress_options_reject_bad_values() {
    let form = |fps: &str, min_size: &str| Form {
        media_max_fps: fps.into(),
        media_min_size: min_size.into(),
        ..Form::default()
    };
    assert_eq!(
        form("abc", "20").compress_options().unwrap_err(),
        "Max FPS must be a number of frames per second above 0, such as 30, not 'abc'."
    );
    assert_eq!(
        form("", "20").compress_options().unwrap_err(),
        "Max FPS is empty. It must be a number of frames per second, such as 30."
    );
    assert_eq!(
        form("30", " ").compress_options().unwrap_err(),
        "Minimum Video File Size is empty. It must be a number of megabytes, such as 20."
    );
    assert!(form("30", "lots").compress_options().is_err());
    assert!(form("30", "20M").compress_options().is_err());
}

#[test]
fn compress_settings_reach_the_config() {
    let form = Form {
        input: std::env::current_dir().unwrap().display().to_string(),
        output: "out".into(),
        owner_phones: "+15555550100".into(),
        attachment_media: AttachmentMedia::Compress,
        // Obfuscated, so the test does not depend on ffmpeg being installed.
        obfuscate: true,
        media_max_resolution: MaxResolution::P720,
        media_max_fps: "24".into(),
        media_min_size: "5".into(),
        media_skip_efficient: false,
        ..Form::default()
    };
    let expected = media::CompressOptions {
        max_resolution: MaxResolution::P720,
        max_fps: 24.0,
        min_size_bytes: 5 * 1024 * 1024,
        skip_efficient: false,
    };
    assert_eq!(form.compress_options().unwrap(), expected);
    for exporter in [Exporter::Imessage, Exporter::GoSmsPro] {
        let config = form.to_config(exporter, Path::new("/cache")).unwrap();
        assert_eq!(config.media.mode, MediaMode::Compress);
        assert_eq!(config.media.compress, expected, "{exporter:?}");
    }

    let bad = Form {
        media_max_fps: "abc".into(),
        ..form
    };
    let err = bad
        .to_config(Exporter::GoSmsPro, Path::new("/cache"))
        .unwrap_err();
    assert_eq!(
        err,
        vec![
            "Max FPS must be a number of frames per second above 0, such as 30, not 'abc'."
                .to_string()
        ]
    );
}

/// Every source takes the output directory, the Scratch Directory, and the
/// output format from the form; only iMazing reads the time zone field.
#[test]
fn every_source_carries_the_shared_form_fields() {
    let form = Form {
        input: concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into(),
        output: "  out  ".into(),
        owner_phones: "+15555550100".into(),
        owner_emails: "me@example.com".into(),
        timezone: "UTC-05:00".into(),
        attachment_media: AttachmentMedia::Clone,
        output_format: OutputFormat::Eml,
        ..Form::default()
    };
    for exporter in [
        Exporter::Imessage,
        Exporter::Whatsapp,
        Exporter::Imazing,
        Exporter::OpenExtract,
        Exporter::GoSmsPro,
        Exporter::SmsBackupRestore,
        Exporter::SmsBackupPlus,
    ] {
        let config = form
            .to_config(exporter, Path::new("/cache"))
            .unwrap_or_else(|errors| panic!("{exporter:?}: {errors:?}"));
        assert_eq!(config.output, PathBuf::from("out"), "{exporter:?}");
        assert_eq!(config.scratch_dir, PathBuf::from("/cache"), "{exporter:?}");
        assert_eq!(config.output_format, OutputFormat::Eml, "{exporter:?}");
        let timezone = (exporter == Exporter::Imazing).then_some("UTC-05:00");
        assert_eq!(config.timezone.as_deref(), timezone, "{exporter:?}");
        assert!(!config.resume, "{exporter:?}");
    }
}
