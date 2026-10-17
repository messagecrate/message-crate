//! The client side of the helper protocol, against small fake helpers.
//!
//! Each fake is a shell script that reads the request line and prints the
//! events a broken or mismatched `imessage-reader` would. They run on Unix
//! only; the real program is covered on every platform by
//! `tests/helper_process.rs`.

mod locating {
    use std::{fs, path::Path};

    use crate::helper::{HELPER_PATH_ENV, Places, executable_name, locate_in};

    fn nowhere<'a>() -> Places<'a> {
        Places {
            explicit: None,
            exe_dir: None,
        }
    }

    fn put_program(dir: &Path) -> std::path::PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(executable_name());
        fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn the_explicit_path_wins_over_everything_else() {
        let root = tempfile::tempdir().unwrap();
        let explicit = put_program(&root.path().join("custom"));
        let app = root.path().join("app");
        put_program(&app);

        let found = locate_in(&Places {
            explicit: Some(explicit.clone()),
            exe_dir: Some(&app),
        })
        .unwrap();
        assert_eq!(found, explicit);
    }

    #[test]
    fn an_explicit_path_that_is_not_a_file_is_an_error_not_a_fallback() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        put_program(&app);
        let missing = root.path().join("gone").join("imessage-reader");

        let err = locate_in(&Places {
            explicit: Some(missing.clone()),
            exe_dir: Some(&app),
        })
        .unwrap_err()
        .to_string();
        assert_eq!(
            err,
            format!(
                "{HELPER_PATH_ENV} is set but not a file: {}",
                missing.display()
            )
        );
    }

    #[test]
    fn the_program_beside_the_app_is_found() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        let program = put_program(&app);

        let found = locate_in(&Places {
            exe_dir: Some(&app),
            ..nowhere()
        })
        .unwrap();
        assert_eq!(found, program);
    }

    /// The program is looked for beside the app and nowhere else: a copy in
    /// the directory above is not used.
    #[test]
    fn a_program_in_the_directory_above_the_app_is_not_found() {
        let root = tempfile::tempdir().unwrap();
        put_program(root.path());
        let app = root.path().join("app");
        fs::create_dir_all(&app).unwrap();

        let err = locate_in(&Places {
            exe_dir: Some(&app),
            ..nowhere()
        })
        .unwrap_err()
        .to_string();
        let executable = executable_name();
        assert!(
            err.starts_with(&format!(
                "Could not find {executable}, the program that reads Apple Messages."
            )),
            "{err}"
        );
        assert!(
            err.ends_with(&format!("Tried: {}", app.join(&executable).display())),
            "{err}"
        );
    }
}

/// A request that expects a `source` event first.
#[cfg(unix)]
fn faults_request() -> imessage_reader_protocol::Request {
    faults::identities_request()
}

#[cfg(unix)]
mod faults {
    use imessage_reader_protocol::{
        Event, IdentitiesRequest, PROTOCOL_VERSION, Platform, Request, Source,
    };

    use crate::testutil::{fake_helper, source_line, spawn_fake};
    use message_crate_core::ProgressSink;

    /// A request that expects a `source` event first.
    pub(super) fn identities_request() -> Request {
        Request::Identities(IdentitiesRequest {
            source: Source {
                db_path: "/nowhere/chat.db".into(),
                platform: Platform::MacOs,
                backup_password: None,
            },
            scratch_dir: "/nowhere/scratch".into(),
        })
    }

    #[test]
    fn a_helper_on_another_protocol_version_is_refused_by_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(dir.path(), &source_line(PROTOCOL_VERSION + 41));
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "imessage-reader speaks protocol version {}, this app speaks {PROTOCOL_VERSION}; \
                 the two were not built together",
                PROTOCOL_VERSION + 41
            )
        );
    }

    #[test]
    fn a_helper_that_answers_without_a_source_event_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(
            dir.path(),
            r#"echo '{"event":"identities","values":["P:+15555550110"]}'"#,
        );
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert!(err.contains("protocol version"), "{err}");
        assert!(err.contains("not built together"), "{err}");
    }

    #[test]
    fn log_and_progress_lines_may_come_before_the_source_event() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "echo '{{\"event\":\"log\",\"line\":\"opening\"}}'\n\
             echo '{{\"event\":\"progress\",\"stage\":\"setup\",\"label\":\"keys\",\"step\":1,\"total\":2}}'\n\
             {}",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(
            helper.next_event().unwrap(),
            Event::Source {
                encrypted: false,
                ..
            }
        ));
        helper.finish().unwrap();
    }

    /// A warning from the program reaches the run's log as a warning, at
    /// `WARN`, and a log line as a line, at `INFO` (#2047).
    #[test]
    fn a_warning_from_the_program_is_a_warning_on_the_run_s_log() {
        use message_crate_core::RunLogLevel;
        use message_crate_core::testutil::recorded_log;

        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "echo '{{\"event\":\"log\",\"line\":\"opening\"}}'\n\
             echo '{{\"event\":\"warning\",\"line\":\"Contacts could not be read\"}}'\n\
             {}",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let (log, recorded) = recorded_log();
        let mut helper =
            crate::Helper::spawn_at(&path, &identities_request(), log, ProgressSink::none())
                .expect("start the fake helper");

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        helper.finish().unwrap();
        assert_eq!(
            *recorded.lock().unwrap(),
            [
                (RunLogLevel::Info, "opening".to_string()),
                (RunLogLevel::Warn, "Contacts could not be read".to_string()),
            ]
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_named_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(dir.path(), "echo 'Segmentation fault (core dumped)'");
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(
            err,
            "imessage-reader sent something unexpected: Segmentation fault (core dumped)"
        );
    }

    #[test]
    fn a_helper_that_dies_mid_stream_reports_its_status_and_stderr_tail() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\necho 'reading chat.db' >&2\necho 'panicked at out of memory' >&2\nexit 3",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(
            err,
            "imessage-reader stopped before finishing (exit status: 3): \
             reading chat.db | panicked at out of memory"
        );
    }

    /// A request to a program that has already exited says it stopped, with
    /// its status, whether the request line still fit in the pipe or the
    /// pipe was already closed (#1442). The second request always meets a
    /// closed pipe, which before read as a bare "Broken pipe" that did not
    /// say the program had stopped.
    #[test]
    fn a_request_to_a_helper_that_has_exited_says_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("{}\nexit 3", source_line(PROTOCOL_VERSION));
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        for attempt in 1..=2 {
            let err = helper
                .decrypt_attachment(std::path::Path::new("/backup/IMG_0001.JPG"))
                .unwrap_err();
            assert_eq!(
                format!("{err:#}"),
                "imessage-reader stopped before finishing (exit status: 3)",
                "request {attempt}"
            );
        }
    }

    /// A program that fails between two requests says why on stdout and
    /// exits. The request that then meets its closed pipe reports that
    /// reason after the "stopped" error, rather than the status alone. The
    /// script closes its stdin before it answers, so the request meets a
    /// closed pipe, unless another test's child, forked but not yet started,
    /// still holds a copy of that pipe; the write then lands and the error
    /// event is read as the answer. Either way the reason is in the error.
    #[test]
    fn a_request_to_a_helper_that_failed_carries_its_reason() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "exec 0<&-\n{}\necho '{{\"event\":\"log\",\"line\":\"opening Manifest.db\"}}'\n\
             echo '{{\"event\":\"error\",\"message\":\"could not read a request: bad\"}}'\nexit 1",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let logged = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = {
            let logged = Arc::clone(&logged);
            message_crate_core::LogSink::new(move |line| logged.lock().unwrap().push(line.into()))
        };
        let mut helper =
            crate::Helper::spawn_at(&path, &identities_request(), log, ProgressSink::none())
                .expect("start the fake helper");

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        let err = helper
            .decrypt_attachment(std::path::Path::new("/backup/IMG_0001.JPG"))
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text == "imessage-reader stopped before finishing (exit status: 1): \
                     could not read a request: bad"
                || text == "could not read a request: bad",
            "{text}"
        );
        assert_eq!(*logged.lock().unwrap(), ["opening Manifest.db"]);
    }

    #[test]
    fn a_helper_that_fails_after_answering_reports_its_status_on_finish() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\necho '{{\"event\":\"identities\",\"values\":[]}}'\necho 'lost the lock' >&2\nexit 3",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let mut helper = spawn_fake(&path, &identities_request());

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        assert!(matches!(
            helper.next_event().unwrap(),
            Event::Identities { .. }
        ));
        let err = helper.finish().unwrap_err().to_string();
        assert_eq!(
            err,
            "imessage-reader exited with exit status: 3: lost the lock"
        );
    }

    #[test]
    fn an_error_event_becomes_the_error_as_the_helper_worded_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_helper(
            dir.path(),
            r#"echo '{"event":"error","message":"The backup password is wrong."}'; exit 1"#,
        );
        let mut helper = spawn_fake(&path, &identities_request());

        let err = helper.next_event().unwrap_err().to_string();
        assert_eq!(err, "The backup password is wrong.");
    }
}

/// Cancel while the program sends nothing (#2085).
#[cfg(unix)]
mod cancel {
    use std::{
        os::unix::process::ExitStatusExt,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use imessage_reader_protocol::{Event, PROTOCOL_VERSION};
    use message_crate_core::{CancelFlag, Canceled, LogSink, ProgressSink};

    use super::faults_request;
    use crate::testutil::{fake_helper, source_line};

    /// A program that says it is decrypting and then sends nothing, ever:
    /// a long silent step, or a program stuck on a backup that went away.
    /// Cancel is set when the log line arrives, so it is set while the
    /// program is running and has nothing more to send. `next_event` must
    /// return [`Canceled`] rather than wait for a line that never comes,
    /// and the program must be gone: killed, not left holding the backup.
    #[test]
    fn cancel_stops_a_read_the_program_never_answers_and_kills_it() {
        let dir = tempfile::tempdir().unwrap();
        // `exec` puts `sleep` in the shell's place, so the process the
        // helper started is the one that goes quiet.
        let body = format!(
            "{}\necho '{{\"event\":\"log\",\"line\":\"decrypting\"}}'\nexec sleep 100000",
            source_line(PROTOCOL_VERSION)
        );
        let path = fake_helper(dir.path(), &body);
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let log = {
            let cancel = Arc::clone(&cancel);
            LogSink::new(move |line| {
                if line == "decrypting" {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
        };
        let mut helper =
            crate::Helper::spawn_at(&path, &faults_request(), log, ProgressSink::none())
                .expect("start the fake helper")
                .cancel_on(Arc::clone(&cancel));

        assert!(matches!(helper.next_event().unwrap(), Event::Source { .. }));
        let err = helper.next_event().unwrap_err();
        assert!(err.downcast_ref::<Canceled>().is_some(), "{err:#}");
        let status = helper
            .child
            .try_wait()
            .expect("ask whether the program exited")
            .expect("the program has exited");
        assert_eq!(status.signal(), Some(9), "killed, not exited: {status}");
    }
}
