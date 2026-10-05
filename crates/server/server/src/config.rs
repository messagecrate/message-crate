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
    /// Directory holding the built website, served at `/`. Default `static`.
    /// A relative path resolves against the directory above the config file's directory.
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
    /// Originals: `data_dir/<account_id>/<assets_dir>`. One directory holds the
    /// account's attachment files from every source, addressed by SHA-256, so
    /// one file imported from two sources is stored once.
    pub fn assets_dir_for_account(&self, account_id: i64) -> PathBuf {
        self.data_dir
            .join(account_id.to_string())
            .join(&self.assets_dir)
    }

    /// Converted media: `data_dir/<account_id>/<assets_converted_dir>`, one
    /// directory for the account's Previews from every source.
    pub fn assets_converted_dir_for_account(&self, account_id: i64) -> PathBuf {
        self.data_dir
            .join(account_id.to_string())
            .join(&self.assets_converted_dir)
    }
}

impl Config {
    /// Read and parse a TOML config file. A relative path in it (`[paths] db`,
    /// `[paths] data_dir`, `[server] static_dir`) resolves against
    /// [`config_root`], the directory above the config file's directory: the
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
    /// same directory `[paths] db` resolves against, so the flag and the key
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

    /// The config for a Message Crate kept whole in one directory, with no config
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
    /// `--static-dir` resolves against `root`, the directory the config's own
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

/// The directory a relative path resolves against when the config file at
/// `path` is read: the directory above the config file's directory, or the config
/// file's own directory when nothing is above it.
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
mod tests;
