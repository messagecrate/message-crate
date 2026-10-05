use super::*;

/// A config file at `<dir>/config/server.toml` holding `text`.
fn config_file(dir: &Path, text: &str) -> PathBuf {
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("server.toml");
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn without_db_the_database_is_the_configured_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_file(dir.path(), "[paths]\ndb = \"/srv/messagecrate.db\"\n");

    let cfg = Config::load_with_db(&path, None).unwrap();

    assert_eq!(cfg.paths.db, PathBuf::from("/srv/messagecrate.db"));
}

#[test]
fn db_replaces_the_configured_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_file(dir.path(), "[paths]\ndb = \"/srv/messagecrate.db\"\n");

    let cfg = Config::load_with_db(&path, Some(PathBuf::from("/elsewhere/other.db"))).unwrap();

    assert_eq!(cfg.paths.db, PathBuf::from("/elsewhere/other.db"));
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
    assert_eq!(server.asset_part_size, 67_108_864);
    assert!(!server.openapi_ui);
    assert!(server.cors_origins.is_empty());
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

#[test]
fn docker_config_includes_packaged_desktop_origins() {
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
    for origin in PACKAGED_ORIGINS {
        assert!(
            origins.iter().any(|item| item == origin),
            "missing {origin} in {origins:?}"
        );
    }
}
