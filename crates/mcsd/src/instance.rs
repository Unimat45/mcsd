//! One managed Minecraft server: install, process supervision, console I/O.
//!
//! Lifecycle:
//! ```text
//!   Stopped/Error/Crashed --start--> [Installing] --> Starting --"Done (..)!"--> Running
//!   Running --stop--> Stopping --exit--> Stopped
//!   Running --unexpected exit--> Crashed --backoff--> Starting   (auto_restart)
//! ```
//! A server stops by getting `stop` on stdin, which saves the world. Signals
//! are only a fallback because they don't behave the same on every OS.

use crate::download;
use crate::java::Java;
use crate::logbuf::{self, LogBuffer, LogLine, RotatingFile, Stream};
use crate::parse::{self, Event};
use crate::paths::Paths;
use crate::procutil;
use crate::properties;
use crate::providers::Providers;
use crate::registry::{ServerConfig, ServerKind};
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;

const LOG_BUFFER_LINES: usize = 2000;
const LOG_FILE_MAX_BYTES: u64 = 10 * 1024 * 1024;
const LOG_FILE_KEEP: usize = 5;
const MAX_CRASHES: usize = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(600);
const BACKOFF_SECS: [u64; MAX_CRASHES] = [2, 5, 10, 30, 60];

/// Services shared by every instance.
pub struct Ctx {
    pub paths: Paths,
    pub http: reqwest::Client,
    pub providers: Providers,
    pub java: Java,
    persist_tx: mpsc::UnboundedSender<()>,
}

impl Ctx {
    pub fn new(paths: Paths, http: reqwest::Client, persist_tx: mpsc::UnboundedSender<()>) -> Self {
        Self {
            providers: Providers::new(http.clone()),
            java: Java::new(paths.clone(), http.clone()),
            paths,
            http,
            persist_tx,
        }
    }
    /// Ask the manager to write the registry (debounced).
    pub fn request_persist(&self) {
        let _ = self.persist_tx.send(());
    }
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Stopped,
    Installing,
    Starting,
    Running,
    Stopping,
    Crashed,
    Error,
}

impl Status {
    /// A process exists (or is about to).
    pub fn is_active(self) -> bool {
        matches!(self, Status::Starting | Status::Running | Status::Stopping)
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
        f.write_str(&s)
    }
}

#[derive(Default)]
struct Runtime {
    pid: Option<u32>,
    started_at: Option<i64>,
    players: BTreeSet<String>,
    desired_running: bool,
    last_error: Option<String>,
    last_exit: Option<String>,
    crash_flag: bool,
    restarts: u32,
    crash_times: Vec<Instant>,
    in_backoff: bool,
    cpu: f32,
    mem: u64,
    progress: Option<String>,
    /// Fires the supervisor's kill path (or cuts a restart backoff short).
    kill_tx: Option<oneshot::Sender<()>>,
}

struct LogState {
    buf: LogBuffer,
    file: RotatingFile,
}

struct RunningProc {
    child: Child,
    kill_rx: oneshot::Receiver<()>,
    pumps: Vec<JoinHandle<()>>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ServerInfo {
    #[serde(flatten)]
    pub config: ServerConfig,
    pub status: Status,
    pub pid: Option<u32>,
    pub started_at: Option<i64>,
    pub uptime_secs: Option<i64>,
    pub players: Vec<String>,
    /// Percent of one core (like `top`): can exceed 100.
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub last_exit: Option<String>,
    pub progress: Option<String>,
    pub dir: String,
}

pub struct Instance {
    pub name: String,
    ctx: Arc<Ctx>,
    cfg: RwLock<ServerConfig>,
    status: watch::Sender<Status>,
    rt: Mutex<Runtime>,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    log: Mutex<LogState>,
    log_tx: broadcast::Sender<LogLine>,
    /// Serialises start/stop/restart/install. `kill` deliberately skips it.
    op: tokio::sync::Mutex<()>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Instance {
    pub fn new(ctx: Arc<Ctx>, cfg: ServerConfig) -> Arc<Self> {
        let name = cfg.name.clone();
        let file = RotatingFile::new(
            ctx.paths.console_log_dir(&name).join("console.log"),
            LOG_FILE_MAX_BYTES,
            LOG_FILE_KEEP,
        );
        let (log_tx, _) = broadcast::channel(1024);
        Arc::new(Self {
            name,
            ctx,
            cfg: RwLock::new(cfg),
            status: watch::Sender::new(Status::Stopped),
            rt: Mutex::new(Runtime::default()),
            stdin: tokio::sync::Mutex::new(None),
            log: Mutex::new(LogState {
                buf: LogBuffer::new(LOG_BUFFER_LINES),
                file,
            }),
            log_tx,
            op: tokio::sync::Mutex::new(()),
        })
    }

    // ---------------------------------------------------------------- state

    pub fn config(&self) -> ServerConfig {
        self.cfg.read().unwrap().clone()
    }

    pub fn update_config(&self, f: impl FnOnce(&mut ServerConfig)) {
        f(&mut self.cfg.write().unwrap());
        self.ctx.request_persist();
    }

    pub fn status(&self) -> Status {
        *self.status.borrow()
    }

    fn set_status(&self, s: Status) {
        self.status.send_replace(s);
    }

    pub fn dir(&self) -> PathBuf {
        self.ctx.paths.server_dir(&self.name)
    }

    pub fn pid(&self) -> Option<u32> {
        self.rt.lock().unwrap().pid
    }

    pub fn set_stats(&self, cpu: f32, mem: u64) {
        let mut rt = self.rt.lock().unwrap();
        rt.cpu = cpu;
        rt.mem = mem;
    }

    fn in_backoff(&self) -> bool {
        self.rt.lock().unwrap().in_backoff
    }

    pub fn info(&self) -> ServerInfo {
        let config = self.config();
        let status = self.status();
        let rt = self.rt.lock().unwrap();
        ServerInfo {
            config,
            status,
            pid: rt.pid,
            started_at: rt.started_at,
            uptime_secs: rt.started_at.map(|t| (now_ms() - t) / 1000),
            players: rt.players.iter().cloned().collect(),
            cpu_percent: rt.cpu,
            memory_bytes: rt.mem,
            restarts: rt.restarts,
            last_error: rt.last_error.clone(),
            last_exit: rt.last_exit.clone(),
            progress: rt.progress.clone(),
            dir: self.dir().display().to_string(),
        }
    }

    async fn wait_for(&self, pred: impl Fn(Status) -> bool, timeout: Duration) -> bool {
        let mut rx = self.status.subscribe();
        tokio::time::timeout(timeout, rx.wait_for(|s| pred(*s))).await.is_ok()
    }

    // ------------------------------------------------------------- logging

    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.log_tx.subscribe()
    }

    pub fn tail(&self, n: usize) -> Vec<LogLine> {
        self.log.lock().unwrap().buf.tail(n)
    }

    pub fn logs_after(&self, seq: u64, limit: usize) -> Vec<LogLine> {
        self.log.lock().unwrap().buf.after(seq, limit)
    }

    /// Log a message from mcsd itself into the server's console stream.
    pub fn sys(&self, text: impl Into<String>) {
        self.emit(Stream::Sys, text.into());
    }

    fn emit(&self, stream: Stream, text: String) {
        let line = {
            let mut log = self.log.lock().unwrap();
            let line = log.buf.push(stream, text);
            log.file.write(&line);
            line
        };
        if matches!(stream, Stream::Out | Stream::Err) {
            self.on_console_line(&line.text);
        }
        let _ = self.log_tx.send(line);
    }

    fn on_console_line(&self, text: &str) {
        let Some(event) = parse::parse(text) else { return };
        match event {
            Event::Ready => {
                if self.status() == Status::Starting {
                    self.set_status(Status::Running);
                }
            }
            Event::PlayerJoin(p) => {
                self.rt.lock().unwrap().players.insert(p);
            }
            Event::PlayerLeave(p) => {
                self.rt.lock().unwrap().players.remove(&p);
            }
            Event::Crash => self.rt.lock().unwrap().crash_flag = true,
            Event::BindFailed => {
                let port = self.config().port;
                self.rt.lock().unwrap().last_error = Some(format!("port {port} is already in use"));
            }
        }
    }

    fn set_progress(&self, p: Option<String>) {
        self.rt.lock().unwrap().progress = p;
    }

    fn fail(&self, msg: String) {
        self.rt.lock().unwrap().last_error = Some(msg.clone());
        self.set_status(Status::Error);
        self.sys(msg);
    }

    // ------------------------------------------------------------- install

    /// Download (or re-download) the server jar and prepare the directory.
    pub async fn install(self: &Arc<Self>, jar_source: Option<PathBuf>) -> Result<()> {
        let _op = self.op.lock().await;
        match self.status() {
            Status::Stopped | Status::Crashed | Status::Error => {}
            s => bail!("cannot install while the server is {s}"),
        }
        self.install_inner(jar_source).await
    }

    async fn install_inner(&self, jar_source: Option<PathBuf>) -> Result<()> {
        self.set_status(Status::Installing);
        self.rt.lock().unwrap().last_error = None;
        let res = self.do_install(jar_source).await;
        self.set_progress(None);
        match &res {
            Ok(()) => {
                self.set_status(Status::Stopped);
                self.sys("Install complete.");
            }
            Err(e) => self.fail(format!("Install failed: {e:#}")),
        }
        res
    }

    async fn do_install(&self, jar_source: Option<PathBuf>) -> Result<()> {
        let cfg = self.config();
        let dir = self.dir();
        tokio::fs::create_dir_all(&dir).await?;
        let progress = |m: String| {
            self.set_progress(Some(m.clone()));
            self.sys(m);
        };
        let jar = dir.join(&cfg.jar);

        let java_major = match cfg.kind {
            ServerKind::Custom => {
                if let Some(src) = jar_source {
                    progress(format!("Copying {} -> {}", src.display(), jar.display()));
                    tokio::fs::copy(&src, &jar)
                        .await
                        .with_context(|| format!("copying {}", src.display()))?;
                }
                if !jar.is_file() {
                    bail!("no server jar: put your jar at {}", jar.display());
                }
                match cfg.java_major {
                    Some(m) => m,
                    None => self.ctx.providers.mojang_java_major(&cfg.version).await.unwrap_or(21),
                }
            }
            kind => {
                progress(format!("Resolving {kind} {}...", cfg.version));
                let r = self.ctx.providers.resolve(kind, &cfg.version).await?;
                progress(format!("Downloading {}...", r.label));
                download::download(&self.ctx.http, &r.url, &jar, r.checksum.as_ref(), &progress).await?;
                r.java_major
            }
        };

        // Only reachable after the user accepted the EULA at creation time.
        tokio::fs::write(
            dir.join("eula.txt"),
            "# Accepted via mcsd (https://aka.ms/MinecraftEULA)\neula=true\n",
        )
        .await?;
        properties::set_in_file(&dir.join(properties::FILE_NAME), "server-port", &cfg.port.to_string())?;

        if cfg.java_path.as_deref().is_none_or(|p| p.trim().is_empty()) {
            progress(format!("Checking for Java {java_major}..."));
            let bin = self.ctx.java.ensure(java_major, &progress).await?;
            self.sys(format!("Using Java at {}", bin.display()));
        }
        self.update_config(|c| {
            c.installed = true;
            c.java_major = Some(java_major);
        });
        Ok(())
    }

    // ------------------------------------------------------------ lifecycle

    pub async fn start(self: &Arc<Self>) -> Result<()> {
        let _op = self.op.lock().await;
        self.start_inner().await
    }

    async fn start_inner(self: &Arc<Self>) -> Result<()> {
        match self.status() {
            Status::Stopped | Status::Error => {}
            Status::Crashed if self.in_backoff() => {
                // A restart is already scheduled: just do it now.
                let mut rt = self.rt.lock().unwrap();
                rt.desired_running = true;
                if let Some(tx) = rt.kill_tx.take() {
                    let _ = tx.send(());
                }
                return Ok(());
            }
            Status::Crashed => {}
            s => bail!("server '{}' is {s}", self.name),
        }
        if !self.config().installed {
            self.install_inner(None).await?;
        }
        {
            let mut rt = self.rt.lock().unwrap();
            rt.desired_running = true;
            rt.restarts = 0;
            rt.crash_times.clear();
            rt.last_error = None;
        }
        self.update_config(|c| c.was_running = true);
        match self.spawn_process().await {
            Ok(proc) => {
                tokio::spawn(self.clone().supervise(proc));
                Ok(())
            }
            Err(e) => {
                self.rt.lock().unwrap().desired_running = false;
                self.update_config(|c| c.was_running = false);
                self.fail(format!("Failed to start: {e:#}"));
                Err(e)
            }
        }
    }

    /// Graceful stop requested by a user.
    pub async fn stop(self: &Arc<Self>) -> Result<()> {
        let _op = self.op.lock().await;
        self.stop_inner(false).await
    }

    /// Graceful stop during daemon shutdown: remember to restart it on boot.
    pub async fn stop_for_shutdown(self: &Arc<Self>) {
        let _op = self.op.lock().await;
        if let Err(e) = self.stop_inner(true).await {
            tracing::warn!(server = %self.name, "stop failed: {e:#}");
        }
    }

    pub async fn restart(self: &Arc<Self>) -> Result<()> {
        let _op = self.op.lock().await;
        self.stop_inner(false).await?;
        self.start_inner().await
    }

    /// Hard kill. Skips the op lock so it can interrupt a hanging stop.
    pub async fn kill(self: &Arc<Self>) -> Result<()> {
        {
            let mut rt = self.rt.lock().unwrap();
            rt.desired_running = false;
            match rt.kill_tx.take() {
                Some(tx) => {
                    let _ = tx.send(());
                }
                None => bail!("server '{}' is not running", self.name),
            }
        }
        self.update_config(|c| c.was_running = false);
        self.wait_for(|s| !s.is_active() && s != Status::Crashed, Duration::from_secs(15)).await;
        Ok(())
    }

    async fn stop_inner(&self, remember: bool) -> Result<()> {
        let status = self.status();
        let in_backoff = {
            let mut rt = self.rt.lock().unwrap();
            rt.desired_running = false;
            rt.in_backoff
        };
        if !remember {
            self.update_config(|c| c.was_running = false);
        }
        match status {
            Status::Stopped | Status::Error => Ok(()),
            Status::Installing => bail!("cannot stop while installing"),
            Status::Crashed => {
                if in_backoff {
                    if let Some(tx) = self.rt.lock().unwrap().kill_tx.take() {
                        let _ = tx.send(());
                    }
                    self.wait_for(|s| s == Status::Stopped, Duration::from_secs(10)).await;
                } else {
                    self.set_status(Status::Stopped);
                }
                Ok(())
            }
            Status::Starting | Status::Running | Status::Stopping => {
                let timeout = self.config().stop_timeout_secs.max(5);
                self.set_status(Status::Stopping);
                self.sys("Stopping server (sending \"stop\")...");
                if let Err(e) = self.write_stdin("stop").await {
                    self.sys(format!("Could not send stop command: {e:#}"));
                }
                if !self.wait_for(|s| s == Status::Stopped, Duration::from_secs(timeout)).await {
                    self.sys(format!("Server did not stop within {timeout}s; killing it."));
                    if let Some(tx) = self.rt.lock().unwrap().kill_tx.take() {
                        let _ = tx.send(());
                    }
                    self.wait_for(|s| !s.is_active(), Duration::from_secs(15)).await;
                }
                Ok(())
            }
        }
    }

    pub async fn send_command(&self, command: &str) -> Result<()> {
        let command = command.trim();
        if command.is_empty() {
            bail!("empty command");
        }
        if command.contains(['\n', '\r']) {
            bail!("commands must be a single line");
        }
        let command = command.strip_prefix('/').unwrap_or(command);
        self.write_stdin(command).await?;
        self.emit(Stream::In, format!("> {command}"));
        Ok(())
    }

    async fn write_stdin(&self, line: &str) -> Result<()> {
        let mut guard = self.stdin.lock().await;
        let stdin = guard.as_mut().ok_or_else(|| anyhow!("server '{}' is not running", self.name))?;
        stdin.write_all(format!("{line}\n").as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    // ---------------------------------------------------------- supervisor

    async fn spawn_process(self: &Arc<Self>) -> Result<RunningProc> {
        let cfg = self.config();
        let java = match cfg.java_path.as_deref().map(str::trim) {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => {
                let major = cfg.java_major.unwrap_or(21);
                self.ctx.java.ensure(major, &|m| self.sys(m)).await?
            }
        };
        let dir = self.dir();
        if !dir.join(&cfg.jar).is_file() {
            bail!("server jar {} is missing (reinstall the server)", dir.join(&cfg.jar).display());
        }

        let mut args = vec![
            format!("-Xms{}M", cfg.memory_mb),
            format!("-Xmx{}M", cfg.memory_mb),
            // Pipes otherwise use the platform charset (cp1252 on Windows).
            "-Dfile.encoding=UTF-8".into(),
            "-Dstdout.encoding=UTF-8".into(),
            "-Dstderr.encoding=UTF-8".into(),
            "-Dsun.stdout.encoding=UTF-8".into(),
            "-Dsun.stderr.encoding=UTF-8".into(),
        ];
        args.extend(cfg.jvm_args.iter().cloned());
        args.extend(["-jar".into(), cfg.jar.clone(), "nogui".into()]);

        let mut cmd = Command::new(&java);
        cmd.args(&args)
            .current_dir(&dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false);
        procutil::configure(&mut cmd);

        self.set_status(Status::Starting);
        self.sys(format!("Starting: {} {}", java.display(), args.join(" ")));
        let mut child = cmd
            .spawn()
            .with_context(|| format!("launching {}", java.display()))?;
        procutil::attach(&child);

        let mut pumps = Vec::new();
        if let Some(out) = child.stdout.take() {
            pumps.push(tokio::spawn(pump(self.clone(), out, Stream::Out)));
        }
        if let Some(err) = child.stderr.take() {
            pumps.push(tokio::spawn(pump(self.clone(), err, Stream::Err)));
        }
        *self.stdin.lock().await = child.stdin.take();

        let pid = child.id();
        let (kill_tx, kill_rx) = oneshot::channel();
        {
            let mut rt = self.rt.lock().unwrap();
            rt.pid = pid;
            rt.started_at = Some(now_ms());
            rt.players.clear();
            rt.crash_flag = false;
            rt.kill_tx = Some(kill_tx);
            rt.cpu = 0.0;
            rt.mem = 0;
        }
        self.update_config(|c| c.last_pid = pid);
        Ok(RunningProc { child, kill_rx, pumps })
    }

    async fn supervise(self: Arc<Self>, mut proc: RunningProc) {
        loop {
            let exit = tokio::select! {
                r = proc.child.wait() => r,
                _ = async {
                    // A dropped sender is not a kill request.
                    if (&mut proc.kill_rx).await.is_err() { std::future::pending::<()>().await }
                } => {
                    self.sys("Killing server process.");
                    let _ = proc.child.start_kill();
                    proc.child.wait().await
                }
            };
            // Let the pumps flush the last lines (crash reports!) first.
            for h in proc.pumps.drain(..) {
                let _ = tokio::time::timeout(Duration::from_secs(3), h).await;
            }
            *self.stdin.lock().await = None;

            let exit_desc = match &exit {
                Ok(s) => describe_exit(s),
                Err(e) => format!("wait failed: {e}"),
            };
            let clean = matches!(&exit, Ok(s) if s.success());
            let (desired, crash_flag, uptime_ms) = {
                let mut rt = self.rt.lock().unwrap();
                let uptime = rt.started_at.map(|t| now_ms() - t).unwrap_or(0);
                rt.pid = None;
                rt.started_at = None;
                rt.players.clear();
                rt.cpu = 0.0;
                rt.mem = 0;
                rt.kill_tx = None;
                rt.last_exit = Some(exit_desc.clone());
                (rt.desired_running, rt.crash_flag, uptime)
            };
            self.update_config(|c| c.last_pid = None);

            if !desired {
                self.sys(format!("Server stopped ({exit_desc})."));
                self.set_status(Status::Stopped);
                break;
            }
            if clean && !crash_flag {
                // e.g. someone ran /stop in game: respect it.
                self.sys(format!("Server exited by itself ({exit_desc}); not restarting."));
                self.rt.lock().unwrap().desired_running = false;
                self.update_config(|c| c.was_running = false);
                self.set_status(Status::Stopped);
                break;
            }

            self.sys(format!("Server crashed ({exit_desc})."));
            let auto_restart = self.config().auto_restart;
            let attempt = {
                let mut rt = self.rt.lock().unwrap();
                if uptime_ms > CRASH_WINDOW.as_millis() as i64 {
                    rt.crash_times.clear();
                }
                let now = Instant::now();
                rt.crash_times.retain(|t| now.duration_since(*t) < CRASH_WINDOW);
                rt.crash_times.push(now);
                rt.crash_times.len()
            };
            if !auto_restart || attempt > MAX_CRASHES {
                let msg = if auto_restart {
                    format!("Crashed {attempt} times within 10 minutes; giving up.")
                } else {
                    "Auto-restart is disabled.".to_string()
                };
                {
                    let mut rt = self.rt.lock().unwrap();
                    rt.desired_running = false;
                    rt.last_error = Some(msg.clone());
                }
                self.update_config(|c| c.was_running = false);
                self.sys(msg);
                self.set_status(Status::Crashed);
                break;
            }

            let delay = BACKOFF_SECS[attempt - 1];
            self.sys(format!("Restarting in {delay}s (attempt {attempt}/{MAX_CRASHES})..."));
            let (tx, rx) = oneshot::channel();
            {
                let mut rt = self.rt.lock().unwrap();
                rt.kill_tx = Some(tx);
                rt.in_backoff = true;
            }
            self.set_status(Status::Crashed);
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
                _ = rx => {}
            }
            let desired = {
                let mut rt = self.rt.lock().unwrap();
                rt.in_backoff = false;
                rt.kill_tx = None;
                rt.desired_running
            };
            if !desired {
                self.sys("Restart cancelled.");
                self.set_status(Status::Stopped);
                break;
            }
            match self.spawn_process().await {
                Ok(p) => {
                    proc = p;
                    self.rt.lock().unwrap().restarts += 1;
                }
                Err(e) => {
                    self.rt.lock().unwrap().desired_running = false;
                    self.update_config(|c| c.was_running = false);
                    self.fail(format!("Restart failed: {e:#}"));
                    break;
                }
            }
        }
    }
}

async fn pump(inst: Arc<Instance>, reader: impl AsyncRead + Unpin, stream: Stream) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(256);
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = logbuf::clean(&String::from_utf8_lossy(&buf));
                if !text.is_empty() {
                    inst.emit(stream, text);
                }
            }
        }
    }
}

fn describe_exit(s: &std::process::ExitStatus) -> String {
    if let Some(code) = s.code() {
        return format!("exit code {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return format!("killed by signal {sig}");
        }
    }
    "terminated".into()
}
