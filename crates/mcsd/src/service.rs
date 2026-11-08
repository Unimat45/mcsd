//! `mcsd service install|uninstall`: run the daemon at boot.
//!
//! * Linux: systemd unit (user unit, or system unit when run as root)
//! * macOS: launchd LaunchAgent
//! * Windows: Windows service (needs an elevated prompt)

use crate::paths::Paths;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

#[cfg_attr(target_os = "macos", allow(dead_code))] // launchd uses a reverse-DNS label
pub const SERVICE_NAME: &str = "mcsd";

fn exe() -> Result<PathBuf> {
    std::env::current_exe().context("locating the mcsd executable")
}

pub fn install(paths: &Paths) -> Result<()> {
    paths.ensure()?;
    platform::install(paths)
}

pub fn uninstall() -> Result<()> {
    platform::uninstall()
}

#[allow(dead_code)]
fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("running {cmd}"))?;
    if !status.success() {
        bail!("`{cmd} {}` failed ({status})", args.join(" "));
    }
    Ok(())
}

// ------------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    fn is_root() -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    fn unit_path() -> Result<PathBuf> {
        if is_root() {
            Ok(PathBuf::from(format!("/etc/systemd/system/{SERVICE_NAME}.service")))
        } else {
            let base = directories::BaseDirs::new().context("no home directory")?;
            Ok(base.config_dir().join(format!("systemd/user/{SERVICE_NAME}.service")))
        }
    }

    fn systemctl(args: &[&str]) -> Result<()> {
        let mut full: Vec<&str> = if is_root() { vec![] } else { vec!["--user"] };
        full.extend_from_slice(args);
        run("systemctl", &full)
    }

    pub fn install(paths: &Paths) -> Result<()> {
        let path = unit_path()?;
        let wanted_by = if is_root() { "multi-user.target" } else { "default.target" };
        let unit = format!(
            "[Unit]\n\
             Description=mcsd - Minecraft server daemon\n\
             After=network-online.target\n\
             Wants=network-online.target\n\n\
             [Service]\n\
             Type=simple\n\
             ExecStart=\"{exe}\" --data-dir \"{data}\" daemon\n\
             Restart=on-failure\n\
             RestartSec=5\n\
             # Only signal the daemon; it stops the Minecraft servers itself.\n\
             KillMode=mixed\n\
             TimeoutStopSec=180\n\n\
             [Install]\n\
             WantedBy={wanted_by}\n",
            exe = exe()?.display(),
            data = paths.root.display(),
        );
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, unit).with_context(|| format!("writing {}", path.display()))?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", "--now", SERVICE_NAME])?;
        println!("Installed systemd unit {}", path.display());
        if !is_root() {
            println!(
                "To keep it running after you log out / start at boot, run:\n  sudo loginctl enable-linger {}",
                std::env::var("USER").unwrap_or_else(|_| "$USER".into())
            );
        }
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let path = unit_path()?;
        let _ = systemctl(&["disable", "--now", SERVICE_NAME]);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        systemctl(&["daemon-reload"])?;
        println!("Removed {}", path.display());
        Ok(())
    }
}

// ------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    const LABEL: &str = "dev.mcsd.daemon";

    fn plist_path() -> Result<PathBuf> {
        let base = directories::BaseDirs::new().context("no home directory")?;
        Ok(base.home_dir().join(format!("Library/LaunchAgents/{LABEL}.plist")))
    }

    fn xml(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    pub fn install(paths: &Paths) -> Result<()> {
        let path = plist_path()?;
        let log = paths.daemon_log_dir().join("launchd.log");
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--data-dir</string>
    <string>{data}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ExitTimeOut</key><integer>180</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
            exe = xml(&exe()?.display().to_string()),
            data = xml(&paths.root.display().to_string()),
            log = xml(&log.display().to_string()),
        );
        std::fs::create_dir_all(path.parent().unwrap())?;
        let p = path.display().to_string();
        let _ = run("launchctl", &["unload", &p]);
        std::fs::write(&path, plist)?;
        run("launchctl", &["load", "-w", &p])?;
        println!("Installed LaunchAgent {p}");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let path = plist_path()?;
        if path.exists() {
            let p = path.display().to_string();
            let _ = run("launchctl", &["unload", "-w", &p]);
            std::fs::remove_file(&path)?;
            println!("Removed {p}");
        }
        Ok(())
    }
}

// ----------------------------------------------------------------- Windows

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::OsString;
    use std::time::{Duration, Instant};
    use windows_service::service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
    };
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    fn manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
        ServiceManager::local_computer(None::<&str>, access)
            .context("cannot open the service manager - run this from an elevated (Administrator) terminal")
    }

    pub fn install(paths: &Paths) -> Result<()> {
        let mgr = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from("mcsd Minecraft Server Daemon"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe()?,
            launch_arguments: vec![
                OsString::from("--data-dir"),
                paths.root.clone().into_os_string(),
                OsString::from("service"),
                OsString::from("run"),
            ],
            dependencies: vec![],
            account_name: None, // LocalSystem
            account_password: None,
        };
        let svc = mgr
            .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
            .context("creating the service (already installed? try `mcsd service uninstall` first)")?;
        svc.set_description("Runs and supervises Minecraft servers. Dashboard: http://127.0.0.1:8765")?;
        svc.start::<&str>(&[])?;
        println!("Installed and started the '{SERVICE_NAME}' Windows service.");
        println!("Data directory: {}", paths.root.display());
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let mgr = manager(ServiceManagerAccess::CONNECT)?;
        let svc = mgr
            .open_service(
                SERVICE_NAME,
                ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
            )
            .context("opening the service (is it installed?)")?;
        if svc.query_status()?.current_state != ServiceState::Stopped {
            println!("Stopping service (servers are stopped gracefully)...");
            svc.stop()?;
            let deadline = Instant::now() + Duration::from_secs(180);
            while svc.query_status()?.current_state != ServiceState::Stopped && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        svc.delete()?;
        println!("Removed the '{SERVICE_NAME}' Windows service.");
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use super::*;
    pub fn install(_paths: &Paths) -> Result<()> {
        bail!("service installation is not supported on this OS; run `mcsd daemon` from your init system")
    }
    pub fn uninstall() -> Result<()> {
        bail!("service installation is not supported on this OS")
    }
}

/// Entry point when started by the Windows Service Control Manager.
#[cfg(windows)]
pub mod winsvc {
    use super::SERVICE_NAME;
    use crate::paths::Paths;
    use anyhow::{Context, Result};
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    static PATHS: OnceLock<Paths> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    pub fn run(paths: Paths) -> Result<()> {
        let _ = PATHS.set(paths);
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .context("not launched by the Service Control Manager (use `mcsd daemon` in a console)")
    }

    fn service_main(_args: Vec<OsString>) {
        if let Err(e) = run_service() {
            tracing::error!("service failed: {e:#}");
        }
    }

    fn run_service() -> Result<()> {
        let paths = PATHS.get().cloned().context("data dir not set")?;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let tx = Mutex::new(Some(tx));
        let handle = service_control_handler::register(SERVICE_NAME, move |ev| match ev {
            ServiceControl::Stop | ServiceControl::Preshutdown | ServiceControl::Shutdown => {
                if let Some(t) = tx.lock().unwrap().take() {
                    let _ = t.send(());
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;
        let status = move |state, accept, wait_hint: Duration, code: u32| {
            let _ = handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: accept,
                exit_code: if code == 0 {
                    ServiceExitCode::Win32(0)
                } else {
                    ServiceExitCode::ServiceSpecific(code)
                },
                checkpoint: 0,
                wait_hint,
                process_id: None,
            });
        };
        // PRESHUTDOWN gives us minutes (not seconds) at OS shutdown to save worlds.
        status(
            ServiceState::Running,
            ServiceControlAccept::STOP | ServiceControlAccept::PRESHUTDOWN,
            Duration::ZERO,
            0,
        );
        let rt = tokio::runtime::Runtime::new()?;
        let result = rt.block_on(crate::daemon::run(paths, None, async move {
            let _ = rx.await;
            status(
                ServiceState::StopPending,
                ServiceControlAccept::empty(),
                Duration::from_secs(180),
                0,
            );
        }));
        let code = if result.is_ok() { 0 } else { 1 };
        if let Err(e) = &result {
            tracing::error!("daemon error: {e:#}");
        }
        status(ServiceState::Stopped, ServiceControlAccept::empty(), Duration::ZERO, code);
        result
    }
}
