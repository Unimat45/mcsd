mod api;
mod cli;
mod config;
mod daemon;
mod download;
mod instance;
mod java;
mod logbuf;
mod manager;
mod parse;
mod paths;
mod procutil;
mod properties;
mod providers;
mod registry;
mod service;
mod web;

use clap::Parser;
use cli::{Cli, Cmd, ServiceCmd};
use paths::Paths;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    // reqwest uses rustls with the `ring` backend: pure cargo build, no cmake.
    let _ = rustls::crypto::ring::default_provider().install_default();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let paths = Paths::resolve(cli.data_dir)?;
    match cli.cmd {
        Cmd::Daemon { bind } => {
            paths.ensure()?;
            let _guard = init_daemon_logging(&paths, true);
            runtime()?.block_on(daemon::run(paths, bind, daemon::shutdown_signal()))
        }
        Cmd::Service(ServiceCmd::Install) => service::install(&paths),
        Cmd::Service(ServiceCmd::Uninstall) => service::uninstall(),
        Cmd::Service(ServiceCmd::Run) => {
            #[cfg(windows)]
            {
                paths.ensure()?;
                let _guard = init_daemon_logging(&paths, false);
                service::winsvc::run(paths)
            }
            #[cfg(not(windows))]
            {
                anyhow::bail!("`service run` is only used by Windows; use `mcsd daemon`")
            }
        }
        other => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter("warn"))
                .with_writer(std::io::stderr)
                .init();
            runtime()?.block_on(cli::run(paths, other))
        }
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?)
}

fn env_filter(default: &str) -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_env("MCSD_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default))
}

/// Daemon logs go to `<data>/logs/daemon/mcsd.YYYY-MM-DD.log` (7 days kept)
/// and, when attached to a console, to stdout as well.
fn init_daemon_logging(paths: &Paths, console: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("mcsd")
        .filename_suffix("log")
        .max_log_files(7)
        .build(paths.daemon_log_dir())
        .ok();
    let (file_layer, guard) = match appender {
        Some(a) => {
            let (w, g) = tracing_appender::non_blocking(a);
            (Some(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(w)), Some(g))
        }
        None => (None, None),
    };
    let console_layer = console.then(tracing_subscriber::fmt::layer);
    tracing_subscriber::registry()
        .with(env_filter("info"))
        .with(file_layer)
        .with(console_layer)
        .init();
    guard
}
