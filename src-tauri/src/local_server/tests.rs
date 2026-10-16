//! Tests for the app's own Message Crate. The server here is a shell script
//! or an `httpmock` server, so nothing depends on the real program being
//! built; the script-based ones run on Unix only.

use super::*;
use httpmock::prelude::*;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The sockets holding the tests' ports, kept until the test program ends.
///
/// Each is bound and never listens, so a connection to its port is refused,
/// and a search for a free port passes the port over while it is held. A port that is
/// only let go of can be bound by any process on this computer, and a test
/// suite running beside this one binds ports all the time. A start watched
/// at such an address saw that process answer, and failed (#1783). Keeping
/// the sockets here, rather than in a value each test holds, leaves no test
/// a way to let its port go early.
static HELD_PORTS: Mutex<Vec<socket2::Socket>> = Mutex::new(Vec::new());

/// Whether a held port can also be bound by a listener of the test's own.
#[derive(Debug, Clone, Copy)]
enum Sharing {
    /// No other socket can bind the port.
    Alone,
    /// Another socket set up the same way can bind the port too. Sets
    /// `SO_REUSEADDR`, and `SO_REUSEPORT` on Apple systems, where
    /// `SO_REUSEADDR` alone does not let two sockets bind one address. A
    /// search for a free port still passes the port over, so only a program
    /// that binds this port number by name, with the same options, could
    /// share it.
    WithOwnListener,
}

/// Bind a socket that never listens to `address` and keep it until the test
/// program ends. Returns the address it is bound to.
fn hold_port(address: SocketAddr, sharing: Sharing) -> SocketAddr {
    let socket = bound_socket(address, sharing);
    let address = socket.local_addr().unwrap().as_socket().unwrap();
    HELD_PORTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(socket);
    address
}

/// A TCP socket bound to `address`, shared as `sharing` says.
fn bound_socket(address: SocketAddr, sharing: Sharing) -> socket2::Socket {
    let shared = matches!(sharing, Sharing::WithOwnListener);
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    socket.set_reuse_address(shared).unwrap();
    #[cfg(target_vendor = "apple")]
    socket.set_reuse_port(shared).unwrap();
    socket.bind(&address.into()).unwrap();
    socket
}

/// A loopback address that nothing listens on and nothing else can take.
fn free_address() -> SocketAddr {
    hold_port(SocketAddr::from(([127, 0, 0, 1], 0)), Sharing::Alone)
}

/// Wait for a listener this test let go of to be gone. A process another
/// test is starting holds a copy of every open socket until it runs its
/// program, and answers connections on it until then.
fn wait_until_free(address: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while probe(address) != Probe::Free {
        assert!(Instant::now() < deadline, "{address} is still listened on");
        thread::sleep(Duration::from_millis(20));
    }
}

/// A mock that answers `GET /v1/server` the way a Message Crate does.
fn message_crate() -> MockServer {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/server");
        then.status(200).json_body(serde_json::json!({
            "state": "unclaimed",
            "demo_account": true,
            "version": "0.10.0",
            "schema_fingerprint": 1,
        }));
    });
    server
}

/// Wait for a start to settle, failing the test if it never does.
fn settled(server: &LocalServer) -> Status {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = server.status();
        if !matches!(status, Status::Starting { .. }) {
            return status;
        }
        assert!(Instant::now() < deadline, "the start never settled");
        thread::sleep(Duration::from_millis(20));
    }
}

/// A launch with its Data Directory and its website side by side in
/// `parent`, as the packaged app keeps them apart: `serve` refuses a website
/// inside the Data Directory, or the reverse (#2682).
fn launch_at(address: SocketAddr, program: &Path, parent: &Path) -> Launch {
    Launch {
        program: program.to_path_buf(),
        data_dir: parent.join("data"),
        static_dir: parent.join("website"),
        address,
        open_to_network: false,
        cors_origins: Vec::new(),
        tools_dir: None,
    }
}

#[test]
fn probe_reports_a_free_port() {
    assert_eq!(probe(free_address()), Probe::Free);
}

#[test]
fn probe_recognizes_a_message_crate() {
    let server = message_crate();
    assert_eq!(probe(*server.address()), Probe::MessageCrate);
}

#[test]
fn probe_reports_another_program_on_the_port() {
    // Answers every path, `/v1/server` included, with something that is not
    // a Message Crate's answer.
    let server = MockServer::start();
    server.mock(|when, then| {
        when.any_request();
        then.status(200).body("<html>router admin</html>");
    });
    assert_eq!(probe(*server.address()), Probe::Other);

    let not_found = MockServer::start();
    assert_eq!(probe(*not_found.address()), Probe::Other);
}

#[test]
fn the_server_is_started_on_this_computer_with_no_config_file() {
    let launch = Launch {
        program: PathBuf::from("message-crate-server"),
        data_dir: PathBuf::from("/data"),
        static_dir: PathBuf::from("/site"),
        address: OWN_ADDRESS.parse().unwrap(),
        open_to_network: false,
        cors_origins: Vec::new(),
        tools_dir: None,
    };
    assert_eq!(
        Launch {
            cors_origins: vec!["http://localhost:5173".into()],
            ..launch.clone()
        }
        .arguments()[9..],
        ["--cors-origin", "http://localhost:5173"]
    );
    // The app names itself, so the server ends if the app crashes (#1934).
    assert_eq!(
        launch.arguments(),
        [
            "serve",
            "--data-dir",
            "/data",
            "--bind",
            "127.0.0.1:8080",
            "--static-dir",
            "/site",
            "--exit-with-parent",
            &std::process::id().to_string(),
        ]
    );
}

/// The server looks for ffmpeg where the app does: on `PATH`, then in the
/// app's Tools Directory, which the app names to it (#1053).
#[test]
fn the_server_is_told_the_tools_directory() {
    let launch = Launch {
        tools_dir: Some(PathBuf::from("/home/someone/message-crate/tools")),
        ..launch_at(
            OWN_ADDRESS.parse().unwrap(),
            Path::new("unused"),
            Path::new("/app"),
        )
    };
    assert_eq!(
        launch.arguments()[9..],
        ["--tools-dir", "/home/someone/message-crate/tools"]
    );
}

#[test]
fn open_to_the_network_listens_on_every_address_at_the_same_port() {
    let launch = Launch {
        open_to_network: true,
        ..launch_at(
            OWN_ADDRESS.parse().unwrap(),
            Path::new("unused"),
            Path::new("/app"),
        )
    };
    assert_eq!(launch.arguments()[3..5], ["--bind", "0.0.0.0:8080"]);
    // The app still asks this computer's own address what is running.
    assert_eq!(launch.address.to_string(), OWN_ADDRESS);
}

/// A state with `phase`, the setting `wanted_open`, and no job running.
fn at(phase: Phase, wanted_open: bool) -> State {
    State {
        phase,
        wanted_open,
        jobs: 0,
        restarts: Vec::new(),
        recovering: false,
    }
}

/// The address answered `answer`, with nothing written by the app's server.
fn probed(answer: Probe) -> Event {
    Event::Probed {
        answer,
        output: String::new(),
        found_at: Instant::now(),
    }
}

/// The app's own server, ready, started open to the network when `open`,
/// with no check of it under way and none failed.
fn own(open: bool) -> Phase {
    Phase::Own {
        open,
        checking: false,
        unanswered: 0,
    }
}

/// The app found its own server had exited at `found_at`, having written
/// `output`.
fn exit_found_at(found_at: Instant, output: &str) -> Event {
    Event::ChildExited {
        output: output.into(),
        code: Some(1),
        found_at,
    }
}

/// The app's own server, ready, exits at each of `times` in turn, and each
/// restart listens again. Returns the state and actions after the last exit.
fn exits_at(mut state: State, times: &[Instant]) -> (State, Vec<Action>) {
    let mut actions = Vec::new();
    for &time in times {
        if matches!(state.phase, Phase::Starting { .. }) {
            (state, _) = step(state, Event::Listening);
        }
        (state, actions) = step(state, exit_found_at(time, "crashed"));
    }
    (state, actions)
}

/// A ready server that exits on its own, as in a crash, is started again, so
/// an Upload in progress gets its server back without anyone pressing Retry
/// (#2089).
#[test]
fn the_apps_own_server_is_started_again_when_it_exits() {
    let now = Instant::now();
    let (state, actions) = step(at(own(false), false), exit_found_at(now, "crashed"));
    assert_eq!(
        actions,
        [
            Action::Spawn { open: false },
            Action::Notify(NoticeKind::Restarting)
        ]
    );
    assert_eq!(state.phase, Phase::Starting { open: false });

    // Started the way the person wants it now.
    let (state, actions) = step(at(own(false), true), exit_found_at(now, "crashed"));
    assert_eq!(
        actions,
        [
            Action::Spawn { open: true },
            Action::Notify(NoticeKind::Restarting)
        ]
    );
    assert_eq!(state.phase, Phase::Starting { open: true });

    // The window is told once the new one answers.
    let (state, actions) = step(state, Event::Listening);
    assert_eq!(actions, [Action::Notify(NoticeKind::Answering)]);
    assert_eq!(state.phase, own(true));
}

/// A server that keeps exiting is not started forever: after
/// [`MAX_RESTARTS`] restarts within [`RESTART_WINDOW`], the next exit fails
/// as before, with what it last wrote.
#[test]
fn a_server_that_keeps_exiting_is_started_again_only_a_few_times() {
    let now = Instant::now();
    let times: Vec<Instant> = (0..=MAX_RESTARTS)
        .map(|n| now + Duration::from_secs(n.into()))
        .collect();
    let (state, actions) = exits_at(at(own(false), false), &times[..times.len() - 1]);
    assert_eq!(
        actions,
        [
            Action::Spawn { open: false },
            Action::Notify(NoticeKind::Restarting)
        ]
    );

    let (state, actions) = exits_at(state, &times[times.len() - 1..]);
    assert_eq!(actions, [Action::Notify(NoticeKind::Failed)]);
    assert_eq!(
        state.phase,
        Phase::Failed {
            reason: FailureReason::StartFailed,
            message: "Message Crate stopped 4 times within 10 minutes.".into(),
            details: "crashed".into(),
        }
    );

    // Retry on the login card starts the count again.
    let (state, actions) = step(state, Event::Requested { open: false });
    assert_eq!(actions, [Action::Probe]);
    let (state, _) = step(state, probed(Probe::Free));
    let (_, actions) = exits_at(state, &[now + RESTART_WINDOW / 2]);
    assert_eq!(
        actions,
        [
            Action::Spawn { open: false },
            Action::Notify(NoticeKind::Restarting)
        ]
    );
}

/// Exits further apart than [`RESTART_WINDOW`] are each started again: only
/// the restarts within the window count.
#[test]
fn exits_far_apart_are_always_started_again() {
    let now = Instant::now();
    let times: Vec<Instant> = (0..=MAX_RESTARTS * 3)
        .map(|n| now + RESTART_WINDOW * n)
        .collect();
    let mut state = at(own(false), false);
    for time in times {
        let actions;
        (state, actions) = exits_at(state, &[time]);
        assert_eq!(
            actions,
            [
                Action::Spawn { open: false },
                Action::Notify(NoticeKind::Restarting)
            ],
            "{time:?}"
        );
    }
}

#[test]
fn turning_the_network_off_during_a_start_restarts_the_server_closed() {
    let (state, actions) = step(
        at(Phase::Starting { open: true }, true),
        Event::Requested { open: false },
    );
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: false }]);
    assert_eq!(state.phase, Phase::Starting { open: false });

    // The setting alone does the same.
    let (_, actions) = step(
        at(Phase::Starting { open: true }, true),
        Event::Configured { open: false },
    );
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: false }]);
}

#[test]
fn the_network_setting_starts_nothing_and_leaves_a_found_message_crate_alone() {
    // On another address the app never started its own server.
    for phase in [
        Phase::Idle,
        Phase::Found { checking: false },
        Phase::PortTaken,
    ] {
        let (state, actions) = step(at(phase.clone(), false), Event::Configured { open: true });
        assert_eq!(actions, [], "{phase:?}");
        assert_eq!(state.phase, phase);
        assert!(state.wanted_open);
    }
}

#[test]
fn ticking_the_network_setting_on_another_address_starts_no_server() {
    let server = LocalServer::default();
    assert_eq!(server.set_open_to_network(true), Status::Idle);
    thread::sleep(POLL_INTERVAL * 2);
    assert_eq!(server.status(), Status::Idle);
    assert!(server.lock().server.is_none());
}

#[test]
fn a_ready_server_is_restarted_for_the_network_setting_only_once_no_job_runs() {
    let (state, _) = step(at(own(false), false), Event::JobStarted);
    let (state, actions) = step(state, Event::Configured { open: true });
    assert_eq!(actions, [], "the import would be cut off");
    assert_eq!(state.phase, own(false));

    let (state, actions) = step(state, Event::JobEnded);
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: true }]);
    assert_eq!(state.phase, Phase::Starting { open: true });

    // With no job, at once.
    let (_, actions) = step(at(own(false), false), Event::Configured { open: true });
    assert_eq!(actions, [Action::Kill, Action::Spawn { open: true }]);
}

#[test]
fn a_found_message_crate_is_asked_again_and_replaced_when_it_stops() {
    let found = at(Phase::Found { checking: false }, false);
    let (state, actions) = step(found.clone(), Event::Check);
    assert_eq!(actions, [Action::Probe]);
    // A second look while the first is under way asks nothing more.
    let (state, actions) = step(state, Event::Check);
    assert_eq!(actions, []);
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(actions, [Action::Spawn { open: false }]);
    assert_eq!(state.phase, Phase::Starting { open: false });

    // Retry asks again too.
    let (state, actions) = step(found, Event::Requested { open: false });
    assert_eq!(actions, [Action::Probe]);
    assert_eq!(state.phase, Phase::Probing);
}

/// The app's own server that still runs but no longer answers is killed and
/// started again, rather than left ready while every screen fails (#2204,
/// #2695). The window is told when it is killed and when the new one answers.
#[test]
fn the_apps_own_server_is_killed_and_started_again_once_it_stops_answering() {
    let ready = at(own(false), false);
    let (state, actions) = step(ready, Event::Check);
    assert_eq!(actions, [Action::Probe]);
    // A second look while the first is under way asks nothing more.
    let (state, actions) = step(state, Event::Check);
    assert_eq!(actions, []);

    // One check that finds nothing answering is not enough: the server may
    // have been slow once.
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(actions, []);
    assert!(matches!(state.phase, Phase::Own { .. }), "{state:?}");
    // An answer in between starts the count again.
    let (state, _) = step(state, Event::Check);
    let (state, _) = step(state, probed(Probe::MessageCrate));
    let (state, _) = step(state, Event::Check);
    let (state, actions) = step(state, probed(Probe::Other));
    assert_eq!(actions, []);
    assert!(matches!(state.phase, Phase::Own { .. }), "{state:?}");

    // The next check that finds nothing answering ends it, and starts it
    // again the way the person wants it.
    let (state, actions) = step(state, Event::Check);
    assert_eq!(actions, [Action::Probe]);
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(
        actions,
        [
            Action::Kill,
            Action::Spawn { open: false },
            Action::Notify(NoticeKind::Restarting)
        ]
    );
    assert_eq!(state.phase, Phase::Starting { open: false });

    let (state, actions) = step(state, Event::Listening);
    assert_eq!(actions, [Action::Notify(NoticeKind::Answering)]);
    assert_eq!(state.phase, own(false));
}

/// A server that stops answering counts against the same limit as one that
/// exits: one that keeps stopping is not started forever, and the window is
/// told it stopped, with what it last wrote.
#[test]
fn a_server_that_keeps_stopping_answering_is_started_again_only_a_few_times() {
    let now = Instant::now();
    let mut state = at(own(false), false);
    state.restarts = vec![now; MAX_RESTARTS as usize];
    let (state, _) = step(state, Event::Check);
    let (state, _) = step(state, probed(Probe::Free));
    let (state, _) = step(state, Event::Check);
    let (state, actions) = step(
        state,
        Event::Probed {
            answer: Probe::Free,
            output: "stuck here".into(),
            found_at: now,
        },
    );
    assert_eq!(actions, [Action::Kill, Action::Notify(NoticeKind::Failed)]);
    assert_eq!(
        state.phase,
        Phase::Failed {
            reason: FailureReason::StartFailed,
            message: "Message Crate stopped 4 times within 10 minutes.".into(),
            details: "stuck here".into(),
        }
    );
}

/// A restart that fails stays failed, and the window is told why. Once a
/// Retry brings a Message Crate back, the window is told it answers.
/// Closing the app during a restart tells the window nothing.
#[test]
fn a_restart_that_fails_tells_the_window_and_so_does_a_retry_that_answers() {
    let (restarting, _) = step(
        at(own(false), false),
        exit_found_at(Instant::now(), "crashed"),
    );

    let (state, actions) = step(
        restarting.clone(),
        Event::SpawnFailed {
            error: "no such file".into(),
        },
    );
    assert_eq!(actions, [Action::Notify(NoticeKind::Failed)]);
    assert!(matches!(state.phase, Phase::Failed { .. }), "{state:?}");

    // Retry on the login card or on the notice.
    let (state, actions) = step(state, Event::Requested { open: false });
    assert_eq!(actions, [Action::Probe]);
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(actions, [Action::Spawn { open: false }]);
    let (state, actions) = step(state, Event::Listening);
    assert_eq!(actions, [Action::Notify(NoticeKind::Answering)]);
    // Told once.
    let (state, _) = step(state, Event::Check);
    let (_, actions) = step(state, probed(Probe::MessageCrate));
    assert_eq!(actions, []);

    let (state, actions) = step(restarting, Event::Stopped);
    assert_eq!(actions, [Action::Kill]);
    assert!(!state.recovering);
}

/// An import can keep the server from answering in time, so the app's own
/// server is not killed while a desktop job runs. Once none does, two more
/// checks in a row must find nothing answering.
#[test]
fn the_apps_own_server_is_not_killed_for_not_answering_while_a_job_runs() {
    let (mut state, _) = step(at(own(false), false), Event::JobStarted);
    for _ in 0..UNANSWERED_CHECKS + 1 {
        let actions;
        (state, _) = step(state, Event::Check);
        (state, actions) = step(state, probed(Probe::Other));
        assert_eq!(actions, [], "the import would be cut off");
        assert!(matches!(state.phase, Phase::Own { .. }), "{state:?}");
    }

    let (state, _) = step(state, Event::JobEnded);
    let (state, _) = step(state, Event::Check);
    let (state, actions) = step(state, probed(Probe::Other));
    assert_eq!(actions, [], "the misses during the job do not count");
    let (state, _) = step(state, Event::Check);
    let (state, actions) = step(state, probed(Probe::Other));
    assert_eq!(
        actions,
        [
            Action::Kill,
            Action::Spawn { open: false },
            Action::Notify(NoticeKind::Restarting)
        ]
    );
    assert_eq!(state.phase, Phase::Starting { open: false });
}

/// An answer to a check the app's own server is not waiting for is not
/// counted.
#[test]
fn an_answer_nobody_asked_the_apps_own_server_for_is_not_counted() {
    let (state, actions) = step(at(own(false), false), probed(Probe::Free));
    assert_eq!(actions, []);
    assert_eq!(state.phase, own(false));
}

/// An answer asked before the app's server was killed or started, such as
/// one asked before a restart for the network setting, is dropped: it is
/// about the server that was replaced.
#[test]
fn an_answer_asked_of_a_replaced_server_is_dropped() {
    let server = LocalServer::default();
    let checking = Phase::Own {
        open: false,
        checking: true,
        unanswered: 0,
    };
    {
        let mut inner = server.lock();
        inner.state = at(checking.clone(), false);
        inner.generation = 2;
    }

    server.probed(1, Probe::Free);
    assert_eq!(server.lock().state.phase, checking);

    server.probed(2, Probe::Free);
    assert_eq!(
        server.lock().state.phase,
        Phase::Own {
            open: false,
            checking: false,
            unanswered: 1,
        }
    );
}

#[test]
fn only_the_apps_own_server_saying_it_listens_makes_the_start_its_own() {
    let starting = at(Phase::Starting { open: false }, false);
    // Another Message Crate answering the port is not the app's server.
    let (state, actions) = step(starting.clone(), probed(Probe::MessageCrate));
    assert_eq!(state.phase, Phase::Starting { open: false });
    assert_eq!(actions, []);

    let (state, _) = step(starting.clone(), Event::Listening);
    assert_eq!(state.phase, own(false));

    // A server that exits leaves the port to whichever Message Crate answers.
    let (state, actions) = step(
        starting.clone(),
        Event::ChildExited {
            output: "lost".into(),
            code: Some(1),
            found_at: Instant::now(),
        },
    );
    assert_eq!(actions, [Action::Probe]);
    let (state, _) = step(state, probed(Probe::MessageCrate));
    assert_eq!(state.phase, Phase::Found { checking: false });

    // Locked out by the server of a second window, still being set up: the
    // address is asked again until it answers. The exit code says so, not
    // the words.
    let (state, _) = step(
        starting.clone(),
        Event::ChildExited {
            output: "any words at all".into(),
            code: Some(i32::from(OPERATION_LOCK_HELD_EXIT_CODE)),
            found_at: Instant::now(),
        },
    );
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(actions, [Action::ProbeLater]);
    let (state, _) = step(state, probed(Probe::MessageCrate));
    assert_eq!(state.phase, Phase::Found { checking: false });

    // Any other exit with nothing answering is a failure, even one whose
    // words say another server is active.
    let (state, _) = step(
        starting.clone(),
        Event::ChildExited {
            output: "while reset-demo or another server is active".into(),
            code: Some(1),
            found_at: Instant::now(),
        },
    );
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(actions, []);
    assert!(matches!(state.phase, Phase::Failed { .. }), "{state:?}");
    let (state, _) = step(
        starting,
        Event::ChildExited {
            output: "database is locked".into(),
            code: None,
            found_at: Instant::now(),
        },
    );
    let (state, actions) = step(state, probed(Probe::Free));
    assert_eq!(actions, []);
    assert!(
        matches!(&state.phase, Phase::Failed { details, .. } if details == "database is locked"),
        "{state:?}"
    );
}

#[test]
fn a_message_crate_already_answering_is_used_and_nothing_is_started() {
    let existing = message_crate();
    let dir = tempfile::tempdir().unwrap();
    // A program that does not exist: starting it would fail the test.
    let launch = launch_at(*existing.address(), &dir.path().join("absent"), dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch);

    assert_eq!(
        settled(&server),
        Status::Ready {
            started_by_app: false
        }
    );
    assert!(server.lock().server.is_none());
}

#[test]
fn another_program_on_the_port_is_reported_by_port_number() {
    let other = MockServer::start();
    let dir = tempfile::tempdir().unwrap();
    let launch = launch_at(*other.address(), &dir.path().join("absent"), dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch);

    let Status::Failed {
        reason, message, ..
    } = settled(&server)
    else {
        panic!("a taken port must fail the start");
    };
    assert_eq!(reason, FailureReason::PortTaken);
    assert!(
        message.contains(&other.address().port().to_string()),
        "{message}"
    );
}

#[test]
fn a_missing_server_program_fails_the_start_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("absent");
    let launch = launch_at(free_address(), &program, dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch);

    let Status::Failed {
        reason, details, ..
    } = settled(&server)
    else {
        panic!("a missing program must fail the start");
    };
    assert_eq!(reason, FailureReason::StartFailed);
    assert!(details.contains("absent"), "{details}");
}

#[test]
fn the_first_start_is_the_one_with_no_database() {
    let dir = tempfile::tempdir().unwrap();
    let launch = launch_at(free_address(), Path::new("unused"), dir.path());
    assert!(launch.is_first_start());
    std::fs::create_dir_all(&launch.data_dir).unwrap();
    std::fs::write(launch.data_dir.join(DATABASE_FILE), b"").unwrap();
    assert!(!launch.is_first_start());
}

#[test]
fn a_found_message_crate_that_stops_is_not_reported_ready() {
    // A Message Crate that can be stopped: httpmock keeps its servers
    // listening after they are dropped. Its port is held before it listens,
    // so once it stops, a search for a free port passes the port over and a
    // connection to it is refused (#1789).
    let address = hold_port(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Sharing::WithOwnListener,
    );
    let socket = bound_socket(address, Sharing::WithOwnListener);
    socket.listen(128).unwrap();
    let listener = TcpListener::from(socket);
    listener.set_nonblocking(true).unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    let existing = thread::spawn(move || {
        use std::io::Write;
        while flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let mut buf = [0u8; 4096];
                    let _ = std::io::Read::read(&mut stream, &mut buf);
                    let body = r#"{"schema_fingerprint":1}"#;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let launch = launch_at(address, &dir.path().join("absent"), dir.path());

    let server = LocalServer::default();
    server.ensure_started(launch.clone());
    assert_eq!(
        settled(&server),
        Status::Ready {
            started_by_app: false
        }
    );

    // Docker is stopped, or the other app instance closes.
    running.store(false, Ordering::Relaxed);
    existing.join().unwrap();
    wait_until_free(address);
    // Retry on the login card.
    server.ensure_started(launch);

    assert_ne!(
        settled(&server),
        Status::Ready {
            started_by_app: false
        },
        "nothing answers at the address any more"
    );
}

/// A Message Crate that is busy is still a Message Crate (ADR 0018). Neither
/// a slow answer nor a 503 may make the app say another program holds the
/// port.
#[test]
fn a_slow_or_busy_message_crate_is_not_another_program() {
    let slow = MockServer::start();
    slow.mock(|when, then| {
        when.method(GET).path("/v1/server");
        then.status(200)
            .delay(Duration::from_secs(3))
            .json_body(serde_json::json!({ "schema_fingerprint": 1 }));
    });
    let busy = MockServer::start();
    busy.mock(|when, then| {
        when.method(GET).path("/v1/server");
        then.status(503).json_body(serde_json::json!({
            "type": "about:blank", "status": 503, "request_id": "x"
        }));
    });
    let slow_probe = probe(*slow.address());
    let busy_probe = probe(*busy.address());
    assert_eq!(
        (slow_probe, busy_probe),
        (Probe::MessageCrate, Probe::MessageCrate),
        "slow: {slow_probe:?}, 503: {busy_probe:?}"
    );
}

#[cfg(unix)]
mod with_a_script {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Write an executable shell script standing in for the server.
    ///
    /// A process another test is starting holds a copy of the file as it was
    /// open for writing, and until it runs its own program, running the
    /// script fails with "Text file busy". So the script is run once with
    /// `ready`, which only exits, until that works.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fake-server");
        std::fs::write(
            &path,
            format!("#!/bin/sh\n[ \"$1\" = ready ] && exit 0\n{body}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Command::new(&path).arg("ready").status().is_err() {
            assert!(
                Instant::now() < deadline,
                "the script never became runnable"
            );
            thread::sleep(Duration::from_millis(20));
        }
        path
    }

    /// The process id a script wrote to `started`, once it is there.
    fn started_process(started: &Path) -> libc::pid_t {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(text) = std::fs::read_to_string(started) {
                return text.trim().parse().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "the server never started its process"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Whether process `pid` ends within ten seconds. A killed process is a
    /// zombie until the system reaps it, which a container whose first
    /// process reaps nothing never does, so a zombie counts as ended. One
    /// that is still running at the end is killed here, so the test leaves
    /// nothing running.
    fn ends_or_is_killed(pid: libc::pid_t) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // SAFETY: signal 0 checks the process exists and sends nothing.
            let exists = unsafe { libc::kill(pid, 0) } == 0;
            if !exists || is_zombie(pid) {
                return true;
            }
            if Instant::now() >= deadline {
                // SAFETY: ends the test's own process, which outlived the
                // server.
                unsafe { libc::kill(pid, libc::SIGKILL) };
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Whether `pid` is a zombie: state `Z` in `/proc/<pid>/stat` on Linux.
    /// Other systems have no `/proc`, so a zombie there counts as running.
    fn is_zombie(pid: libc::pid_t) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            // The state follows the command name, which is in parentheses
            // and may itself hold spaces or parentheses.
            stat.rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
        })
    }

    #[test]
    fn a_server_that_stops_while_starting_fails_with_what_it_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "echo 'database is locked' >&2\nexit 3");
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);

        let Status::Failed {
            reason, details, ..
        } = settled(&server)
        else {
            panic!("a server that exits must fail the start");
        };
        assert_eq!(reason, FailureReason::StartFailed);
        assert!(details.contains("database is locked"), "{details}");
    }

    #[test]
    fn a_server_refused_the_operation_lock_waits_for_the_other_server() {
        let dir = tempfile::tempdir().unwrap();
        // Told apart by its exit code alone, whatever it wrote.
        let program = script(
            dir.path(),
            &format!("echo 'any words at all' >&2\nexit {OPERATION_LOCK_HELD_EXIT_CODE}"),
        );
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);
        let deadline = Instant::now() + Duration::from_secs(20);
        let phase = loop {
            let phase = server.lock().state.phase.clone();
            if !matches!(phase, Phase::Probing | Phase::Starting { .. }) {
                break phase;
            }
            assert!(Instant::now() < deadline, "the server never exited");
            thread::sleep(Duration::from_millis(20));
        };
        server.stop();

        // Nothing answers the address, and the app keeps asking it.
        assert!(
            matches!(phase, Phase::Lost { probes_left, .. } if probes_left > 0),
            "{phase:?}"
        );
    }

    #[test]
    fn stop_ends_the_server_the_app_started() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "exec sleep 600");
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);
        // The script never answers, so the start stays under way; wait for
        // the process to exist.
        let deadline = Instant::now() + Duration::from_secs(20);
        let pid = loop {
            if let Some(process) = server.lock().server.as_ref() {
                break process.id();
            }
            assert!(Instant::now() < deadline, "the server was never started");
            thread::sleep(Duration::from_millis(20));
        };

        server.stop();

        assert!(server.lock().server.is_none());
        // `stop` waited for the process, so its id is no longer a process.
        assert!(!Path::new(&format!("/proc/{pid}")).exists() || !cfg!(target_os = "linux"));
        // The start that was waiting on it now reports the failure.
        assert!(matches!(settled(&server), Status::Failed { .. }));
    }

    #[test]
    fn stop_ends_the_processes_the_server_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let started = dir.path().join("started");
        // Starts a process of its own, the way the server starts ffmpeg,
        // writes down its id, and waits for it.
        let program = script(
            dir.path(),
            &format!(
                "sleep 600 &\necho $! > {}.part\nmv {0}.part {0}\nwait",
                started.display()
            ),
        );
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);
        let pid = started_process(&started);

        server.stop();

        assert!(
            ends_or_is_killed(pid),
            "the server's own process outlived the server"
        );
    }

    #[test]
    fn a_server_that_exits_on_its_own_takes_its_processes_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let started = dir.path().join("started");
        // Starts a process of its own, the way the server starts ffmpeg,
        // writes down its id, and exits without it.
        let program = script(
            dir.path(),
            &format!(
                "sleep 600 &\necho $! > {}.part\nmv {0}.part {0}\nexit 3",
                started.display()
            ),
        );
        let launch = launch_at(free_address(), &program, dir.path());

        let server = LocalServer::default();
        server.ensure_started(launch);
        let pid = started_process(&started);

        assert!(
            ends_or_is_killed(pid),
            "the server's own process outlived the server's exit"
        );
        assert!(matches!(settled(&server), Status::Failed { .. }));
    }

    /// A server that says it listens and then never answers is found by the
    /// app's own checks, with no screen asking for the state, killed, and
    /// started again (#2695). The checks run on a clock, so this test waits
    /// on one.
    #[test]
    fn a_ready_server_that_never_answers_is_killed_and_started_again_by_the_apps_own_checks() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            &format!("echo '{LISTENING_LINE}{OWN_ADDRESS}'\nexec sleep 600"),
        );
        let server = LocalServer::default();
        let (told, notices) = std::sync::mpsc::channel();
        server.on_notice(move |notice| {
            let _ = told.send(notice);
        });
        server.ensure_started(launch_at(free_address(), &program, dir.path()));
        assert_eq!(
            settled(&server),
            Status::Ready {
                started_by_app: true
            }
        );
        let first = server.lock().server.as_ref().unwrap().id();

        // Nothing asks for the state, which would check the address: the
        // app checks on its own. Nothing listens there, so the checks find
        // no Message Crate, and the server is killed and started again. The
        // new one says it listens, so the window is told it answers.
        let wait = Duration::from_secs(60);
        assert_eq!(notices.recv_timeout(wait), Ok(Notice::Restarting));
        assert_eq!(notices.recv_timeout(wait), Ok(Notice::Answering));
        let running = server.lock().server.as_ref().map(ServerProcess::id);
        assert!(running.is_some_and(|pid| pid != first), "{running:?}");
        server.stop();
        assert!(
            ends_or_is_killed(libc::pid_t::try_from(first).unwrap()),
            "the server outlived the kill"
        );
    }

    #[test]
    fn turning_the_network_setting_off_during_a_start_is_applied() {
        let dir = tempfile::tempdir().unwrap();
        // A pipe each server writes the address it was told to listen on
        // into. Writing waits for the test to read and reading waits for a
        // server to write, so the test reads each server's address as it
        // comes, with no clock.
        let binds = dir.path().join("binds");
        let path = std::ffi::CString::new(binds.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `path` is a NUL-terminated path the call only reads.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        // Says where it was told to listen, then never answers.
        let program = script(
            dir.path(),
            &format!("echo \"$5\" > {}\nexec sleep 600", binds.display()),
        );
        let address = free_address();
        let next_bind = || std::fs::read_to_string(&binds).unwrap().trim().to_string();
        let server = LocalServer::default();
        server.ensure_started(Launch {
            open_to_network: true,
            ..launch_at(address, &program, dir.path())
        });
        // The app asks the address before it starts anything. Only a start
        // brings a writer to the pipe, so anything else fails here rather
        // than leave the read below waiting.
        assert_eq!(
            server.phase_after(|phase| *phase == Phase::Probing),
            Phase::Starting { open: true },
            "the app started no server"
        );
        assert_eq!(
            next_bind(),
            SocketAddr::from(([0, 0, 0, 0], address.port())).to_string(),
            "the first server listens on every address"
        );

        // The person unticks the setting while the start is under way.
        server.ensure_started(launch_at(address, &program, dir.path()));
        // The app has killed the open server and started a closed one by the
        // time the call returns, so the pipe has a writer coming.
        assert_eq!(
            server.lock().state.phase,
            Phase::Starting { open: false },
            "no closed server was started in place of the open one"
        );
        let restarted = next_bind();
        server.stop();
        assert_eq!(
            restarted,
            address.to_string(),
            "the running server listens where the setting no longer says"
        );
    }
}

/// The Job Object the app's server runs in, on Windows (#1962). The Unix
/// tests above cover the process group; Linux and macOS share that code.
///
/// The server here is this test program, run again as a stand-in. Started as
/// the parent, it waits to be told it is in the Job Object, then starts
/// itself once more as a child, the way the server starts ffmpeg, writes the
/// child's process id, and waits for it. The child runs until something ends
/// it. Each test reads the child's id from the parent's output, so it is told
/// when both have started, and waits on the processes' handles, so it is told
/// when each has ended.
#[cfg(windows)]
mod in_a_job {
    use super::*;
    use std::io::Write;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };

    /// The environment variable that tells [`stand_in`] which process to be.
    const ROLE: &str = "MESSAGE_CRATE_STAND_IN_ROLE";

    /// What the parent writes before the child's process id. libtest has
    /// already written `test <name> ... ` on the same line, with no line
    /// break, so the words are looked for anywhere in a line.
    const CHILD_LINE: &str = "stand-in child: ";

    /// The stand-in server and its child, when this test program is run
    /// again by [`run_as`]. Run any other way it does nothing.
    #[test]
    #[ignore = "the stand-in the tests below start as their server and its child"]
    fn stand_in() {
        match std::env::var(ROLE).as_deref() {
            Ok("parent") => {
                // The app puts the server in its job just after starting it,
                // and the real server starts nothing in that moment. The
                // stand-in waits for the line that says the job holds it, so
                // its child always starts inside the job.
                let mut go = String::new();
                std::io::stdin()
                    .read_line(&mut go)
                    .expect("read the go-ahead");
                let mut child = run_as("child")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                    .expect("start the child");
                let mut out = std::io::stdout();
                writeln!(out, "{CHILD_LINE}{}", child.id()).unwrap();
                out.flush().unwrap();
                let _ = child.wait();
            }
            Ok("child") => loop {
                thread::park();
            },
            _ => {}
        }
    }

    /// This test program, running [`stand_in`] alone as `role`.
    fn run_as(role: &str) -> Command {
        // libtest names a test by its path without the crate's name.
        let (_, module) = module_path!().split_once("::").unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([&format!("{module}::stand_in"), "--exact", "--ignored"])
            .args(["--nocapture", "--test-threads=1"])
            .env(ROLE, role);
        command
    }

    /// The stand-in server, started the way the app starts its server, with
    /// a handle to it and to the child it started.
    struct Started {
        server: ServerProcess,
        parent: OwnedHandle,
        child: OwnedHandle,
    }

    /// Start the stand-in server the way the app starts its server, and
    /// return it once it has started its child.
    fn start_server() -> Started {
        let mut command = run_as("parent");
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_console(&mut command);
        let mut server = ServerProcess::spawn(&mut command).expect("start the stand-in server");
        // `spawn` returns with the server in the job: tell it to go on.
        let mut go = server.child().stdin.take().unwrap();
        writeln!(go, "go").expect("tell the stand-in server to go on");
        drop(go);
        let parent = open_process(server.child().id());
        let stdout = server.child().stdout.take().unwrap();
        let child = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .find_map(|line| line.split_once(CHILD_LINE).map(|(_, pid)| pid.to_owned()))
            .expect("the stand-in server ended before it started its child");
        let child = open_process(child.trim().parse().unwrap());
        Started {
            server,
            parent,
            child,
        }
    }

    /// A handle to process `pid`, to wait on and to end it with. A running
    /// process keeps its id, so the handle names the process the id did.
    fn open_process(pid: u32) -> OwnedHandle {
        // SAFETY: the call takes no pointers.
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
        assert!(
            !raw.is_null(),
            "open process {pid}: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: `raw` is a handle just opened and owned by nothing else.
        unsafe { OwnedHandle::from_raw_handle(raw) }
    }

    /// Whether `process` ends. The wait returns as soon as it ends; its
    /// minute only turns a process that never ends into a failure rather
    /// than a test that never returns. One still running then is ended here,
    /// so a failed test leaves nothing running.
    fn ends(process: &OwnedHandle) -> bool {
        // SAFETY: `process` is an open process handle with SYNCHRONIZE.
        let ended =
            unsafe { WaitForSingleObject(process.as_raw_handle(), 60_000) } == WAIT_OBJECT_0;
        if !ended {
            // SAFETY: `process` is an open process handle with TERMINATE.
            unsafe { TerminateProcess(process.as_raw_handle(), 1) };
        }
        ended
    }

    /// Whether `process` is running now. Returns at once.
    fn is_running(process: &OwnedHandle) -> bool {
        // SAFETY: `process` is an open process handle with SYNCHRONIZE.
        unsafe { WaitForSingleObject(process.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }

    /// The app closing: `stop` kills its server this way.
    #[test]
    fn killing_the_server_ends_the_process_it_started() {
        let Started {
            server,
            parent,
            child,
        } = start_server();

        server.kill();

        assert!(ends(&parent), "the server outlived the kill");
        assert!(ends(&child), "the server's own process outlived the server");
    }

    /// The server ended abruptly and alone, the way Task Manager ends a
    /// process, and the job's handle then closed, as the system closes it
    /// when the app ends.
    #[test]
    fn the_process_the_server_started_ends_when_the_job_handle_closes() {
        let Started {
            mut server,
            parent,
            child,
        } = start_server();

        server.child().kill().expect("end the stand-in server");
        assert!(ends(&parent), "the server outlived its own end");
        assert!(
            is_running(&child),
            "the child ended with the server, so the job's handle closing ends nothing"
        );

        drop(server);

        assert!(ends(&child), "the server's own process outlived the job");
    }
}
