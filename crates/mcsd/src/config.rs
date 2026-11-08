use crate::paths::{Paths, write_atomic};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_BIND: &str = "127.0.0.1:8765";

/// Daemon settings, stored in `<data>/config.toml`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DaemonConfig {
    /// Address the HTTP API + dashboard listen on. Keep it on loopback unless
    /// you put a TLS reverse proxy in front of it.
    #[serde(default = "default_bind")]
    pub bind: String,
    /// Bearer token required by every API call (CLI and dashboard).
    pub token: String,
}

fn default_bind() -> String {
    DEFAULT_BIND.to_string()
}

impl DaemonConfig {
    pub fn load_or_init(paths: &Paths) -> Result<Self> {
        let file = paths.config_file();
        if file.exists() {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            return toml::from_str(&text).with_context(|| format!("parsing {}", file.display()));
        }
        std::fs::create_dir_all(&paths.root)?;
        let cfg = DaemonConfig {
            bind: default_bind(),
            token: random_token(),
        };
        let body = format!(
            "# mcsd daemon configuration\n\
             # bind: address of the API/dashboard. Exposing it beyond 127.0.0.1 gives\n\
             #       remote console access to anyone holding the token; use a TLS proxy.\n\
             {}",
            toml::to_string_pretty(&cfg)?
        );
        write_atomic(&file, body.as_bytes())?;
        restrict_permissions(&file);
        Ok(cfg)
    }

    /// URL a local client should use to reach the daemon.
    pub fn local_url(&self) -> String {
        let bind = self.bind.replace("0.0.0.0", "127.0.0.1").replace("[::]", "[::1]");
        format!("http://{bind}")
    }
}

fn random_token() -> String {
    let bytes: [u8; 24] = rand::random();
    hex::encode(bytes)
}

#[cfg(unix)]
fn restrict_permissions(file: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn restrict_permissions(_file: &std::path::Path) {
    // %APPDATA% is already per-user on Windows.
}
