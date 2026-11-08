//! Persistent registry of servers (`<data>/servers.json`).
//!
//! A JSON file rather than SQLite: it holds tens of records, stays
//! human-editable, and keeps the build free of C dependencies so the binary
//! cross-compiles cleanly.

use crate::paths::{Paths, write_atomic};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    Vanilla,
    Paper,
    Fabric,
    /// Bring your own jar (Forge, NeoForge, Purpur, ...).
    Custom,
}

impl std::str::FromStr for ServerKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "vanilla" => Self::Vanilla,
            "paper" => Self::Paper,
            "fabric" => Self::Fabric,
            "custom" => Self::Custom,
            other => bail!("unknown server kind '{other}' (vanilla, paper, fabric, custom)"),
        })
    }
}

impl std::fmt::Display for ServerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Vanilla => "vanilla",
            Self::Paper => "paper",
            Self::Fabric => "fabric",
            Self::Custom => "custom",
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
    pub name: String,
    pub kind: ServerKind,
    pub version: String,
    pub memory_mb: u32,
    pub port: u16,
    /// Java feature version the server needs (resolved at install time).
    #[serde(default)]
    pub java_major: Option<u32>,
    /// Explicit java binary; overrides automatic runtime selection.
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default = "default_jar")]
    pub jar: String,
    /// Start when the daemon starts.
    #[serde(default)]
    pub autostart: bool,
    /// Restart (with backoff) when the process crashes.
    #[serde(default = "yes")]
    pub auto_restart: bool,
    /// Seconds to wait after sending `stop` before killing the process.
    #[serde(default = "default_stop_timeout")]
    pub stop_timeout_secs: u64,
    #[serde(default)]
    pub installed: bool,
    /// Was running when the daemon last shut down -> restarted on boot.
    #[serde(default)]
    pub was_running: bool,
    /// PID of the last process, used to clean up orphans after a daemon crash.
    #[serde(default)]
    pub last_pid: Option<u32>,
    pub created_at: i64,
}

pub fn default_jar() -> String {
    "server.jar".into()
}
fn yes() -> bool {
    true
}
pub fn default_stop_timeout() -> u64 {
    60
}

pub fn load(paths: &Paths) -> Result<Vec<ServerConfig>> {
    let file = paths.registry_file();
    if !file.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))
}

pub fn save(paths: &Paths, servers: &[ServerConfig]) -> Result<()> {
    write_atomic(&paths.registry_file(), &serde_json::to_vec_pretty(servers)?)
}

/// Names double as directory names, so keep them portable.
pub fn validate_name(name: &str) -> Result<()> {
    let ok_chars = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if name.is_empty() || name.len() > 32 || !ok_chars || name.starts_with(['-', '_']) {
        bail!("invalid name '{name}': use 1-32 chars of a-z, 0-9, '-' or '_'");
    }
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if RESERVED.contains(&name) {
        bail!("'{name}' is a reserved file name on Windows");
    }
    Ok(())
}
