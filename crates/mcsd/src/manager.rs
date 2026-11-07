//! Owns all instances: registry persistence, creation/removal, boot and
//! graceful shutdown, resource sampling.

use crate::instance::{Ctx, Instance, Status, set_property};
use crate::paths::Paths;
use crate::procutil;
use crate::registry::{self, ServerConfig, ServerKind};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;

pub const DEFAULT_PORT: u16 = 25565;
pub const DEFAULT_MEMORY_MB: u32 = 2048;

#[derive(Deserialize, Debug, Default)]
pub struct CreateRequest {
    pub name: String,
    pub kind: Option<ServerKind>,
    /// Defaults to the latest release for the kind.
    pub version: Option<String>,
    pub memory_mb: Option<u32>,
    pub port: Option<u16>,
    #[serde(default)]
    pub accept_eula: bool,
    #[serde(default)]
    pub autostart: bool,
    /// Start the server as soon as the install finishes.
    #[serde(default)]
    pub start: bool,
    pub java_major: Option<u32>,
    pub java_path: Option<String>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    /// Custom servers: path (on the daemon's machine) of a jar to copy in.
    pub jar_source: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct UpdateRequest {
    pub version: Option<String>,
    pub memory_mb: Option<u32>,
    pub port: Option<u16>,
    pub autostart: Option<bool>,
    pub auto_restart: Option<bool>,
    pub stop_timeout_secs: Option<u64>,
    pub jvm_args: Option<Vec<String>>,
    /// Empty string clears the override.
    pub java_path: Option<String>,
    pub java_major: Option<u32>,
}

pub struct Manager {
    pub ctx: Arc<Ctx>,
    instances: RwLock<BTreeMap<String, Arc<Instance>>>,
    save_lock: Mutex<()>,
}

impl Manager {
    pub async fn load(paths: Paths, http: reqwest::Client) -> Result<Arc<Self>> {
        let (persist_tx, mut persist_rx) = mpsc::unbounded_channel();
        let ctx = Arc::new(Ctx::new(paths, http, persist_tx));
        let mut map = BTreeMap::new();
        for cfg in registry::load(&ctx.paths)? {
            map.insert(cfg.name.clone(), Instance::new(ctx.clone(), cfg));
        }
        let mgr = Arc::new(Self {
            ctx,
            instances: RwLock::new(map),
            save_lock: Mutex::new(()),
        });

        // Debounced registry writer.
        let weak = Arc::downgrade(&mgr);
        tokio::spawn(async move {
            while persist_rx.recv().await.is_some() {
                tokio::time::sleep(Duration::from_millis(200)).await;
                while persist_rx.try_recv().is_ok() {}
                let Some(m) = weak.upgrade() else { break };
                if let Err(e) = m.persist() {
                    tracing::error!("saving registry: {e:#}");
                }
            }
        });
        Ok(mgr)
    }

    pub fn persist(&self) -> Result<()> {
        let _g = self.save_lock.lock().unwrap();
        let cfgs: Vec<ServerConfig> = self.list().iter().map(|i| i.config()).collect();
        registry::save(&self.ctx.paths, &cfgs)
    }

    pub fn list(&self) -> Vec<Arc<Instance>> {
        self.instances.read().unwrap().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<Instance>> {
        self.instances.read().unwrap().get(name).cloned()
    }

    /// Clean up after a previous crash, then start servers that were running
    /// at shutdown (or are marked autostart).
    pub async fn boot(self: &Arc<Self>) {
        for inst in self.list() {
            let cfg = inst.config();
            if let Some(pid) = cfg.last_pid {
                procutil::reap_orphan(pid, &inst.dir()).await;
                inst.update_config(|c| c.last_pid = None);
            }
            if cfg.was_running || cfg.autostart {
                tracing::info!(server = %inst.name, "starting on boot");
                if let Err(e) = inst.start().await {
                    tracing::error!(server = %inst.name, "boot start failed: {e:#}");
                }
            }
        }
    }

    /// Stop every server gracefully (in parallel), remembering which ones
    /// were running so the next boot restarts them.
    pub async fn shutdown(&self) {
        let running: Vec<_> = self
            .list()
            .into_iter()
            .filter(|i| i.status().is_active() || i.status() == Status::Crashed)
            .collect();
        if !running.is_empty() {
            tracing::info!("stopping {} server(s)...", running.len());
        }
        futures_util::future::join_all(running.iter().map(|i| i.stop_for_shutdown())).await;
        if let Err(e) = self.persist() {
            tracing::error!("saving registry: {e:#}");
        }
    }

    pub async fn create(self: &Arc<Self>, req: CreateRequest) -> Result<Arc<Instance>> {
        registry::validate_name(&req.name)?;
        if self.get(&req.name).is_some() {
            bail!("a server named '{}' already exists", req.name);
        }
        if !req.accept_eula {
            bail!("you must accept the Minecraft EULA (https://aka.ms/MinecraftEULA)");
        }
        let kind = req.kind.unwrap_or(ServerKind::Paper);
        let version = match req.version.as_deref().map(str::trim) {
            Some(v) if !v.is_empty() => v.to_string(),
            _ if kind == ServerKind::Custom => "custom".into(),
            _ => self
                .ctx
                .providers
                .versions(kind)
                .await?
                .into_iter()
                .next()
                .context("no versions available")?,
        };
        if kind != ServerKind::Custom {
            // Fail fast on typos instead of erroring later in the background install.
            self.ctx.providers.resolve(kind, &version).await?;
        }
        let jar_source = req.jar_source.filter(|s| !s.trim().is_empty()).map(PathBuf::from);
        if let Some(src) = &jar_source
            && !src.is_file()
        {
            bail!("jar not found: {}", src.display());
        }
        let port = match req.port {
            Some(p) => p,
            None => self.free_port(),
        };
        self.check_port(port, &req.name)?;
        let memory_mb = req.memory_mb.unwrap_or(DEFAULT_MEMORY_MB);
        if memory_mb < 256 {
            bail!("memory must be at least 256 MB");
        }

        let cfg = ServerConfig {
            name: req.name.clone(),
            kind,
            version,
            memory_mb,
            port,
            java_major: req.java_major,
            java_path: req.java_path.filter(|s| !s.trim().is_empty()),
            jvm_args: req.jvm_args,
            jar: registry::default_jar(),
            autostart: req.autostart,
            auto_restart: true,
            stop_timeout_secs: registry::default_stop_timeout(),
            installed: false,
            was_running: false,
            last_pid: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        let inst = {
            let mut map = self.instances.write().unwrap();
            if map.contains_key(&cfg.name) {
                bail!("a server named '{}' already exists", cfg.name);
            }
            let inst = Instance::new(self.ctx.clone(), cfg);
            map.insert(inst.name.clone(), inst.clone());
            inst
        };
        self.persist()?;
        inst.sys(format!("Server '{}' created.", inst.name));

        let bg = inst.clone();
        let start = req.start;
        tokio::spawn(async move {
            if bg.install(jar_source).await.is_ok() && start {
                let _ = bg.start().await;
            }
        });
        Ok(inst)
    }

    pub async fn update(&self, name: &str, req: UpdateRequest) -> Result<Arc<Instance>> {
        let inst = self.get(name).context("no such server")?;
        let cfg = inst.config();
        let stopped = matches!(inst.status(), Status::Stopped | Status::Error | Status::Crashed);
        if let Some(m) = req.memory_mb
            && m < 256
        {
            bail!("memory must be at least 256 MB");
        }
        if let Some(port) = req.port {
            self.check_port(port, name)?;
        }
        let version_changed = req.version.as_ref().is_some_and(|v| *v != cfg.version);
        if version_changed {
            if !stopped {
                bail!("stop the server before changing its version");
            }
            if cfg.kind != ServerKind::Custom {
                self.ctx.providers.resolve(cfg.kind, req.version.as_deref().unwrap()).await?;
            }
        }
        if let Some(port) = req.port
            && inst.dir().exists()
        {
            set_property(&inst.dir().join("server.properties"), "server-port", &port.to_string())?;
        }
        inst.update_config(|c| {
            if let Some(v) = req.version {
                if version_changed {
                    c.installed = false; // re-download on next start
                    if c.kind != ServerKind::Custom {
                        c.java_major = None;
                    }
                }
                c.version = v;
            }
            if let Some(v) = req.memory_mb {
                c.memory_mb = v;
            }
            if let Some(v) = req.port {
                c.port = v;
            }
            if let Some(v) = req.autostart {
                c.autostart = v;
            }
            if let Some(v) = req.auto_restart {
                c.auto_restart = v;
            }
            if let Some(v) = req.stop_timeout_secs {
                c.stop_timeout_secs = v.max(5);
            }
            if let Some(v) = req.jvm_args {
                c.jvm_args = v.into_iter().filter(|a| !a.trim().is_empty()).collect();
            }
            if let Some(v) = req.java_path {
                c.java_path = Some(v).filter(|s| !s.trim().is_empty());
            }
            if let Some(v) = req.java_major {
                c.java_major = Some(v);
            }
        });
        if !stopped {
            inst.sys("Settings saved; they apply on the next restart.");
        }
        Ok(inst)
    }

    pub async fn remove(&self, name: &str, delete_files: bool) -> Result<()> {
        let inst = self.get(name).context("no such server")?;
        let status = inst.status();
        if status.is_active() || status == Status::Installing {
            bail!("server '{name}' is {status}; stop it first");
        }
        let _ = inst.stop().await; // cancels a pending crash-restart
        self.instances.write().unwrap().remove(name);
        self.persist()?;
        if delete_files {
            let dirs = [self.ctx.paths.server_dir(name), self.ctx.paths.console_log_dir(name)];
            tokio::task::spawn_blocking(move || {
                for d in dirs {
                    if d.exists() {
                        std::fs::remove_dir_all(&d).with_context(|| format!("deleting {}", d.display()))?;
                    }
                }
                anyhow::Ok(())
            })
            .await??;
        }
        Ok(())
    }

    fn free_port(&self) -> u16 {
        let used: Vec<u16> = self.list().iter().map(|i| i.config().port).collect();
        (DEFAULT_PORT..u16::MAX).find(|p| !used.contains(p)).unwrap_or(DEFAULT_PORT)
    }

    fn check_port(&self, port: u16, owner: &str) -> Result<()> {
        if port < 1024 {
            bail!("port {port} is privileged; use 1024-65535");
        }
        if let Some(other) = self.list().iter().find(|i| i.name != owner && i.config().port == port) {
            bail!("port {port} is already used by server '{}'", other.name);
        }
        Ok(())
    }

    /// Sample CPU/memory of running servers every 2 seconds.
    pub fn spawn_stats(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
            let Ok(mut sys) = System::new() else {
                tracing::warn!("process statistics unavailable on this system");
                return;
            };
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            loop {
                tick.tick().await;
                let Some(mgr) = weak.upgrade() else { break };
                let running: Vec<(Arc<Instance>, Pid)> = mgr
                    .list()
                    .into_iter()
                    .filter_map(|i| i.pid().map(|p| (i, Pid::from_u32(p))))
                    .collect();
                if running.is_empty() {
                    continue;
                }
                let pids: Vec<Pid> = running.iter().map(|(_, p)| *p).collect();
                sys.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&pids),
                    true,
                    ProcessRefreshKind::nothing().with_cpu().with_memory(),
                );
                for (inst, pid) in running {
                    if let Some(p) = sys.process(pid) {
                        inst.set_stats(p.cpu_usage(), p.memory());
                    }
                }
            }
        });
    }
}
