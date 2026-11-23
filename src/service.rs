//! `mxm service`: runs `mxm daemon` at login
//!
//! On macOS this is a LaunchAgent. Other platforms are not supported yet; on Linux this would be a
//! systemd user service.
//!
//! The LaunchAgent runs a copy of `mxm` at a fixed path, not the `mxm` that installed it. macOS
//! keeps the Input Monitoring permission per executable path (and checks the code signature), so
//! the permission would be lost whenever the path changes, e.g. when mise installs a new version
//! into a versioned directory. Running `mxm service update` after updating `mxm` replaces the
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
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use anyhow::{Context, Result, bail};

    use super::{LABEL, plist};

    /// The home directory from the user database rather than `$HOME`, because launchd looks for
    /// LaunchAgents there, and Homebrew runs cask install steps with `$HOME` set to a temporary
    /// directory
    fn home() -> Result<PathBuf> {
        use std::ffi::{CStr, OsStr};
        use std::os::unix::ffi::OsStrExt;

        let mut buf = vec![0 as libc::c_char; 4096];
        // SAFETY: `passwd` is plain data, and getpwuid_r only writes into `pwd` and `buf`
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        // SAFETY: the pointers are valid for the given lengths for the duration of the call
        let rc = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        if rc != 0 || result.is_null() || pwd.pw_dir.is_null() {
            bail!("Cannot find the home directory");
        }
        // SAFETY: on success, `pw_dir` points to a NUL-terminated string in `buf`
        let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
        Ok(PathBuf::from(OsStr::from_bytes(dir.to_bytes())))
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

    /// Restarts the loaded daemon with the same plist, so that it runs the current copy.
    /// Unlike `bootstrap`, this works from Homebrew's sandbox for cask install steps
    fn restart(domain: &str) -> Result<()> {
        let out = launchctl(&["kickstart", "-k", &format!("{domain}/{LABEL}")])?;
        if !out.status.success() {
            bail!(
                "launchctl kickstart failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    /// Stops the daemon if it is loaded, and starts it again from the plist at `path`
    fn reload(domain: &str, path: &Path) -> Result<()> {
        if is_loaded(domain)? {
            bootout(domain)?;
        }
        let out = launchctl(&["bootstrap", domain, &path.display().to_string()])?;
        if !out.status.success() {
            bail!(
                "launchctl bootstrap failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    pub fn print(config: Option<PathBuf>) -> Result<()> {
        let plist = plist(&daemon_exe()?, config.as_deref(), &log_path()?);
        write!(std::io::stdout().lock(), "{plist}")?;
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
        reload(&domain, &path)?;
        let mut out = std::io::stdout().lock();
        writeln!(out, "Installed {} and started mxm daemon", path.display())?;
        writeln!(out, "Daemon: {}", exe.display())?;
        writeln!(out, "Log:    {}", log.display())?;
        writeln!(out)?;
        writeln!(
            out,
            "To reach mice connected over Bluetooth, allow Input Monitoring for"
        )?;
        writeln!(out, "  {}", exe.display())?;
        writeln!(
            out,
            "in System Settings > Privacy & Security > Input Monitoring."
        )?;
        writeln!(out, "For software scrolling, allow it in")?;
        writeln!(
            out,
            "System Settings > Privacy & Security > Accessibility as well."
        )?;
        writeln!(
            out,
            "After updating mxm, run `mxm service update` to update the daemon."
        )?;
        Ok(())
    }

    pub fn update() -> Result<()> {
        let path = plist_path()?;
        let mut out = std::io::stdout().lock();
        // Package managers run this after every install, so do nothing for users who do not use
        // the service
        if !path.exists() {
            writeln!(out, "mxm service is not installed; nothing to update")?;
            return Ok(());
        }
        let exe = daemon_exe()?;
        if is_outdated(&exe) == Some(false) {
            writeln!(out, "mxm daemon is already up to date: {}", exe.display())?;
            return Ok(());
        }
        // Keep the plist, so that the config path given to `mxm service install` (or a plist
        // managed with `--print`) stays as it is
        copy_self(&exe)?;
        let domain = domain()?;
        if is_loaded(&domain)? {
            restart(&domain)?;
            writeln!(out, "Updated {} and restarted mxm daemon", exe.display())?;
        } else {
            writeln!(out, "Updated {}", exe.display())?;
            writeln!(
                out,
                "mxm daemon is not loaded; run `mxm service install` to start it"
            )?;
        }
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let domain = domain()?;
        // Keep the plist if the daemon could not be stopped, so that uninstall can be retried
        if is_loaded(&domain)? {
            bootout(&domain)?;
        }
        let path = plist_path()?;
        let mut out = std::io::stdout().lock();
        // Finish removing before returning a write error (e.g. a closed pipe), so that uninstall
        // is not left half done
        let mut written = match std::fs::remove_file(&path) {
            Ok(()) => writeln!(out, "Stopped mxm daemon and removed {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                writeln!(out, "mxm service is not installed")
            }
            Err(e) => return Err(e.into()),
        };
        let exe = daemon_exe()?;
        match std::fs::remove_file(&exe) {
            Ok(()) => {
                written = written.and(writeln!(out, "Removed {}", exe.display()));
                if let Some(dir) = exe.parent() {
                    // Only succeeds when empty
                    let _ = std::fs::remove_dir(dir);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(written?)
    }

    pub fn status() -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        let path = plist_path()?;
        if !path.exists() {
            writeln!(stdout, "Not installed")?;
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
        writeln!(stdout, "Installed: {}", path.display())?;
        if out.status.success() {
            let state = field("state").unwrap_or_else(|| "unknown".into());
            match field("pid") {
                Some(pid) => writeln!(stdout, "State:     {state} (pid {pid})")?,
                None => writeln!(stdout, "State:     {state}")?,
            }
        } else {
            writeln!(
                stdout,
                "State:     not loaded (run `mxm service install` again)"
            )?;
        }
        let exe = daemon_exe()?;
        writeln!(stdout, "Daemon:    {}", exe.display())?;
        if !exe.exists() {
            writeln!(
                stdout,
                "           missing (run `mxm service install` again)"
            )?;
        } else if is_outdated(&exe) == Some(true) {
            writeln!(
                stdout,
                "           differs from this mxm (run `mxm service update`)"
            )?;
        }
        writeln!(stdout, "Log:       {}", log_path()?.display())?;
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
    pub fn update() -> Result<()> {
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

pub fn update() -> Result<()> {
    platform::update()
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
