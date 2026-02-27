use anyhow::Context;
use serde::Deserialize;

/// Application configuration, loaded from environment variables / .env / config file.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Bind address for the HTTP server.
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Path to the SQLite database file.
    #[serde(default = "default_db_url")]
    pub database_url: String,

    /// Directory where downloaded files are stored.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,

    /// Maximum number of concurrent downloads.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Maximum number of times to retry a failed download (0 = no retries).
    /// Each retry waits 2^n seconds (2s, 4s, 8s, …).
    #[serde(default = "default_max_download_retries")]
    pub max_download_retries: u32,

    /// Path to the get_iplayer binary.
    #[serde(default = "default_get_iplayer_path")]
    pub get_iplayer_path: String,

    /// Path to the ffmpeg binary.
    #[serde(default = "default_ffmpeg_path")]
    pub ffmpeg_path: String,

    /// Directory used by get_iplayer for its programme cache (--profile-dir).
    /// Lives under /data so it persists across container restarts.
    #[serde(default = "default_iplayer_cache_dir")]
    pub iplayer_cache_dir: String,

    /// Secret used for generating auth tokens / salts.
    #[serde(default = "default_secret")]
    pub secret: String,

    /// Optional HTTP proxy to pass to get_iplayer.
    #[serde(default)]
    pub proxy: Option<String>,

    /// Optional initial admin username (only used on first launch).
    #[serde(default = "default_admin_user")]
    pub admin_username: String,

    /// Optional initial admin password (only used on first launch).
    #[serde(default = "default_admin_pass")]
    pub admin_password: String,
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_string()
}
fn default_db_url() -> String {
    "/data/tapedeck.db".to_string()
}
fn default_output_dir() -> String {
    "/downloads".to_string()
}
fn default_max_concurrent() -> usize {
    2
}
fn default_max_download_retries() -> u32 {
    3
}
fn default_get_iplayer_path() -> String {
    "get_iplayer".to_string()
}
fn default_ffmpeg_path() -> String {
    "ffmpeg".to_string()
}
fn default_iplayer_cache_dir() -> String {
    "/data/iplayer-cache".to_string()
}
fn default_secret() -> String {
    "change-me-in-production".to_string()
}
fn default_admin_user() -> String {
    "admin".to_string()
}
fn default_admin_pass() -> String {
    "changeme".to_string()
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        // Load .env if present (ignore errors — it may not exist)
        let _ = dotenvy::dotenv();

        envy::from_env::<AppConfig>().context("Failed to load config from environment")
    }
}
