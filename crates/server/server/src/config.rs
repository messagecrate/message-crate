//! Config file model ([`Config`]) plus path/source validation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Complete server configuration, loaded from a TOML file. It is read only
/// through [`Config::load`], which refuses a key the server does not use.
#[derive(Debug, Clone)]
pub struct Config {
    /// Filesystem locations (database, per-account data).
    pub paths: PathsConfig,
    /// HTTP ingest server (`message-crate-server serve`). Required for `serve`.
    pub server: Option<ServerConfig>,
}

/// Where the keys the config file takes are listed, for a refusal to point at.
const CONFIG_REFERENCE: &str =
    "https://messagecrate.app/docs/developer/reference/config-and-accounts/";

/// One section as the file has it: the keys the server uses, and every other
/// key the section carries. The second map is what lets [`Config::load`]
/// refuse an unknown key by its own name and its section's.
#[derive(Debug, Deserialize)]
struct Section<T> {
    #[serde(flatten)]
    known: T,
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

/// The config file as it is read, before unknown keys are refused.
#[derive(Debug, Deserialize)]
struct ConfigFile {
    paths: Section<PathsConfig>,
    #[serde(default)]
    server: Option<Section<ServerConfig>>,
    /// Sections the server does not have, and keys outside any section.
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

/// Unknown keys as a refusal lists them: `` `a`, `b` ``.
fn key_list(unknown: &BTreeMap<String, toml::Value>) -> String {
    unknown
        .keys()
        .map(|key| format!("`{key}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Take a section's known keys, refusing it when it carries any other.
///
/// A key the server does not use is an error, never ignored: a misspelt key
/// would load as its default, and a key that was removed would sit in the
/// file looking as though it still held.
fn known_keys<T>(name: &str, section: Section<T>) -> Result<T> {
    if name == "server" && section.unknown.contains_key("asset_max_bytes") {
        bail!(
            "[server] asset_max_bytes is not a config key. The attachment size limit is a \
             Server Setting: the owner changes it on the Server Settings screen. Remove the line."
        );
    }
    if !section.unknown.is_empty() {
        bail!(
            "[{name}] has a key the server does not use: {}. Remove it, or correct its name; \
             the keys the config file takes are listed at {CONFIG_REFERENCE}",
            key_list(&section.unknown)
        );
    }
    Ok(section.known)
}

/// Refuse a `[paths]` value that is not one plain directory name.
///
/// `assets_dir` and `assets_converted_dir` are joined onto each account's
/// directory. An absolute path, or on Windows one with a drive (`C:assets`),
/// would replace that directory, so every account's attachments would share
/// one; a separator or `..` would reach outside it; an empty name or `.`
/// would be the account's directory itself. A name starting with `.` is
/// refused too, because the server keeps `.removing` beside the attachments
/// and deletes what is in it. A name ending in `.` or a space is refused,
/// because Windows drops those, so `assets.` would be `assets` there.
fn require_directory_name(key: &str, value: &str) -> Result<()> {
    let mut components = Path::new(value).components();
    let one_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    let plain = one_name
        && !value.starts_with('.')
        && !value.ends_with(['.', ' '])
        && !value.contains(['/', '\\', ':']);
    if !plain {
        bail!(
            "[paths] {key} = {value:?} is not a directory name. It names one directory inside \
             each account's directory, such as \"assets\": not empty, with no separator, no \
             `:`, not starting with `.`, and not ending in `.` or a space"
        );
    }
    Ok(())
}

impl ConfigFile {
    /// The configuration the file states, or the refusal of what it should not hold.
    fn into_config(self) -> Result<Config> {
        if !self.unknown.is_empty() {
            bail!(
                "{} is not a section or key the server uses. The sections are [paths] \
                 and [server]; their keys are listed at {CONFIG_REFERENCE}",
                key_list(&self.unknown)
            );
        }
        let paths = known_keys("paths", self.paths)?;
        require_directory_name("assets_dir", &paths.assets_dir)?;
        require_directory_name("assets_converted_dir", &paths.assets_converted_dir)?;
        if paths.assets_dir.to_lowercase() == paths.assets_converted_dir.to_lowercase() {
            // Originals and Previews are swept under different rules, and a
            // Preview not yet recorded would be swept as an unnamed original.
            // macOS and Windows ignore letter case by default, so `media` and
            // `Media` are one directory there.
            bail!(
                "[paths] assets_dir and assets_converted_dir are both {:?}. Originals and \
                 Previews need a directory each; give them different names",
                paths.assets_dir
            );
        }
        Ok(Config {
            paths,
            server: self
                .server
                .map(|section| known_keys("server", section))
                .transpose()?,
        })
    }
}

/// `[server]` section: HTTP bind address, CORS, and asset upload limits.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Bind address (default `127.0.0.1:8080`).
    #[serde(default = "default_server_bind")]
    pub bind: String,
    /// Largest multipart part, in bytes. Default 64 MiB (under Cloudflare
    /// Free/Pro ~100 MB). The part size a client is told is this or the
    /// attachment size limit, whichever is smaller. The limit is a Server
    /// Setting the owner changes in the app and has no key in this file.
    #[serde(default = "default_asset_part_size")]
    pub asset_part_size: usize,
    /// Cross-Origin Resource Sharing (CORS) origins allowed to call this API,
    /// on top of the packaged desktop app's own origins, which are always
    /// allowed. CORS is the browser rule that decides which other websites may
    /// call this API. Empty is the right setting for a server serving its own
    /// website, since that UI is same-origin and needs no header at all.
    /// Use `["*"]` only for local debugging. Example: `["https://app.example.com"]`.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Serve Swagger UI at `/docs` and the spec at `/openapi.json`. Default false.
    #[serde(default = "default_openapi_ui")]
    pub openapi_ui: bool,
    /// Folder holding the built website, served at `/`. Default `static`.
    /// A relative path resolves against the folder above the config file's folder.
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
}

impl Default for ServerConfig {
    /// The `[server]` section with every key left out.
    fn default() -> Self {
        Self {
            bind: default_server_bind(),
            asset_part_size: default_asset_part_size(),
            cors_origins: Vec::new(),
            openapi_ui: default_openapi_ui(),
            static_dir: default_static_dir(),
        }
    }
}

/// serde default for `[server] static_dir`.
fn default_static_dir() -> PathBuf {
    PathBuf::from("static")
}

/// serde default for `[server] bind`.
fn default_server_bind() -> String {
    "127.0.0.1:8080".to_string()
}

/// serde default for `[server] asset_part_size` (64 MiB).
fn default_asset_part_size() -> usize {
    64 * 1024 * 1024
}

/// serde default for `[server] openapi_ui`.
fn default_openapi_ui() -> bool {
    false
}

/// `[paths]` section: database file and per-account data directories.
#[derive(Debug, Clone, Deserialize)]
pub struct PathsConfig {
    /// SQLite database file path.
    pub db: PathBuf,
    /// Root for per-account data (`data/<account_id>/…`).
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Directory name for an account's originals, one per account (default
    /// `assets`).
    #[serde(default = "default_assets_dir_name")]
    pub assets_dir: String,
    /// Directory name for an account's converted media, one per account.
    #[serde(default = "default_assets_converted_dir_name")]
    pub assets_converted_dir: String,
}

/// serde default for `[paths] data_dir`.
fn default_data_dir() -> PathBuf {
    PathBuf::from("data")
}

/// serde default for `[paths] assets_dir`.
fn default_assets_dir_name() -> String {
    "assets".to_string()
}

/// serde default for `[paths] assets_converted_dir`.
fn default_assets_converted_dir_name() -> String {
    "assets_converted".to_string()
}

/// Safe source slug for path segments and `messages.source` values.
///
/// # Errors
///
/// Returns an error when the id is empty, too long, or uses disallowed characters.
pub fn validate_source_id(source: &str) -> Result<()> {
    let s = source.trim();
    if s.is_empty() {
        bail!("source id must not be empty");
    }
    if s.len() > 64 {
        bail!("source id must be at most 64 characters");
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        bail!("source id '{s}' must use only lowercase letters, digits, hyphens, and underscores");
    }
    if s.starts_with('-') || s.starts_with('_') {
        bail!("source id must not start with '-' or '_'");
    }
    Ok(())
}

impl PathsConfig {
    /// Originals: `data_dir/<account_id>/<assets_dir>`. One folder holds the
    /// account's attachment files from every source, addressed by SHA-256, so
    /// one file imported from two sources is stored once.
    pub fn assets_dir_for_account(&self, account_id: i64) -> PathBuf {
        self.data_dir
            .join(account_id.to_string())
            .join(&self.assets_dir)
    }

    /// Converted media: `data_dir/<account_id>/<assets_converted_dir>`, one
    /// folder for the account's Previews from every source.
    pub fn assets_converted_dir_for_account(&self, account_id: i64) -> PathBuf {
        self.data_dir
            .join(account_id.to_string())
            .join(&self.assets_converted_dir)
    }
}

impl Config {
    /// Read and parse a TOML config file. A relative path in it (`[paths] db`,
    /// `[paths] data_dir`, `[server] static_dir`) resolves against
    /// [`config_root`], the folder above the config file's folder: the
    /// repository root for `config/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, or carries a
    /// section or key the server does not use; the error names each one.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config =
            Self::parse(&text).with_context(|| format!("config {} was refused", path.display()))?;

        let root = config_root(path)?;
        config.paths.db = resolve_path(&root, &config.paths.db);
        config.paths.data_dir = resolve_path(&root, &config.paths.data_dir);
        if let Some(server) = config.server.as_mut() {
            server.static_dir = resolve_path(&root, &server.static_dir);
        }

        Ok(config)
    }

    /// [`Config::load`] with the command line's `--db` applied over
    /// `[paths] db`. A relative `--db` resolves against [`config_root`], the
    /// same folder `[paths] db` resolves against, so the flag and the key
    /// name the same file whatever directory the command runs in. After this
    /// the config alone says where the database is.
    ///
    /// # Errors
    ///
    /// Returns an error when [`Config::load`] does.
    pub(crate) fn load_with_db(path: &Path, db: Option<PathBuf>) -> Result<Self> {
        let mut config = Self::load(path)?;
        if let Some(db) = db {
            config.paths.db = resolve_path(&config_root(path)?, &db);
        }
        Ok(config)
    }

    /// The config for a Message Crate kept whole in one folder, with no config
    /// file: the database is `messagecrate.db` in `data_dir`, the accounts'
    /// files sit beside it, and every server setting has its default. This is
    /// what `serve --data-dir` runs on, and how the desktop app starts the
    /// server without writing a file a person would have to find.
    pub fn for_data_dir(data_dir: &Path) -> Self {
        Self {
            paths: PathsConfig {
                db: data_dir.join("messagecrate.db"),
                data_dir: data_dir.to_path_buf(),
                assets_dir: default_assets_dir_name(),
                assets_converted_dir: default_assets_converted_dir_name(),
            },
            server: Some(ServerConfig::default()),
        }
    }

    /// Apply `serve`'s own flags: `--bind` replaces `[server] bind`,
    /// `--static-dir` replaces `[server] static_dir`, and each
    /// `--cors-origin` is added to `[server] cors_origins`. A relative
    /// `--static-dir` resolves against `root`, the folder the config's own
    /// paths resolve against. A config with no `[server]` section is left
    /// without one, for `require_server` to refuse.
    pub(crate) fn with_serve_overrides(
        mut self,
        root: &Path,
        bind: Option<String>,
        static_dir: Option<PathBuf>,
        cors_origins: Vec<String>,
    ) -> Self {
        if let Some(server) = self.server.as_mut() {
            if let Some(bind) = bind {
                server.bind = bind;
            }
            if let Some(static_dir) = static_dir {
                server.static_dir = resolve_path(root, &static_dir);
            }
            server.cors_origins.extend(cors_origins);
        }
        self
    }

    /// The configuration a config file's text states, with paths as written.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not the TOML the server expects, or
    /// carries a section or key the server does not use.
    fn parse(text: &str) -> Result<Self> {
        let file: ConfigFile = toml::from_str(text)?;
        file.into_config()
    }

    /// Server settings for `serve`. Fails if `[server]` is missing.
    pub fn require_server(&self) -> Result<&ServerConfig> {
        let server = self
            .server
            .as_ref()
            .context("config missing [server] section (needed for serve)")?;
        if server.asset_part_size == 0 {
            bail!("server.asset_part_size must be > 0");
        }
        Ok(server)
    }
}

/// A configured path made absolute against `base`, unless it already is.
fn resolve_path(base: &Path, configured: &Path) -> PathBuf {
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        base.join(configured)
    }
}

/// The folder a relative path resolves against when the config file at
/// `path` is read: the folder above the config file's folder, or the config
/// file's own folder when nothing is above it.
///
/// # Errors
///
/// Returns an error when `path` is relative and the working directory
/// cannot be read.
pub(crate) fn config_root(path: &Path) -> Result<PathBuf> {
    let abs_config = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to get current directory")?
            .join(path)
    };
    let config_dir = abs_config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = config_dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(config_dir);
    Ok(root.to_path_buf())
}

#[cfg(test)]
mod tests {
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
    }

    #[test]
    fn relative_paths_resolve_against_the_folder_above_the_config_folder() {
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

    /// Write `text` as `config/config.toml` under a fresh folder and load it.
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
            load_text(
                "[paths]\ndb = \"data/messagecrate.db\"\n\n[server]\nbnd = \"x\"\nport = 1\n"
            )
            .unwrap_err()
        );
        assert!(text.contains("`bnd`") && text.contains("`port`"), "{text}");
    }

    /// A section the server does not have, or a key outside any section.
    #[test]
    fn a_config_with_an_unknown_section_is_refused_naming_it() {
        let text = format!(
            "{:#}",
            load_text("[paths]\ndb = \"data/messagecrate.db\"\n\n[sever]\nbind = \"x\"\n")
                .unwrap_err()
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
                let config =
                    format!("[paths]\ndb = \"data/messagecrate.db\"\n{key} = \"{value}\"\n");
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
            .filter(|line| {
                line.starts_with("# cors_origins =") || line.starts_with("cors_origins =")
            })
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
        let cfg: Config =
            Config::parse(&uncommented).expect("example after run-dev.sh sed must parse");
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
}
