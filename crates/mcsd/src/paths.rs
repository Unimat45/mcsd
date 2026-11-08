//! On-disk layout of the mcsd data directory.
//!
//! ```text
//! <data>/config.toml          daemon settings + API token
//! <data>/servers.json         server registry
//! <data>/daemon.lock          single-instance lock
//! <data>/servers/<name>/      minecraft server files (jar, world, ...)
//! <data>/logs/daemon/         daemon logs (daily rotation)
//! <data>/logs/servers/<name>/ captured console output (size rotation)
//! <data>/java/                runtimes downloaded by mcsd
//! <data>/cache/               scratch space for downloads
//! ```

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn resolve(explicit: Option<PathBuf>) -> Result<Self> {
        let root = match explicit {
            Some(p) => p,
            None => directories::ProjectDirs::from("", "", "mcsd")
                .context("could not determine a home directory; pass --data-dir")?
                .data_dir()
                .to_path_buf(),
        };
        // Absolute paths matter: the service managers launch us from another cwd.
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()?.join(root)
        };
        // Normalise separators ("C:/x\y" -> "C:\x\y" on Windows).
        let root: PathBuf = root.components().collect();
        Ok(Self { root })
    }

    pub fn ensure(&self) -> Result<()> {
        for d in [
            self.root.clone(),
            self.servers_dir(),
            self.root.join("logs").join("servers"),
            self.daemon_log_dir(),
            self.java_dir(),
            self.cache_dir(),
        ] {
            std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
        }
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }
    pub fn registry_file(&self) -> PathBuf {
        self.root.join("servers.json")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.root.join("daemon.lock")
    }
    pub fn servers_dir(&self) -> PathBuf {
        self.root.join("servers")
    }
    pub fn server_dir(&self, name: &str) -> PathBuf {
        self.servers_dir().join(name)
    }
    pub fn console_log_dir(&self, name: &str) -> PathBuf {
        self.root.join("logs").join("servers").join(name)
    }
    pub fn daemon_log_dir(&self) -> PathBuf {
        self.root.join("logs").join("daemon")
    }
    pub fn java_dir(&self) -> PathBuf {
        self.root.join("java")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
}

/// Write a file atomically (temp file + rename) so a crash never leaves a
/// half-written registry/config behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}
