//! HTTP client + verified file downloads.

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Clone, Debug)]
pub enum Checksum {
    Sha1(String),
    Sha256(String),
}

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        // PaperMC's API asks for an identifying user agent.
        .user_agent(concat!(
            "mcsd/",
            env!("CARGO_PKG_VERSION"),
            " (self-hosted Minecraft server manager)"
        ))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()?)
}

enum Hasher {
    None,
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
}

impl Hasher {
    fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        match self {
            Hasher::None => {}
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
        }
    }
    fn finish(self) -> Option<String> {
        use sha2::Digest;
        match self {
            Hasher::None => None,
            Hasher::Sha1(h) => Some(hex::encode(h.finalize())),
            Hasher::Sha256(h) => Some(hex::encode(h.finalize())),
        }
    }
}

/// Stream `url` into `dest` (via a `.part` file), verifying the checksum if
/// one is given. `progress` receives human readable updates every ~10%.
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    checksum: Option<&Checksum>,
    progress: &(dyn Fn(String) + Send + Sync),
) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;
    let total = resp.content_length();
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part)
        .await
        .with_context(|| format!("creating {}", part.display()))?;
    let mut hasher = match checksum {
        None => Hasher::None,
        Some(Checksum::Sha1(_)) => Hasher::Sha1(Default::default()),
        Some(Checksum::Sha256(_)) => Hasher::Sha256(Default::default()),
    };

    let mut stream = resp.bytes_stream();
    let mut done: u64 = 0;
    let mut next_report = 10;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("downloading {url}"))?;
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if let Some(total) = total.filter(|t| *t > 0) {
            let pct = done * 100 / total;
            if pct >= next_report {
                progress(format!("{pct}% of {:.1} MB", total as f64 / 1_048_576.0));
                next_report = pct - pct % 10 + 10;
            }
        }
    }
    file.flush().await?;
    drop(file);

    if let (Some(expected), Some(actual)) = (checksum, hasher.finish()) {
        let expected = match expected {
            Checksum::Sha1(s) | Checksum::Sha256(s) => s,
        };
        if !expected.eq_ignore_ascii_case(&actual) {
            let _ = tokio::fs::remove_file(&part).await;
            bail!("checksum mismatch for {url}: expected {expected}, got {actual}");
        }
    }
    tokio::fs::rename(&part, dest)
        .await
        .with_context(|| format!("moving download to {}", dest.display()))?;
    Ok(())
}
