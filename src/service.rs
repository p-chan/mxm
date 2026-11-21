//! `mxm service`: runs `mxm daemon` at login
//!
//! On macOS this is a LaunchAgent. Other platforms are not supported yet; on Linux this would be a
//! systemd user service.
//!
//! The LaunchAgent runs a copy of `mxm` at a fixed path, not the `mxm` that installed it. macOS
//! keeps the Input Monitoring permission per executable path (and checks the code signature), so
//! the permission would be lost whenever the path changes, e.g. when mise installs a new version
//! into a versioned directory. Running `mxm service install` again after an update replaces the
//! copy at the same path, and the permission carries over.

use std::path::{Path, PathBuf};

use anyhow::Result;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const LABEL: &str = "io.github.p-chan.mxm";

/// `ProgramArguments` of the LaunchAgent: `mxm daemon [--config <path>]`
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn program_arguments(exe: &Path, config: Option<&Path>) -> Vec<String> {
    let mut args = vec![exe.display().to_string(), "daemon".into()];
    if let Some(c) = config {
        args.push("--config".into());
        args.push(c.display().to_string());
    }
    args
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The LaunchAgent plist that runs `mxm daemon` at login and restarts it if it exits
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn plist(exe: &Path, config: Option<&Path>, log: &Path) -> String {
    let args: String = program_arguments(exe, config)
        .iter()
        .map(|a| format!("    <string>{}</string>\n", escape_xml(a)))
        .collect();
    let log = escape_xml(&log.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

/// The config path for the daemon, as an absolute path. Without `--config`, the default path is
/// resolved now, because launchd does not pass the shell's environment (e.g. `XDG_CONFIG_HOME`)
/// or current directory to the daemon
fn absolute_config(config: Option<PathBuf>) -> Result<PathBuf> {
    let path = match config {
        Some(c) => c,
        None => crate::config::default_path()?,
    };
    Ok(std::path::absolute(path)?)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use anyhow::{Context, Result, anyhow, bail};

    use super::{LABEL, plist};

    fn home() -> Result<PathBuf> {
        std::env::home_dir().ok_or_else(|| anyhow!("Cannot find the home directory"))
    }

    fn plist_path() -> Result<PathBuf> {
        Ok(home()?
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")))
    }

    fn log_path() -> Result<PathBuf> {
        Ok(home()?.join("Library/Logs/mxm.log"))
    }

    /// `gui/<uid>`, the launchd domain of the logged-in user
    fn domain() -> Result<String> {
        let out = Command::new("/usr/bin/id").arg("-u").output()?;
        let uid = String::from_utf8(out.stdout)?.trim().to_string();
        if !out.status.success() || uid.is_empty() {
            bail!("Cannot get the user ID");
        }
        Ok(format!("gui/{uid}"))
    }

    fn launchctl(args: &[&str]) -> Result<std::process::Output> {
        Command::new("/bin/launchctl")
            .args(args)
            .output()
            .context("Cannot run launchctl")
    }

    fn is_loaded(domain: &str) -> Result<bool> {
        Ok(launchctl(&["print", &format!("{domain}/{LABEL}")])?
            .status
            .success())
    }

    /// The copy of `mxm` that the LaunchAgent runs
    fn daemon_exe() -> Result<PathBuf> {
        Ok(home()?.join("Library/Application Support/mxm/mxm"))
    }

    /// Copies the running `mxm` to `dest`, through a temporary file renamed into place
    fn copy_self(dest: &Path) -> Result<()> {
        let src = std::env::current_exe().context("Cannot find the running mxm")?;
        // Skip when running the copy itself
        if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&src), std::fs::canonicalize(dest))
            && a == b
        {
            return Ok(());
        }
        let dir = dest.parent().context("Invalid path for the daemon")?;
        std::fs::create_dir_all(dir)?;
        // Unique per process, so that concurrent installs do not write to the same file
        let tmp = dir.join(format!(".mxm.{}.tmp", std::process::id()));
        let result = (|| {
            std::fs::copy(&src, &tmp)
                .with_context(|| format!("Cannot copy {} to {}", src.display(), tmp.display()))?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
            std::fs::rename(&tmp, dest).with_context(|| format!("Cannot write {}", dest.display()))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// Whether the installed copy differs from the running `mxm`
    fn is_outdated(dest: &Path) -> Option<bool> {
        let src = std::fs::read(std::env::current_exe().ok()?).ok()?;
        Some(std::fs::read(dest).ok()? != src)
    }

    /// Stops the daemon and waits until launchd has removed it. `launchctl bootout` can return
    /// before that, and `launchctl bootstrap` right after it then fails with an I/O error
    fn bootout(domain: &str) -> Result<()> {
        let out = launchctl(&["bootout", &format!("{domain}/{LABEL}")])?;
        if !out.status.success() {
            bail!(
                "launchctl bootout failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        for _ in 0..50 {
            if !is_loaded(domain)? {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        bail!("mxm daemon did not stop within 5 seconds")
    }

    pub fn print(config: Option<PathBuf>) -> Result<()> {
        print!("{}", plist(&daemon_exe()?, config.as_deref(), &log_path()?));
        Ok(())
    }

    pub fn install(config: Option<PathBuf>) -> Result<()> {
        let exe = daemon_exe()?;
        let path = plist_path()?;
        let log = log_path()?;
        let domain = domain()?;
        // Prepare the copy and the plist before stopping the running daemon, so that a failure
        // leaves it running. Replacing the copy by rename does not affect the running process
        copy_self(&exe)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, plist(&exe, config.as_deref(), &log))
            .with_context(|| format!("Cannot write {}", path.display()))?;
        // Reload so that the new copy and config take effect
        if is_loaded(&domain)? {
            bootout(&domain)?;
        }
        let out = launchctl(&["bootstrap", &domain, &path.display().to_string()])?;
        if !out.status.success() {
            bail!(
                "launchctl bootstrap failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        println!("Installed {} and started mxm daemon", path.display());
        println!("Daemon: {}", exe.display());
        println!("Log:    {}", log.display());
        println!();
        println!("To reach mice connected over Bluetooth, allow Input Monitoring for");
        println!("  {}", exe.display());
        println!("in System Settings > Privacy & Security > Input Monitoring.");
        println!("After updating mxm, run `mxm service install` again to update the daemon.");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let domain = domain()?;
        // Keep the plist if the daemon could not be stopped, so that uninstall can be retried
        if is_loaded(&domain)? {
            bootout(&domain)?;
        }
        let path = plist_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => println!("Stopped mxm daemon and removed {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("mxm service is not installed");
            }
            Err(e) => return Err(e.into()),
        }
        let exe = daemon_exe()?;
        match std::fs::remove_file(&exe) {
            Ok(()) => {
                println!("Removed {}", exe.display());
                if let Some(dir) = exe.parent() {
                    // Only succeeds when empty
                    let _ = std::fs::remove_dir(dir);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    pub fn status() -> Result<()> {
        let path = plist_path()?;
        if !path.exists() {
            println!("Not installed");
            return Ok(());
        }
        let out = launchctl(&["print", &format!("{}/{LABEL}", domain()?)])?;
        let text = String::from_utf8_lossy(&out.stdout);
        let field = |name: &str| {
            text.lines()
                .map(str::trim)
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(" = "))
                .map(str::to_string)
        };
        println!("Installed: {}", path.display());
        if out.status.success() {
            let state = field("state").unwrap_or_else(|| "unknown".into());
            match field("pid") {
                Some(pid) => println!("State:     {state} (pid {pid})"),
                None => println!("State:     {state}"),
            }
        } else {
            println!("State:     not loaded (run `mxm service install` again)");
        }
        let exe = daemon_exe()?;
        println!("Daemon:    {}", exe.display());
        if !exe.exists() {
            println!("           missing (run `mxm service install` again)");
        } else if is_outdated(&exe) == Some(true) {
            println!("           differs from this mxm (run `mxm service install` to update)");
        }
        println!("Log:       {}", log_path()?.display());
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::PathBuf;

    use anyhow::{Result, bail};

    fn unsupported() -> Result<()> {
        bail!(
            "`mxm service` is not supported on this platform yet; run `mxm daemon` with your service manager"
        )
    }

    pub fn print(_: Option<PathBuf>) -> Result<()> {
        unsupported()
    }
    pub fn install(_: Option<PathBuf>) -> Result<()> {
        unsupported()
    }
    pub fn uninstall() -> Result<()> {
        unsupported()
    }
    pub fn status() -> Result<()> {
        unsupported()
    }
}

pub fn install(config: Option<PathBuf>, print_only: bool) -> Result<()> {
    let config = Some(absolute_config(config)?);
    if print_only {
        platform::print(config)
    } else {
        platform::install(config)
    }
}

pub fn uninstall() -> Result<()> {
    platform::uninstall()
}

pub fn status() -> Result<()> {
    platform::status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_runs_the_daemon_with_the_given_config() {
        let p = plist(
            Path::new("/Users/me/.local/bin/mxm"),
            Some(Path::new("/Users/me/my & config.toml")),
            Path::new("/Users/me/Library/Logs/mxm.log"),
        );
        assert!(p.contains("<string>io.github.p-chan.mxm</string>"));
        assert!(p.contains(
            "    <string>/Users/me/.local/bin/mxm</string>\n    <string>daemon</string>\n    <string>--config</string>\n    <string>/Users/me/my &amp; config.toml</string>\n"
        ));
        assert!(p.contains("<string>/Users/me/Library/Logs/mxm.log</string>"));
    }

    #[test]
    fn plist_without_config_uses_the_default() {
        let p = plist(Path::new("/opt/mxm"), None, Path::new("/tmp/mxm.log"));
        assert!(!p.contains("--config"));
    }
}
