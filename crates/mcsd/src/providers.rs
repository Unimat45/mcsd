//! Where server jars come from: Mojang (vanilla), PaperMC Fill v3, Fabric meta.

use crate::download::Checksum;
use crate::registry::ServerKind;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MOJANG_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const PAPER_API: &str = "https://fill.papermc.io/v3/projects/paper";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const CACHE_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct Resolved {
    pub url: String,
    pub checksum: Option<Checksum>,
    pub java_major: u32,
    /// Human description of what was resolved (e.g. "paper 1.21.4 build 232").
    pub label: String,
}

pub struct Providers {
    client: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

impl Providers {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        if let Some((at, v)) = self.cache.lock().await.get(url)
            && at.elapsed() < CACHE_TTL
        {
            return Ok(v.clone());
        }
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("GET {url}: HTTP {status} {}", body.chars().take(200).collect::<String>());
        }
        let v: Value = resp.json().await.with_context(|| format!("decoding {url}"))?;
        self.cache.lock().await.insert(url.to_string(), (Instant::now(), v.clone()));
        Ok(v)
    }

    /// Available release versions, newest first.
    pub async fn versions(&self, kind: ServerKind) -> Result<Vec<String>> {
        match kind {
            ServerKind::Vanilla => {
                let m = self.get_json(MOJANG_MANIFEST).await?;
                Ok(arr(&m["versions"])
                    .iter()
                    .filter(|v| v["type"] == "release")
                    .filter_map(|v| v["id"].as_str().map(String::from))
                    .collect())
            }
            ServerKind::Paper => {
                let p = self.get_json(PAPER_API).await?;
                let mut out: Vec<String> = p["versions"]
                    .as_object()
                    .ok_or_else(|| anyhow!("unexpected PaperMC response"))?
                    .values()
                    .flat_map(|group| arr(group).iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                    .filter(|v| !v.contains('-')) // drop -pre / -rc builds
                    .collect();
                out.sort_by_key(|v| std::cmp::Reverse(version_key(v)));
                Ok(out)
            }
            ServerKind::Fabric => {
                let g = self.get_json(&format!("{FABRIC_META}/versions/game")).await?;
                Ok(arr(&g)
                    .iter()
                    .filter(|v| v["stable"] == true)
                    .filter_map(|v| v["version"].as_str().map(String::from))
                    .collect())
            }
            ServerKind::Custom => Ok(vec![]),
        }
    }

    async fn mojang_version(&self, version: &str) -> Result<Value> {
        let m = self.get_json(MOJANG_MANIFEST).await?;
        let url = arr(&m["versions"])
            .iter()
            .find(|v| v["id"] == version)
            .and_then(|v| v["url"].as_str())
            .ok_or_else(|| anyhow!("unknown Minecraft version '{version}'"))?
            .to_string();
        self.get_json(&url).await
    }

    /// Java feature version Mojang declares for a Minecraft version.
    pub async fn mojang_java_major(&self, version: &str) -> Result<u32> {
        let v = self.mojang_version(version).await?;
        // Very old manifests have no javaVersion: they ran on Java 8.
        Ok(v["javaVersion"]["majorVersion"].as_u64().unwrap_or(8) as u32)
    }

    pub async fn resolve(&self, kind: ServerKind, version: &str) -> Result<Resolved> {
        match kind {
            ServerKind::Vanilla => {
                let v = self.mojang_version(version).await?;
                let server = &v["downloads"]["server"];
                let url = server["url"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Minecraft {version} has no dedicated server download"))?;
                Ok(Resolved {
                    url: url.into(),
                    checksum: server["sha1"].as_str().map(|s| Checksum::Sha1(s.into())),
                    java_major: v["javaVersion"]["majorVersion"].as_u64().unwrap_or(8) as u32,
                    label: format!("vanilla {version}"),
                })
            }
            ServerKind::Paper => {
                let build = self
                    .get_json(&format!("{PAPER_API}/versions/{version}/builds/latest"))
                    .await
                    .with_context(|| format!("no Paper build for {version}"))?;
                let dl = &build["downloads"]["server:default"];
                let url = dl["url"].as_str().ok_or_else(|| anyhow!("Paper build has no server download"))?;
                let java_major = match self.get_json(&format!("{PAPER_API}/versions/{version}")).await {
                    Ok(info) => match info["version"]["java"]["version"]["minimum"].as_u64() {
                        Some(m) => m as u32,
                        None => self.mojang_java_major(version).await?,
                    },
                    Err(_) => self.mojang_java_major(version).await?,
                };
                Ok(Resolved {
                    url: url.into(),
                    checksum: dl["checksums"]["sha256"].as_str().map(|s| Checksum::Sha256(s.into())),
                    java_major,
                    label: format!("paper {version} build {}", build["id"]),
                })
            }
            ServerKind::Fabric => {
                let loaders = self.get_json(&format!("{FABRIC_META}/versions/loader")).await?;
                let installers = self.get_json(&format!("{FABRIC_META}/versions/installer")).await?;
                let first_stable = |v: &Value| {
                    arr(v)
                        .iter()
                        .find(|x| x["stable"] == true)
                        .and_then(|x| x["version"].as_str())
                        .map(String::from)
                };
                let loader = first_stable(&loaders).ok_or_else(|| anyhow!("no stable Fabric loader"))?;
                let installer = first_stable(&installers).ok_or_else(|| anyhow!("no stable Fabric installer"))?;
                Ok(Resolved {
                    url: format!("{FABRIC_META}/versions/loader/{version}/{loader}/{installer}/server/jar"),
                    checksum: None,
                    java_major: self.mojang_java_major(version).await?,
                    label: format!("fabric {version} (loader {loader})"),
                })
            }
            ServerKind::Custom => bail!("custom servers bring their own jar"),
        }
    }
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// "1.21.10" -> [1, 21, 10]; sorts numerically instead of lexically.
pub fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-']).map(|p| p.parse().unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::version_key;

    #[test]
    fn numeric_version_sort() {
        let mut v = vec!["1.21.9", "26.1", "1.21.10", "1.8.8"];
        v.sort_by_key(|s| std::cmp::Reverse(version_key(s)));
        assert_eq!(v, ["26.1", "1.21.10", "1.21.9", "1.8.8"]);
    }
}
