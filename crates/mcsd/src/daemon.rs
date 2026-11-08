//! The long-running daemon: lock, API server, supervisor boot and graceful
//! shutdown.
//!
//! Servers are children of the daemon. Stopping the daemon stops them
//! gracefully, and they are started again the next time the daemon boots.

use crate::api::{self, AppState};
use crate::config::DaemonConfig;
use crate::download;
use crate::manager::Manager;
use crate::paths::Paths;
use anyhow::{Context, Result, bail};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

pub async fn run(paths: Paths, bind_override: Option<String>, shutdown: impl Future<Output = ()>) -> Result<()> {
    paths.ensure()?;

    // One daemon per data directory.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.lock_file())?;
    if lock.try_lock().is_err() {
        bail!("another mcsd daemon is already running on {}", paths.root.display());
    }

    let cfg = DaemonConfig::load_or_init(&paths)?;
    let bind = bind_override.unwrap_or_else(|| cfg.bind.clone());
    let http = download::http_client()?;
    let mgr = Manager::load(paths.clone(), http).await?;

    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("cannot listen on {bind} (is another program using it?)"))?;
    let addr = listener.local_addr()?;
    tracing::info!("mcsd {} listening on http://{addr}", env!("CARGO_PKG_VERSION"));
    tracing::info!("data directory: {}", paths.root.display());
    tracing::info!("run `mcsd dashboard` to print the dashboard login token");
    if !addr.ip().is_loopback() {
        tracing::warn!(
            "the API is reachable from the network on {addr}; anyone with the token gets console access. Put a TLS reverse proxy in front of it."
        );
    }

    mgr.spawn_stats();
    {
        let mgr = mgr.clone();
        tokio::spawn(async move { mgr.boot().await });
    }

    let app = api::router(AppState {
        mgr: mgr.clone(),
        token: Arc::new(cfg.token.clone()),
    });
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
    });

    tokio::select! {
        _ = shutdown => {}
        res = &mut server => {
            res??;
            bail!("HTTP server stopped unexpectedly");
        }
    }

    tracing::info!("shutting down: stopping servers gracefully (Ctrl+C again to force)");
    mgr.shutdown().await;
    let _ = stop_tx.send(());
    // Open console websockets would otherwise hold graceful shutdown forever.
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
    drop(lock);
    tracing::info!("bye");
    Ok(())
}

/// Resolves on Ctrl+C / SIGTERM / console close. A second Ctrl+C forces exit.
pub async fn shutdown_signal() {
    wait_for_signal().await;
    tokio::spawn(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::warn!("forced exit");
        std::process::exit(130);
    });
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(windows)]
async fn wait_for_signal() {
    use tokio::signal::windows;
    let mut brk = windows::ctrl_break().expect("ctrl_break handler");
    let mut close = windows::ctrl_close().expect("ctrl_close handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = brk.recv() => {}
        _ = close.recv() => {}
    }
}
