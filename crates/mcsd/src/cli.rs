//! Command-line client: talks to the daemon's HTTP API using the token from
//! `<data>/config.toml`.

use crate::config::DaemonConfig;
use crate::paths::Paths;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use reqwest::Method;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "mcsd",
    version,
    about = "Create, run and supervise Minecraft servers (daemon + CLI + web dashboard)"
)]
pub struct Cli {
    /// Data directory (servers, logs, config). Defaults to the OS app-data dir.
    #[arg(long, global = true, env = "MCSD_HOME", value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Run the daemon in the foreground
    Daemon {
        /// Listen address (overrides config.toml), e.g. 127.0.0.1:8765
        #[arg(long)]
        bind: Option<String>,
    },
    /// List servers
    #[command(alias = "ls")]
    List,
    /// Show details of a server
    Info { name: String },
    /// Create (and install) a new server
    Create(CreateArgs),
    /// Start a server
    Start { name: String },
    /// Stop a server gracefully (sends "stop")
    Stop { name: String },
    /// Restart a server
    Restart { name: String },
    /// Kill a server process immediately (no world save!)
    Kill { name: String },
    /// Re-download the server jar (e.g. latest Paper build)
    Reinstall { name: String },
    /// Delete a server from the registry
    #[command(alias = "rm")]
    Remove {
        name: String,
        /// Also delete the world and all server files
        #[arg(long)]
        delete_files: bool,
    },
    /// Send a console command, e.g. `mcsd send survival say hello`
    Send {
        name: String,
        #[arg(trailing_var_arg = true, required = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Show console output
    Logs {
        name: String,
        /// Number of lines
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Keep streaming new lines
        #[arg(short, long)]
        follow: bool,
    },
    /// Change server settings (applied on next restart)
    Set(SetArgs),
    /// List available versions for a server kind (vanilla, paper, fabric)
    Versions { kind: String },
    /// Manage Java runtimes
    #[command(subcommand)]
    Java(JavaCmd),
    /// Print the dashboard login token (stdout) and how to reach the dashboard (stderr)
    Dashboard,
    /// Install/uninstall mcsd as a system service (start at boot)
    #[command(subcommand)]
    Service(ServiceCmd),
}

#[derive(Args)]
pub struct CreateArgs {
    pub name: String,
    /// vanilla | paper | fabric | custom
    #[arg(short, long, default_value = "paper")]
    pub kind: String,
    /// Minecraft version (default: latest release)
    #[arg(short, long)]
    pub version: Option<String>,
    /// Heap size in MB
    #[arg(short, long, default_value_t = 2048)]
    pub memory: u32,
    /// Server port (default: first free from 25565)
    #[arg(short, long)]
    pub port: Option<u16>,
    /// Accept the Minecraft EULA (https://aka.ms/MinecraftEULA) - required
    #[arg(long)]
    pub accept_eula: bool,
    /// Start whenever the daemon starts
    #[arg(long)]
    pub autostart: bool,
    /// Start right after installing
    #[arg(long)]
    pub start: bool,
    /// custom kind: jar file to copy into the server directory
    #[arg(long)]
    pub jar: Option<PathBuf>,
    /// Force a Java major version (default: what the MC version requires)
    #[arg(long)]
    pub java_major: Option<u32>,
    /// Use this java binary instead of an auto-selected one
    #[arg(long)]
    pub java_path: Option<String>,
}

#[derive(Args)]
pub struct SetArgs {
    pub name: String,
    #[arg(long)]
    pub memory: Option<u32>,
    #[arg(long)]
    pub port: Option<u16>,
    /// Change version (server must be stopped; re-downloads on next start)
    #[arg(long)]
    pub version: Option<String>,
    #[arg(long)]
    pub autostart: Option<bool>,
    #[arg(long)]
    pub auto_restart: Option<bool>,
    /// Seconds to wait for a graceful stop before killing
    #[arg(long)]
    pub stop_timeout: Option<u64>,
    /// Extra JVM argument (repeatable; replaces the current list)
    #[arg(long = "jvm-arg", allow_hyphen_values = true)]
    pub jvm_args: Option<Vec<String>>,
    /// Java binary to use ("" = automatic)
    #[arg(long)]
    pub java_path: Option<String>,
    #[arg(long)]
    pub java_major: Option<u32>,
}

#[derive(Subcommand)]
pub enum JavaCmd {
    /// List detected Java runtimes
    #[command(alias = "ls")]
    List,
    /// Download a Temurin runtime, e.g. `mcsd java install 21`
    Install { major: u32 },
}

#[derive(Subcommand)]
pub enum ServiceCmd {
    /// Register and start the daemon as a service
    Install,
    /// Stop and remove the service
    Uninstall,
    /// (internal) entry point for the Windows Service Control Manager
    #[command(hide = true)]
    Run,
}

struct Client {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl Client {
    fn new(paths: &Paths) -> Result<Self> {
        let cfg = DaemonConfig::load_or_init(paths)?;
        Ok(Self {
            base: format!("{}/api", cfg.local_url()),
            token: cfg.token,
            http: reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build()?,
        })
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_connect() {
                anyhow!(
                    "cannot reach the mcsd daemon at {} - is it running? Start it with `mcsd daemon` or `mcsd service install`.",
                    self.base.trim_end_matches("/api")
                )
            } else {
                e.into()
            }
        })?;
        let status = resp.status();
        let text = resp.text().await?;
        let v: Value = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
        if !status.is_success() {
            bail!("{}", v["error"].as_str().unwrap_or(status.as_str()));
        }
        Ok(v)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.call(Method::GET, path, None).await
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(Method::POST, path, Some(body)).await
    }
}

pub async fn run(paths: Paths, cmd: Cmd) -> Result<()> {
    if let Cmd::Dashboard = cmd {
        let cfg = DaemonConfig::load_or_init(&paths)?;
        print_dashboard_info(&cfg, &paths);
        return Ok(());
    }
    let c = Client::new(&paths)?;
    match cmd {
        Cmd::List => {
            let servers = c.get("/servers").await?;
            print_table(servers.as_array().map(Vec::as_slice).unwrap_or(&[]));
        }
        Cmd::Info { name } => {
            let s = c.get(&format!("/servers/{name}")).await?;
            print_info(&s);
        }
        Cmd::Create(a) => {
            if !a.accept_eula {
                bail!("you must accept the Minecraft EULA (https://aka.ms/MinecraftEULA): pass --accept-eula");
            }
            let jar_source = a
                .jar
                .map(|p| p.canonicalize().with_context(|| format!("jar not found: {}", p.display())))
                .transpose()?;
            let body = json!({
                "name": a.name, "kind": a.kind.to_ascii_lowercase(), "version": a.version,
                "memory_mb": a.memory, "port": a.port, "accept_eula": true,
                "autostart": a.autostart, "start": a.start, "java_major": a.java_major,
                "java_path": a.java_path, "jar_source": jar_source,
            });
            let s = c.post("/servers", body).await?;
            println!(
                "Created '{}' ({} {}) on port {}. Installing in the background...",
                s["name"].as_str().unwrap_or_default(),
                s["kind"].as_str().unwrap_or_default(),
                s["version"].as_str().unwrap_or_default(),
                s["port"]
            );
            println!("Follow progress with: mcsd logs {} -f", a.name);
        }
        Cmd::Start { name } => action(&c, &name, "start", "Started").await?,
        Cmd::Stop { name } => action(&c, &name, "stop", "Stopped").await?,
        Cmd::Restart { name } => action(&c, &name, "restart", "Restarted").await?,
        Cmd::Kill { name } => action(&c, &name, "kill", "Killed").await?,
        Cmd::Reinstall { name } => action(&c, &name, "reinstall", "Reinstalling").await?,
        Cmd::Remove { name, delete_files } => {
            c.call(Method::DELETE, &format!("/servers/{name}?delete_files={delete_files}"), None)
                .await?;
            println!("Removed '{name}'{}", if delete_files { " and its files" } else { " (files kept)" });
        }
        Cmd::Send { name, command } => {
            c.post(&format!("/servers/{name}/command"), json!({ "command": command.join(" ") }))
                .await?;
        }
        Cmd::Logs { name, lines, follow } => {
            let logs = c.get(&format!("/servers/{name}/logs?tail={lines}")).await?;
            let mut last = print_lines(&logs, 0);
            if !follow {
                return Ok(());
            }
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let logs = c.get(&format!("/servers/{name}/logs?after={last}")).await?;
                last = print_lines(&logs, last);
            }
        }
        Cmd::Set(a) => {
            let mut body = serde_json::Map::new();
            let mut put = |k: &str, v: Value| {
                if !v.is_null() {
                    body.insert(k.into(), v);
                }
            };
            put("memory_mb", json!(a.memory));
            put("port", json!(a.port));
            put("version", json!(a.version));
            put("autostart", json!(a.autostart));
            put("auto_restart", json!(a.auto_restart));
            put("stop_timeout_secs", json!(a.stop_timeout));
            put("jvm_args", json!(a.jvm_args));
            put("java_path", json!(a.java_path));
            put("java_major", json!(a.java_major));
            if body.is_empty() {
                bail!("nothing to change (see `mcsd set --help`)");
            }
            let s = c.call(Method::PATCH, &format!("/servers/{}", a.name), Some(Value::Object(body))).await?;
            print_info(&s);
        }
        Cmd::Versions { kind } => {
            let v = c.get(&format!("/versions/{kind}")).await?;
            for ver in v.as_array().into_iter().flatten() {
                println!("{}", ver.as_str().unwrap_or_default());
            }
        }
        Cmd::Java(JavaCmd::List) => {
            let v = c.get("/java?refresh=true").await?;
            for r in v.as_array().into_iter().flatten() {
                println!(
                    "{:>3}  {:<14} {}{}",
                    r["major"],
                    r["version"].as_str().unwrap_or_default(),
                    r["path"].as_str().unwrap_or_default(),
                    if r["managed"] == true { "  (managed)" } else { "" }
                );
            }
        }
        Cmd::Java(JavaCmd::Install { major }) => {
            println!("Downloading Java {major} (this can take a minute)...");
            let r = c.post("/java/install", json!({ "major": major })).await?;
            println!("Installed {} at {}", r["version"].as_str().unwrap_or_default(), r["path"].as_str().unwrap_or_default());
        }
        Cmd::Daemon { .. } | Cmd::Service(_) | Cmd::Dashboard => unreachable!("handled in main"),
    }
    Ok(())
}

/// The token goes alone on stdout (`TOKEN=$(mcsd dashboard)` works); the
/// human guidance goes to stderr.
fn print_dashboard_info(cfg: &DaemonConfig, paths: &Paths) {
    use std::net::SocketAddr;
    eprintln!("Dashboard login token:");
    println!("{}", cfg.token);
    eprintln!();
    match cfg.bind.parse::<SocketAddr>() {
        Ok(addr) if addr.ip().is_loopback() => {
            let port = addr.port();
            eprintln!("Dashboard: http://{addr} (only reachable from this machine)");
            eprintln!();
            eprintln!("To open it from another computer, either:");
            eprintln!("  - tunnel it over SSH, then browse to http://127.0.0.1:{port}");
            eprintln!("      ssh -L {port}:127.0.0.1:{port} <user>@<this-machine>");
            eprintln!(
                "  - or set bind = \"0.0.0.0:{port}\" in {} and restart the daemon",
                paths.config_file().display()
            );
            eprintln!("    (put an HTTPS reverse proxy in front: the token is sent with every request)");
        }
        Ok(addr) if addr.ip().is_unspecified() => {
            eprintln!("Dashboard: http://<this-machine's-address>:{}", addr.port());
        }
        _ => eprintln!("Dashboard: http://{}", cfg.bind),
    }
}

async fn action(c: &Client, name: &str, verb: &str, done: &str) -> Result<()> {
    let s = c.post(&format!("/servers/{name}/{verb}"), json!({})).await?;
    println!("{done} '{name}' (status: {})", s["status"].as_str().unwrap_or("?"));
    if let Some(err) = s["last_error"].as_str() {
        println!("last error: {err}");
    }
    Ok(())
}

fn print_lines(logs: &Value, mut last: u64) -> u64 {
    for l in logs.as_array().into_iter().flatten() {
        let seq = l["seq"].as_u64().unwrap_or(0);
        if seq <= last {
            continue;
        }
        last = seq;
        let text = l["text"].as_str().unwrap_or_default();
        match l["stream"].as_str() {
            Some("sys") => println!("[mcsd] {text}"),
            _ => println!("{text}"),
        }
    }
    last
}

/// Whole megabytes, matching the unit of the `memory_mb` heap setting.
fn megabytes(b: u64) -> String {
    if b == 0 {
        return "-".into();
    }
    format!("{}", (b as f64 / 1_048_576.0).round() as u64)
}

fn human_secs(s: Option<i64>) -> String {
    match s {
        None => "-".into(),
        Some(s) if s < 3600 => format!("{}m{}s", s / 60, s % 60),
        Some(s) if s < 86400 => format!("{}h{}m", s / 3600, (s % 3600) / 60),
        Some(s) => format!("{}d{}h", s / 86400, (s % 86400) / 3600),
    }
}

fn print_table(servers: &[Value]) {
    if servers.is_empty() {
        println!("No servers yet. Create one with: mcsd create <name> --accept-eula");
        return;
    }
    let rows: Vec<[String; 8]> = servers
        .iter()
        .map(|s| {
            [
                s["name"].as_str().unwrap_or_default().to_string(),
                format!("{} {}", s["kind"].as_str().unwrap_or_default(), s["version"].as_str().unwrap_or_default()),
                s["status"].as_str().unwrap_or_default().to_string(),
                s["port"].to_string(),
                s["players"].as_array().map(|p| p.len()).unwrap_or(0).to_string(),
                format!(
                    "{} / {} MB",
                    megabytes(s["memory_bytes"].as_u64().unwrap_or(0)),
                    s["memory_mb"]
                ),
                format!("{:.0}%", s["cpu_percent"].as_f64().unwrap_or(0.0)),
                human_secs(s["uptime_secs"].as_i64()),
            ]
        })
        .collect();
    let header = ["NAME", "TYPE", "STATUS", "PORT", "PLAYERS", "MEMORY", "CPU", "UPTIME"];
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.len());
        }
    }
    let line = |cols: Vec<&str>| {
        cols.iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<w$}", w = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    println!("{}", line(header.to_vec()));
    for r in &rows {
        println!("{}", line(r.iter().map(String::as_str).collect()));
    }
}

fn print_info(s: &Value) {
    let field = |k: &str| match &s[k] {
        Value::Null => "-".to_string(),
        Value::String(x) => x.clone(),
        other => other.to_string(),
    };
    println!("{} ({} {})", field("name"), field("kind"), field("version"));
    println!("  status:       {}", field("status"));
    println!("  port:         {}", field("port"));
    println!("  memory:       {} / {} MB (process / heap)", megabytes(s["memory_bytes"].as_u64().unwrap_or(0)), field("memory_mb"));
    println!("  players:      {}", s["players"].as_array().map(|p| p.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).filter(|x| !x.is_empty()).unwrap_or("-".into()));
    println!("  uptime:       {}", human_secs(s["uptime_secs"].as_i64()));
    println!("  java:         {} (path: {})", field("java_major"), field("java_path"));
    println!("  autostart:    {}   auto_restart: {}", field("autostart"), field("auto_restart"));
    println!("  directory:    {}", field("dir"));
    if !s["last_error"].is_null() {
        println!("  last error:   {}", field("last_error"));
    }
    if !s["last_exit"].is_null() {
        println!("  last exit:    {}", field("last_exit"));
    }
}
