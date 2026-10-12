use super::*;
use media::MediaTools as _;

/// A config file at `<dir>/config/server.toml` holding `text`.
fn config_file(dir: &Path, text: &str) -> PathBuf {
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("server.toml");
    fs::write(&path, text).unwrap();
    path
}

/// A `[paths] db` naming `db`, an absolute path. `/srv/...` is not
/// absolute on Windows, which wants a drive, so the tests use the temporary
/// directory's own path. A TOML literal string (single quotes) keeps a
/// Windows path's backslashes as they are; in double quotes they would be
/// escapes.
fn db_config(db: &Path) -> String {
    format!("[paths]\ndb = '{}'\n", db.display())
}

#[test]
fn without_db_the_database_is_the_configured_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("srv/messagecrate.db");
    let path = config_file(dir.path(), &db_config(&db));

    let cfg = Config::load_with_db(&path, None).unwrap();

    assert_eq!(cfg.paths.db, db);
}

#[test]
fn db_replaces_the_configured_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("srv/messagecrate.db");
    let path = config_file(dir.path(), &db_config(&db));
    let other = dir.path().join("elsewhere/other.db");

    let cfg = Config::load_with_db(&path, Some(other.clone())).unwrap();

    assert_eq!(cfg.paths.db, other);
}

/// S7-12: `--db data/messagecrate.db` names the file `[paths] db =
/// "data/messagecrate.db"` names, wherever the command runs, rather than
/// a file under the working directory.
#[test]
fn a_relative_db_resolves_where_the_config_key_does() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_file(dir.path(), "[paths]\ndb = \"data/messagecrate.db\"\n");

    let from_key = Config::load(&path).unwrap();
    let from_flag =
        Config::load_with_db(&path, Some(PathBuf::from("data/messagecrate.db"))).unwrap();

    assert_eq!(from_flag.paths.db, from_key.paths.db);
    assert_eq!(from_flag.paths.db, dir.path().join("data/messagecrate.db"));
}

#[test]
fn validate_source_id_accepts_slugs() {
    assert!(validate_source_id("imessage").is_ok());
    assert!(validate_source_id("go-sms-pro").is_ok());
    assert!(validate_source_id("sms_backup_plus").is_ok());
    assert!(validate_source_id("a1").is_ok());
}

#[test]
fn validate_source_id_rejects_bad() {
    assert!(validate_source_id("").is_err());
    assert!(validate_source_id("iMessage").is_err());
    assert!(validate_source_id("../x").is_err());
    assert!(validate_source_id("-bad").is_err());
    assert!(validate_source_id("has space").is_err());
    assert!(validate_source_id(" imessage").is_err());
    assert!(validate_source_id("imessage\n").is_err());
}

#[test]
fn relative_paths_resolve_against_the_directory_above_the_config_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_file(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\ndata_dir = \"data\"\n\n\
         [server]\nstatic_dir = \"site\"\n",
    );

    let cfg = Config::load(&path).unwrap();

    assert_eq!(cfg.paths.db, dir.path().join("data/messagecrate.db"));
    assert_eq!(cfg.paths.data_dir, dir.path().join("data"));
    assert_eq!(
        cfg.require_server().unwrap().static_dir,
        dir.path().join("site")
    );
}

/// The defaults `docs/developer/reference/config-and-accounts.md` states.
#[test]
fn a_config_with_no_settings_loads_the_documented_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("config.toml");
    fs::write(
        &path,
        "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\n",
    )
    .unwrap();

    let cfg = Config::load(&path).unwrap();

    assert_eq!(cfg.paths.data_dir, dir.path().join("data"));
    assert_eq!(
        cfg.paths.assets_dir_for_account(7),
        dir.path().join("data/7/assets")
    );
    assert_eq!(
        cfg.paths.assets_converted_dir_for_account(7),
        dir.path().join("data/7/assets_converted")
    );
    let server = cfg.require_server().unwrap();
    assert_eq!(server.bind, "127.0.0.1:8080");
    assert_eq!(server.asset_part_size, 16_777_216);
    assert_eq!(server.upload_stall_secs, 60);
    assert_eq!(server.upload_piece_secs, 300);
    assert!(!server.openapi_ui);
    assert!(server.cors_origins.is_empty());
    assert_eq!(server.transcode_stall_secs, 120);
}

/// `[server] transcode_stall_secs` sets how long a conversion may go
/// without progress before ffmpeg is stopped; a file with no `[server]`
/// section, and a server started with `--data-dir`, use the default
/// (#2898).
#[test]
fn transcode_stall_secs_sets_the_no_progress_wait() {
    let cfg = load_text(
        "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\ntranscode_stall_secs = 600\n",
    )
    .unwrap();
    assert_eq!(
        cfg.transcode_stall_wait(),
        std::time::Duration::from_secs(600)
    );

    let no_server = load_text("[paths]\ndb = \"data/messagecrate.db\"\n").unwrap();
    assert_eq!(no_server.transcode_stall_wait(), media::DEFAULT_STALL_WAIT);
    let data_dir = Config::for_data_dir(Path::new("data"));
    assert_eq!(data_dir.transcode_stall_wait(), media::DEFAULT_STALL_WAIT);
}

/// The tools the server runs are built from its configuration: the Tools
/// Directory `serve --tools-dir` names and `transcode_stall_secs`, with no
/// state anywhere else in the process (#2696).
#[test]
fn the_media_tools_come_from_the_configuration() {
    let mut cfg =
        load_text("[paths]\ndb = \"data/messagecrate.db\"\n[server]\ntranscode_stall_secs = 600\n")
            .unwrap();
    let tools = cfg.media_tools();
    assert_eq!(tools.tools_dir(), None);
    assert_eq!(tools.stall_wait(), std::time::Duration::from_secs(600));

    cfg.server.as_mut().unwrap().tools_dir = Some(PathBuf::from("/app/tools"));
    assert_eq!(cfg.media_tools().tools_dir(), Some(Path::new("/app/tools")));
}

/// A wait of zero would stop every conversion at once, so it is refused,
/// naming the key.
#[test]
fn a_transcode_stall_secs_of_zero_is_refused_naming_the_key() {
    let err =
        load_text("[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\ntranscode_stall_secs = 0\n")
            .unwrap_err();
    let text = format!("{err:#}");

    assert!(text.contains("[server] transcode_stall_secs"), "{text}");
    assert!(text.contains("greater than 0"), "{text}");
}

/// Write `text` as `config/config.toml` under a fresh directory and load it.
fn load_text(text: &str) -> Result<Config> {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("config.toml");
    fs::write(&path, text).unwrap();
    Config::load(&path)
}

/// The limit left the config file. A line that still sets it is refused,
/// and the refusal says where the limit is set now, because a line that
/// loaded and did nothing would leave the operator believing it held.
#[test]
fn a_config_that_still_sets_asset_max_bytes_is_refused_and_told_where_the_limit_lives() {
    let err = load_text(
        "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nasset_max_bytes = 536870912\n",
    )
    .unwrap_err();
    let text = format!("{err:#}");

    assert!(text.contains("config.toml"), "{text}");
    assert!(text.contains("[server] asset_max_bytes"), "{text}");
    assert!(text.contains("Server Settings screen"), "{text}");
}

/// The upload time limits the file sets are the ones the server holds.
#[test]
fn a_config_sets_the_upload_time_limits() {
    let cfg = load_text(
        "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nupload_stall_secs = 7\nupload_piece_secs = 45\n",
    )
    .unwrap();

    let server = cfg.require_server().unwrap();
    assert_eq!(server.upload_stall_secs, 7);
    assert_eq!(server.upload_piece_secs, 45);
}

/// A limit of 0 would refuse every upload at once, so the server refuses
/// the file, naming the key.
#[test]
fn an_upload_time_limit_of_zero_is_refused_naming_the_key() {
    for key in ["upload_stall_secs", "upload_piece_secs"] {
        let cfg = load_text(&format!(
            "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\n{key} = 0\n"
        ))
        .unwrap();

        let text = format!("{:#}", cfg.require_server().unwrap_err());

        assert!(text.contains(&format!("[server] {key}")), "{text}");
        assert!(text.contains("greater than 0"), "{text}");
    }
}

/// A key the server does not use is refused wherever it sits, by its name
/// and its section: a misspelt key otherwise loads as the default.
#[test]
fn a_config_with_an_unknown_key_is_refused_naming_the_key_and_its_section() {
    for (section, config) in [
        (
            "[paths]",
            "[paths]\ndb = \"data/messagecrate.db\"\nasset_dir = \"assets\"\n",
        ),
        (
            "[server]",
            "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nasset_dir = 1\n",
        ),
    ] {
        let text = format!("{:#}", load_text(config).unwrap_err());
        assert!(text.contains("`asset_dir`"), "{section}: {text}");
        assert!(text.contains(section), "{section}: {text}");
    }
}

/// The Tools Directory comes only from `serve --tools-dir`, which the
/// desktop app passes. A server started by hand finds ffmpeg on `PATH` or
/// does without, so the config file has no key for it (#1053).
#[test]
fn a_config_cannot_name_a_tools_directory() {
    let text = format!(
        "{:#}",
        load_text("[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\ntools_dir = \"tools\"\n")
            .unwrap_err()
    );
    assert!(text.contains("`tools_dir`"), "{text}");
}

/// Every unknown key is named, not only the first.
#[test]
fn every_unknown_key_in_a_section_is_named() {
    let text = format!(
        "{:#}",
        load_text("[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nbnd = \"x\"\nport = 1\n")
            .unwrap_err()
    );
    assert!(text.contains("`bnd`") && text.contains("`port`"), "{text}");
}

/// A section the server does not have, or a key outside any section.
#[test]
fn a_config_with_an_unknown_section_is_refused_naming_it() {
    let text = format!(
        "{:#}",
        load_text("[paths]\ndb = \"data/messagecrate.db\"\n\n[sever]\nbind = \"x\"\n").unwrap_err()
    );
    assert!(text.contains("`sever`"), "{text}");
    assert!(text.contains("[paths] and [server]"), "{text}");
}

/// `[database]` named the connection URL while the server ran on Postgres
/// too. It is an unknown section now, so a config that still has it is
/// refused by name and never read as if the URL were honoured.
#[test]
fn a_config_with_the_removed_database_section_is_refused_naming_it() {
    let text = format!(
        "{:#}",
        load_text(
            "[paths]\ndb = \"data/messagecrate.db\"\n\n[database]\nurl = \"sqlite://x.db\"\n"
        )
        .unwrap_err()
    );
    assert!(text.contains("`database`"), "{text}");
}

/// `assets_dir` and `assets_converted_dir` are joined onto each
/// account's directory, so anything but one plain directory name is
/// refused by its key: an absolute path would put every account's
/// attachments in one directory, and a separator or `..` would reach
/// outside the account's own. A name starting with `.` is refused, since
/// the server's own `.removing` sits beside the attachments, and so is one
/// ending in `.` or a space, which Windows drops. A plain name loads.
#[test]
fn an_assets_directory_that_is_not_one_plain_name_is_refused_naming_its_key() {
    for key in ["assets_dir", "assets_converted_dir"] {
        for value in [
            "/srv/mc/assets",
            "media/assets",
            "media\\\\assets",
            "..",
            ".",
            "../assets",
            "",
            ".removing",
            ".incoming",
            "C:assets",
            "assets.",
            "assets ",
        ] {
            let config = format!("[paths]\ndb = \"data/messagecrate.db\"\n{key} = \"{value}\"\n");
            let Err(err) = load_text(&config) else {
                panic!("{key} = {value:?} loaded");
            };
            let text = format!("{err:#}");
            assert!(
                text.contains(&format!("[paths] {key}")),
                "{key} = {value:?}: {text}"
            );
        }
    }

    // A plain name other than the default loads, and the account's
    // directories are that name under the account's own directory.
    let cfg = load_text(
        "[paths]\ndb = \"data/messagecrate.db\"\nassets_dir = \"originals\"\nassets_converted_dir = \"previews.v2\"\n",
    )
    .unwrap();
    assert!(
        cfg.paths
            .assets_dir_for_account(7)
            .ends_with("data/7/originals")
    );
    assert!(
        cfg.paths
            .assets_converted_dir_for_account(7)
            .ends_with("data/7/previews.v2")
    );
}

/// Originals and Previews are cleaned up under different rules, so one
/// directory for both is refused, in any letter case, because macOS and
/// Windows ignore it by default.
#[test]
fn one_name_for_both_asset_directories_is_refused() {
    for converted in ["media", "Media"] {
        let text = format!(
            "{:#}",
            load_text(&format!(
                "[paths]\ndb = \"data/messagecrate.db\"\nassets_dir = \"media\"\nassets_converted_dir = \"{converted}\"\n"
            ))
            .unwrap_err()
        );
        assert!(
            text.contains("assets_dir and assets_converted_dir are both"),
            "{converted}: {text}"
        );
    }
}

/// The config files the repository ships must load under the same rule:
/// the example a developer copies and the one the Docker image starts from.
#[test]
fn every_committed_config_file_loads() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for file in ["config/config.toml.example", "config/config.docker.toml"] {
        let text = fs::read_to_string(repo.join(file)).unwrap();
        if let Err(err) = load_text(&text) {
            panic!("{file} does not load: {err:#}");
        }
    }
}

const PACKAGED_ORIGINS: &[&str] = &[
    "https://tauri.localhost",
    "http://tauri.localhost",
    "tauri://localhost",
];

/// `scripts/run-dev.sh` only uncomments the `# cors_origins =` line.
/// That line must be a complete array or the config it writes on a first
/// run is invalid TOML.
#[test]
fn example_cors_origins_uncomments_to_a_complete_array() {
    let example = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../config/config.toml.example"
    ));
    let cors_lines: Vec<&str> = example
        .lines()
        .filter(|line| line.starts_with("# cors_origins =") || line.starts_with("cors_origins ="))
        .collect();
    assert_eq!(
        cors_lines.len(),
        1,
        "run-dev.sh uncomments one cors_origins line"
    );
    assert!(
        cors_lines[0].contains('[') && cors_lines[0].contains(']'),
        "cors_origins must stay on one line so sed yields a closed array, got {}",
        cors_lines[0]
    );

    let uncommented: String = example
        .lines()
        .map(|line| {
            line.strip_prefix("# cors_origins =")
                .map(|rest| format!("cors_origins ={rest}"))
                .unwrap_or_else(|| line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cfg: Config = Config::parse(&uncommented).expect("example after run-dev.sh sed must parse");
    let origins = &cfg
        .server
        .as_ref()
        .expect("[server] in example")
        .cors_origins;
    for origin in [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        PACKAGED_ORIGINS[0],
        PACKAGED_ORIGINS[1],
        PACKAGED_ORIGINS[2],
    ] {
        assert!(
            origins.iter().any(|item| item == origin),
            "missing {origin} in {origins:?}"
        );
    }
}

/// The Docker image's config names no CORS origin. The comment in
/// `config/config.docker.toml` says why (#2144).
#[test]
fn docker_config_names_no_cors_origins() {
    let docker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../config/config.docker.toml"
    ));
    let cfg = Config::parse(docker).expect("config.docker.toml must parse");
    let origins = &cfg
        .server
        .as_ref()
        .expect("[server] in docker config")
        .cors_origins;
    assert!(origins.is_empty(), "config.docker.toml names {origins:?}");
}

/// The config a file in `dir` with `text` loads to, for the layout checks.
fn load_in(dir: &Path, text: &str) -> Config {
    Config::load(&config_file(dir, text)).unwrap()
}

/// The text of [`Config::require_static_dir_apart`]'s refusal, or a panic
/// when it accepts.
fn refusal(cfg: &Config) -> String {
    match cfg.require_static_dir_apart() {
        Ok(()) => panic!("the layout was accepted"),
        Err(err) => format!("{err:#}"),
    }
}

/// #2175: everything under the website directory is served without a login,
/// so a Data Directory inside it would publish the database.
#[test]
fn a_data_directory_inside_the_website_directory_is_refused_naming_both() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static")).unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"elsewhere/messagecrate.db\"\ndata_dir = \"static/data\"\n[server]\n",
    );

    let text = refusal(&cfg);

    assert!(
        text.contains(&cfg.paths.data_dir.display().to_string()),
        "{text}"
    );
    let static_dir = &cfg.server.as_ref().unwrap().static_dir;
    assert!(text.contains(&static_dir.display().to_string()), "{text}");
}

#[test]
fn a_database_inside_the_website_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"static/messagecrate.db\"\n[server]\n",
    );

    assert!(refusal(&cfg).contains("messagecrate.db"));
}

#[test]
fn a_website_directory_inside_the_data_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\nstatic_dir = \"data/static\"\n",
    );

    refusal(&cfg);
}

#[test]
fn one_directory_for_the_website_and_the_data_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"site/messagecrate.db\"\ndata_dir = \"site\"\n[server]\nstatic_dir = \"site\"\n",
    );

    refusal(&cfg);
}

/// `static/../static/data` is `static/data`, though the text does not start
/// with `static`.
#[test]
fn a_parent_step_cannot_hide_the_nesting() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static")).unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"messagecrate.db\"\ndata_dir = \"other/../static/data\"\n[server]\n",
    );

    refusal(&cfg);
}

#[cfg(unix)]
#[test]
fn a_symlink_cannot_hide_the_nesting() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static/data")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("static/data"), dir.path().join("data")).unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\n",
    );

    refusal(&cfg);
}

/// `website` only shares its first letters with `web`: it is beside it, not
/// inside it.
#[test]
fn a_sibling_whose_name_starts_with_the_other_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"website/messagecrate.db\"\ndata_dir = \"website\"\n[server]\nstatic_dir = \"web\"\n",
    );

    cfg.require_static_dir_apart().unwrap();
}

/// The default layout, the Docker image's and the repository's: `static`
/// and `data` side by side.
#[test]
fn the_default_layout_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\n",
    );

    cfg.require_static_dir_apart().unwrap();
}

/// The desktop app's layout: `serve --data-dir` with the website shipped in
/// the app's own resources.
#[test]
fn the_desktop_app_layout_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::for_data_dir(&dir.path().join("app-data/data")).with_serve_overrides(
        dir.path(),
        ServeFlags {
            static_dir: Some(dir.path().join("resources/static")),
            ..ServeFlags::default()
        },
    );

    cfg.require_static_dir_apart().unwrap();
}

/// A link whose target is not there yet still decides where SQLite creates
/// the database, so it is followed like any other.
#[cfg(unix)]
#[test]
fn a_dangling_symlink_cannot_hide_the_nesting() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static")).unwrap();
    fs::create_dir_all(dir.path().join("data")).unwrap();
    std::os::unix::fs::symlink(
        "../static/messagecrate.db",
        dir.path().join("data/messagecrate.db"),
    )
    .unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\n",
    );

    refusal(&cfg);
}

/// When a link hides the nesting, the refusal says where the path really
/// is, or the two paths it names would not look nested.
#[cfg(unix)]
#[test]
fn the_refusal_names_where_a_link_leads() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static/data")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("static/data"), dir.path().join("data")).unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\n",
    );

    let real = dir.path().canonicalize().unwrap().join("static/data");
    let text = refusal(&cfg);
    assert!(text.contains(&real.display().to_string()), "{text}");
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_is_refused_rather_than_followed_for_ever() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("loop", dir.path().join("loop")).unwrap();
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"loop/messagecrate.db\"\ndata_dir = \"loop\"\n[server]\n",
    );

    let text = refusal(&cfg);
    assert!(
        text.contains(&format!("more than {MAX_SYMLINKS} symlinks")),
        "{text}"
    );
}

/// What `cmd /C <line>` prints, after asserting it succeeded. `line` is
/// passed as written, not quoted again, so a quoted path in it reaches
/// `cmd` as one argument. `line` must not start with a quote, or `cmd`
/// strips the first and last quote of it.
#[cfg(windows)]
fn cmd(line: &str) -> String {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("cmd")
        .raw_arg(format!("/C {line}"))
        .output()
        .unwrap();
    assert!(output.status.success(), "cmd /C {line}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// Makes `link` a directory junction to `target`. A junction needs no
/// privilege, where a directory symlink on Windows does.
#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    cmd(&format!(
        "mklink /J \"{}\" \"{}\"",
        link.display(),
        target.display()
    ));
}

/// A junction's target comes back verbatim (`\\?\C:\...`), unlike every
/// other path, so the check canonicalizes before it compares.
#[cfg(windows)]
#[test]
fn a_junction_cannot_hide_the_nesting_and_the_refusal_names_where_it_leads() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static/data")).unwrap();
    junction(&dir.path().join("data"), &dir.path().join("static\\data"));
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\n",
    );

    let real = dir.path().canonicalize().unwrap().join("static\\data");
    let text = refusal(&cfg);
    assert!(text.contains(&real.display().to_string()), "{text}");
}

/// A junction to a directory not made yet still decides where the database
/// is created, so it is followed like any other.
#[cfg(windows)]
#[test]
fn a_dangling_junction_cannot_hide_the_nesting() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("static")).unwrap();
    junction(&dir.path().join("data"), &dir.path().join("static\\data"));
    let cfg = load_in(
        dir.path(),
        "[paths]\ndb = \"data/messagecrate.db\"\n[server]\n",
    );

    refusal(&cfg);
}

/// `path` with every component as its 8.3 short name, as `cmd` reports it.
#[cfg(windows)]
fn short_name(path: &Path) -> PathBuf {
    let printed = cmd(&format!("for %I in (\"{}\") do @echo %~sI", path.display()));
    PathBuf::from(printed.trim())
}

/// `WEBSIT~1` is another name for `websitedirectory`, so a Data Directory
/// named through the long name is inside a website directory named through
/// the short one.
#[cfg(windows)]
#[test]
fn a_short_name_cannot_hide_the_nesting() {
    let dir = tempfile::tempdir().unwrap();
    let website = dir.path().join("websitedirectory");
    fs::create_dir_all(website.join("data")).unwrap();
    let short = short_name(&website);
    assert_ne!(
        short.file_name(),
        website.file_name(),
        "8.3 short names are off on the volume holding {}",
        website.display()
    );
    let cfg = load_in(
        dir.path(),
        &format!(
            "[paths]\ndb = \"elsewhere/messagecrate.db\"\ndata_dir = \"websitedirectory/data\"\n\
             [server]\nstatic_dir = '{}'\n",
            short.display()
        ),
    );

    refusal(&cfg);
}
