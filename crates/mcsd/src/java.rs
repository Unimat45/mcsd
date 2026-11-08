//! Java runtime discovery and on-demand installation (Eclipse Temurin via the
//! Adoptium API). Minecraft needs different Java versions over time:
//! <=1.16 -> 8, 1.17 -> 16, 1.18-1.20.4 -> 17, 1.20.5-1.21.x -> 21, 26.x -> 25.

use crate::download;
use crate::paths::Paths;
use crate::procutil;
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Serialize, Clone, Debug)]
pub struct JavaRuntime {
    pub path: PathBuf,
    pub major: u32,
    pub version: String,
    /// Installed by mcsd under `<data>/java`.
    pub managed: bool,
}

pub struct Java {
    paths: Paths,
    client: reqwest::Client,
    cache: Mutex<Option<(Instant, Vec<JavaRuntime>)>>,
    install_lock: Mutex<()>,
}

const JAVA_EXE: &str = if cfg!(windows) { "java.exe" } else { "java" };

/// Map a required feature version onto the runtime we would download:
/// non-LTS requirements (e.g. 16) are satisfied by the next LTS.
pub fn preferred_major(required: u32) -> u32 {
    match required {
        0..=8 => 8,
        9..=11 => 11,
        12..=17 => 17,
        n => 17 + (n - 17).div_ceil(4) * 4,
    }
}

impl Java {
    pub fn new(paths: Paths, client: reqwest::Client) -> Self {
        Self {
            paths,
            client,
            cache: Mutex::new(None),
            install_lock: Mutex::new(()),
        }
    }

    /// All runtimes found on this machine (cached for 5 minutes).
    pub async fn list(&self, refresh: bool) -> Vec<JavaRuntime> {
        let mut cache = self.cache.lock().await;
        if !refresh
            && let Some((at, list)) = cache.as_ref()
            && at.elapsed() < Duration::from_secs(300)
        {
            return list.clone();
        }
        let list = self.detect().await;
        *cache = Some((Instant::now(), list.clone()));
        list
    }

    /// Best installed runtime for `required` (exact or LTS match), managed first.
    pub async fn find(&self, required: u32) -> Option<JavaRuntime> {
        let pref = preferred_major(required);
        let list = self.list(false).await;
        let mut matches: Vec<_> = list
            .into_iter()
            .filter(|r| r.major == required || r.major == pref)
            .collect();
        matches.sort_by_key(|r| (!r.managed, r.major != required));
        matches.into_iter().next()
    }

    /// Path to a java binary for `required`, downloading one if needed.
    pub async fn ensure(&self, required: u32, log: &(dyn Fn(String) + Send + Sync)) -> Result<PathBuf> {
        if let Some(rt) = self.find(required).await {
            return Ok(rt.path);
        }
        Ok(self.install(preferred_major(required), log).await?.path)
    }

    pub async fn install(&self, major: u32, log: &(dyn Fn(String) + Send + Sync)) -> Result<JavaRuntime> {
        let _guard = self.install_lock.lock().await;
        let target = self.paths.java_dir().join(format!("temurin-{major}"));
        if let Some(bin) = find_java_bin(&target, 5) {
            // Another request installed it while we waited for the lock.
            if let Some((m, v)) = probe(&bin).await {
                return Ok(JavaRuntime { path: bin, major: m, version: v, managed: true });
            }
        }

        let os = adoptium_os()?;
        let arch = adoptium_arch()?;
        let mut attempts = vec![("jre", arch), ("jdk", arch)];
        if os == "mac" && arch == "aarch64" {
            // Old Java versions (8) have no Apple Silicon build: use Rosetta.
            attempts.extend([("jre", "x64"), ("jdk", "x64")]);
        }

        let ext = if os == "windows" { "zip" } else { "tar.gz" };
        let archive = self.paths.cache_dir().join(format!("temurin-{major}.{ext}"));
        let mut last_err = None;
        let mut fetched = false;
        for (image, arch) in attempts {
            let url = format!(
                "https://api.adoptium.net/v3/binary/latest/{major}/ga/{os}/{arch}/{image}/hotspot/normal/eclipse"
            );
            log(format!("Downloading Java {major} ({image}, {os}/{arch}) from Adoptium..."));
            match download::download(&self.client, &url, &archive, None, &|p| log(format!("Java {major}: {p}"))).await {
                Ok(()) => {
                    fetched = true;
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        if !fetched {
            return Err(last_err
                .unwrap_or_else(|| anyhow!("no download candidates"))
                .context(format!("could not download Java {major} for {os}/{arch}")));
        }

        log(format!("Extracting Java {major}..."));
        let staging = self.paths.java_dir().join(format!(".staging-temurin-{major}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let (archive2, staging2) = (archive.clone(), staging.clone());
        tokio::task::spawn_blocking(move || extract(&archive2, &staging2))
            .await?
            .context("extracting Java archive")?;
        let _ = std::fs::remove_file(&archive);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&staging, &target)?;

        let bin = find_java_bin(&target, 5).ok_or_else(|| anyhow!("no java binary in downloaded archive"))?;
        let (m, v) = probe(&bin).await.ok_or_else(|| anyhow!("downloaded java at {} does not run", bin.display()))?;
        log(format!("Installed Java {v} at {}", bin.display()));
        *self.cache.lock().await = None;
        Ok(JavaRuntime { path: bin, major: m, version: v, managed: true })
    }

    async fn detect(&self) -> Vec<JavaRuntime> {
        let mut candidates: Vec<(PathBuf, bool)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.paths.java_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.starts_with('.')
                    && let Some(bin) = find_java_bin(&e.path(), 5)
                {
                    candidates.push((bin, true));
                }
            }
        }
        if let Some(home) = std::env::var_os("JAVA_HOME") {
            candidates.push((PathBuf::from(home).join("bin").join(JAVA_EXE), false));
        }
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                candidates.push((dir.join(JAVA_EXE), false));
            }
        }
        for root in system_java_roots() {
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    let p = e.path();
                    for bin in [
                        p.join("bin").join(JAVA_EXE),
                        p.join("Contents").join("Home").join("bin").join(JAVA_EXE),
                    ] {
                        candidates.push((bin, false));
                    }
                }
            }
        }

        let mut seen = HashSet::new();
        let unique: Vec<(PathBuf, bool)> = candidates
            .into_iter()
            .filter(|(p, _)| p.is_file())
            .filter(|(p, _)| seen.insert(p.canonicalize().unwrap_or_else(|_| p.clone())))
            .collect();

        let probes = unique.into_iter().map(|(path, managed)| async move {
            probe(&path).await.map(|(major, version)| JavaRuntime { path, major, version, managed })
        });
        let mut found: Vec<JavaRuntime> = futures_util::future::join_all(probes).await.into_iter().flatten().collect();
        found.sort_by_key(|r| (std::cmp::Reverse(r.major), !r.managed));
        found
    }
}

/// Run `java -version` and parse the major version.
pub async fn probe(bin: &Path) -> Option<(u32, String)> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-version").stdin(std::process::Stdio::null()).kill_on_drop(true);
    procutil::configure_helper(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(15), cmd.output()).await.ok()?.ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    parse_version(&text)
}

pub fn parse_version(text: &str) -> Option<(u32, String)> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"version "([^"]+)""#).unwrap());
    let version = RE.captures(text)?.get(1)?.as_str().to_string();
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    let major = if first == 1 { parts.next()?.parse().ok()? } else { first };
    Some((major, version))
}

fn find_java_bin(dir: &Path, depth: usize) -> Option<PathBuf> {
    let direct = dir.join("bin").join(JAVA_EXE);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subdirs.sort();
    subdirs.into_iter().find_map(|d| find_java_bin(&d, depth - 1))
}

fn system_java_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(var) {
                let pf = PathBuf::from(pf);
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Eclipse Foundation",
                    "Microsoft",
                    "Zulu",
                    "BellSoft",
                    "Amazon Corretto",
                    "AdoptOpenJDK",
                ] {
                    roots.push(pf.join(vendor));
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push("/Library/Java/JavaVirtualMachines".into());
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(&home).join("Library/Java/JavaVirtualMachines"));
            roots.push(PathBuf::from(&home).join(".sdkman/candidates/java"));
        }
        roots.push("/opt/homebrew/opt".into());
        roots.push("/usr/local/opt".into());
    } else {
        roots.push("/usr/lib/jvm".into());
        roots.push("/usr/java".into());
        roots.push("/opt/java".into());
        roots.push("/usr/local/lib/jvm".into());
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".sdkman/candidates/java"));
        }
    }
    roots
}

fn adoptium_os() -> Result<&'static str> {
    Ok(if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(all(target_os = "linux", target_env = "musl")) {
        "alpine-linux"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        bail!("no automatic Java downloads for this OS; install Java yourself and set java_path")
    })
}

fn adoptium_arch() -> Result<&'static str> {
    Ok(match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "arm" => "arm",
        "x86" => "x32",
        "powerpc64" => "ppc64le",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        other => bail!("no automatic Java downloads for architecture {other}"),
    })
}

fn extract(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    if archive.extension().is_some_and(|e| e == "zip") {
        zip::ZipArchive::new(file)?.extract(dest)?;
    } else {
        tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_versions() {
        assert_eq!(parse_version(r#"java version "1.8.0_392""#), Some((8, "1.8.0_392".into())));
        assert_eq!(
            parse_version("openjdk version \"21.0.2\" 2024-01-16 LTS"),
            Some((21, "21.0.2".into()))
        );
        assert_eq!(parse_version(r#"openjdk version "25" 2025-09-16"#), Some((25, "25".into())));
    }

    #[test]
    fn lts_mapping() {
        assert_eq!(preferred_major(8), 8);
        assert_eq!(preferred_major(16), 17);
        assert_eq!(preferred_major(21), 21);
        assert_eq!(preferred_major(22), 25);
        assert_eq!(preferred_major(25), 25);
    }
}
