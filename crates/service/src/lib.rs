//! Keeping the Mimi daemon running: where its binary is, and how to make it start at
//! login and keep running with no window open.
//!
//! - Linux: a systemd user unit (`~/.config/systemd/user/mimi.service`), restarted on
//!   failure. Without systemd, an XDG autostart entry starts it at login instead.
//! - macOS: a LaunchAgent (`~/Library/LaunchAgents/dev.mimi.daemon.plist`), relaunched
//!   if it crashes.
//!
//! One service per data directory: the default install is `mimi` / `dev.mimi.daemon`;
//! an instance with a custom `MIMI_HOME` (development, tests) gets a name derived from
//! that path, so it can never replace the real one.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mimi_protocol::Paths;
use mimi_protocol::paths::HOME_ENV;

/// The daemon's executable name, in bundles (a Tauri sidecar next to the app) and in
/// `target/<profile>/` during development.
pub const DAEMON_BIN: &str = "mimid";

/// Overrides where the daemon binary is found.
pub const BIN_ENV: &str = "MIMI_DAEMON_BIN";

/// Set by `pnpm dev` (or anyone running the daemon themselves): the app connects to the
/// daemon but never starts, stops or installs one.
pub const EXTERNAL_ENV: &str = "MIMI_DAEMON";

/// Port override, carried into the service when set at install time.
const PORT_ENV: &str = "MIMI_PORT";

/// Key storage override (`file`), carried into the service when set at install time.
const KEY_STORE_ENV: &str = "MIMI_KEY_STORE";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't write the service file: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Command(String),
    #[error("couldn't find the Mimi assistant program ({DAEMON_BIN})")]
    NoBinary,
    #[error("couldn't determine a home directory")]
    NoHome,
}

/// Whether someone else (e.g. `pnpm dev`) manages the daemon.
pub fn daemon_is_external() -> bool {
    std::env::var(EXTERNAL_ENV).is_ok_and(|v| v == "external")
}

/// Finds the daemon binary for the running program: `MIMI_DAEMON_BIN`, then next to the
/// current executable (bundled sidecar, or `target/<profile>/` in development), then
/// `PATH`.
pub fn daemon_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    daemon_binary_from(&exe, std::env::var_os(BIN_ENV), std::env::var_os("PATH"))
}

/// [`daemon_binary`] with its inputs made explicit, for tests.
pub fn daemon_binary_from(
    exe: &Path,
    override_: Option<OsString>,
    path_var: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(p) = override_.map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    // A daemon binary is its own "neighbour"; anything else looks beside itself.
    let beside = exe.with_file_name(DAEMON_BIN);
    if beside.is_file() {
        return Some(beside);
    }
    std::env::split_paths(&path_var?)
        .map(|dir| dir.join(DAEMON_BIN))
        .find(|p| p.is_file())
}

/// How this system starts things at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Systemd,
    Launchd,
    /// An XDG autostart entry: starts at login, but nothing restarts it after a crash.
    Autostart,
}

pub fn manager() -> Manager {
    if cfg!(target_os = "macos") {
        return Manager::Launchd;
    }
    let systemd = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if systemd {
        Manager::Systemd
    } else {
        Manager::Autostart
    }
}

/// Everything needed to describe the service for one data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// systemd unit / autostart entry name, without extension.
    pub name: String,
    /// launchd label.
    pub label: String,
    pub binary: PathBuf,
    /// Arguments for `binary`; empty when it's `mimid` itself.
    pub args: Vec<String>,
    pub data_dir: PathBuf,
    /// Extra environment for the daemon (only what differs from the defaults).
    pub env: Vec<(String, String)>,
}

impl Spec {
    /// The service for this process's data directory (honouring `MIMI_HOME`).
    pub fn current(binary: PathBuf) -> Result<Self, Error> {
        let paths = Paths::resolve().map_err(|_| Error::NoHome)?;
        let custom_home = std::env::var_os(HOME_ENV).map(PathBuf::from);
        let port = std::env::var(PORT_ENV).ok().filter(|p| !p.is_empty());
        let mut spec = Self::new(binary, paths.data_dir, custom_home.is_some(), port);
        // Keep the key storage the user chose (e.g. `file` on a headless server).
        if let Ok(store) = std::env::var(KEY_STORE_ENV) {
            spec.env.push((KEY_STORE_ENV.to_owned(), store));
        }
        Ok(spec)
    }

    pub fn new(
        binary: PathBuf,
        data_dir: PathBuf,
        custom_home: bool,
        port: Option<String>,
    ) -> Self {
        let suffix =
            custom_home.then(|| format!("{:08x}", fnv1a(data_dir.to_string_lossy().as_bytes())));
        let mut env = Vec::new();
        if custom_home {
            env.push((HOME_ENV.to_owned(), data_dir.to_string_lossy().into_owned()));
        }
        if let Some(port) = port {
            env.push((PORT_ENV.to_owned(), port));
        }
        Self {
            name: suffix
                .as_ref()
                .map_or("mimi".to_owned(), |s| format!("mimi-{s}")),
            label: suffix.as_ref().map_or("dev.mimi.daemon".to_owned(), |s| {
                format!("dev.mimi.daemon.{s}")
            }),
            binary,
            args: Vec::new(),
            data_dir,
            env,
        }
    }

    /// Makes the service survive the app being an AppImage. An AppImage runs from a
    /// temporary mount that disappears when it quits, so a service pointing inside it
    /// would break; instead the service runs the AppImage file itself with `--daemon`,
    /// which also keeps working when the AppImage is replaced by a newer one.
    pub fn launched_from_app(mut self) -> Self {
        if let Some((program, args)) = app_launch(std::env::var_os("APPIMAGE")) {
            self.binary = program;
            self.args = args;
        }
        self
    }
}

/// How to start the daemon through the running app, when the app can't be pointed at
/// directly (see [`Spec::launched_from_app`]). `appimage` is the `APPIMAGE` variable the
/// AppImage runtime sets to the image's own path.
pub fn app_launch(appimage: Option<std::ffi::OsString>) -> Option<(PathBuf, Vec<String>)> {
    let image = PathBuf::from(appimage.filter(|v| !v.is_empty())?);
    image
        .is_file()
        .then(|| (image, vec![DAEMON_FLAG.to_owned()]))
}

/// Passed to the desktop app to make it run the bundled daemon instead of its window.
pub const DAEMON_FLAG: &str = "--daemon";

/// 32-bit FNV-1a: a stable, short, dependency-free name for a data directory.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

// --- File contents -----------------------------------------------------------------

/// A systemd user unit that starts the daemon at login and restarts it if it fails.
pub fn systemd_unit(spec: &Spec) -> String {
    let mut out = String::from(
        "[Unit]\n\
         Description=Mimi, your private assistant\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n",
    );
    // ExecStart also expands `$VARIABLES`, so a literal `$` is doubled there.
    let binary = systemd_quote(&spec.binary.to_string_lossy()).replace('$', "$$");
    let args: String = spec
        .args
        .iter()
        .map(|a| format!(" {}", systemd_quote(a).replace('$', "$$")))
        .collect();
    out.push_str(&format!("ExecStart={binary}{args}\n"));
    for (k, v) in &spec.env {
        out.push_str(&format!(
            "Environment={}\n",
            systemd_quote(&format!("{k}={v}"))
        ));
    }
    out.push_str(
        "Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
    );
    out
}

/// systemd's quoting: double quotes, backslash escapes, and `%` doubled so it's never
/// read as a specifier.
fn systemd_quote(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

/// A LaunchAgent that starts the daemon at login and relaunches it after a crash (but
/// not after a clean stop).
pub fn launchd_plist(spec: &Spec) -> String {
    let mut env = String::new();
    if !spec.env.is_empty() {
        env.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
        for (k, v) in &spec.env {
            env.push_str(&format!(
                "    <key>{}</key>\n    <string>{}</string>\n",
                xml_escape(k),
                xml_escape(v)
            ));
        }
        env.push_str("  </dict>\n");
    }
    let log = xml_escape(&spec.data_dir.join("daemon.log").to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
{args}  </array>
{env}  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#,
        label = xml_escape(&spec.label),
        binary = xml_escape(&spec.binary.to_string_lossy()),
        args = spec
            .args
            .iter()
            .map(|a| format!("    <string>{}</string>\n", xml_escape(a)))
            .collect::<String>(),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// An XDG autostart entry, for Linux desktops without a systemd user session.
pub fn autostart_entry(spec: &Spec) -> String {
    let mut exec = String::new();
    if !spec.env.is_empty() {
        exec.push_str("env ");
        for (k, v) in &spec.env {
            exec.push_str(&desktop_quote(&format!("{k}={v}")));
            exec.push(' ');
        }
    }
    exec.push_str(&desktop_quote(&spec.binary.to_string_lossy()));
    for a in &spec.args {
        exec.push(' ');
        exec.push_str(&desktop_quote(a));
    }
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Mimi\n\
         Comment=Keeps your private assistant running\n\
         Exec={exec}\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// Desktop Entry Exec quoting: double quotes with `"`, `` ` ``, `$` and `\` escaped, and
/// `%` doubled (field codes).
fn desktop_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// --- Installing and controlling ----------------------------------------------------

/// What the system says about the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub manager: Manager,
    /// Set up to start at login.
    pub installed: bool,
    /// The service manager reports it running. `None` where it can't tell (autostart).
    pub running: Option<bool>,
}

fn home() -> Result<PathBuf, Error> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(Error::NoHome)
}

fn config_home() -> Result<PathBuf, Error> {
    match std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p),
        _ => Ok(home()?.join(".config")),
    }
}

/// Where the service's file lives for `manager`.
pub fn service_file(manager: Manager, spec: &Spec) -> Result<PathBuf, Error> {
    Ok(match manager {
        Manager::Systemd => config_home()?
            .join("systemd/user")
            .join(format!("{}.service", spec.name)),
        Manager::Launchd => home()?
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", spec.label)),
        Manager::Autostart => config_home()?
            .join("autostart")
            .join(format!("{}.desktop", spec.name)),
    })
}

pub fn status(spec: &Spec) -> Result<Status, Error> {
    let manager = manager();
    let file = service_file(manager, spec)?;
    let (installed, running) = match manager {
        Manager::Systemd => {
            let unit = format!("{}.service", spec.name);
            let enabled = output("systemctl", &["--user", "is-enabled", &unit])
                .is_ok_and(|o| o.trim() == "enabled");
            let active = output("systemctl", &["--user", "is-active", &unit])
                .is_ok_and(|o| o.trim() == "active");
            (file.is_file() && enabled, Some(active))
        }
        Manager::Launchd => {
            let target = format!("gui/{}/{}", uid()?, spec.label);
            let running = output("launchctl", &["print", &target])
                .is_ok_and(|o| o.contains("state = running"));
            (file.is_file(), Some(running))
        }
        Manager::Autostart => (file.is_file(), None),
    };
    Ok(Status {
        manager,
        installed,
        running,
    })
}

/// Sets the service up to start at login, and starts it now.
pub fn install(spec: &Spec) -> Result<Status, Error> {
    let manager = manager();
    let file = service_file(manager, spec)?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    match manager {
        Manager::Systemd => {
            fs::write(&file, systemd_unit(spec))?;
            run("systemctl", &["--user", "daemon-reload"])?;
            run(
                "systemctl",
                &[
                    "--user",
                    "enable",
                    "--now",
                    &format!("{}.service", spec.name),
                ],
            )?;
        }
        Manager::Launchd => {
            fs::write(&file, launchd_plist(spec))?;
            let domain = format!("gui/{}", uid()?);
            // Replace any previous definition, then load and allow it at login.
            let _ = run(
                "launchctl",
                &["bootout", &format!("{domain}/{}", spec.label)],
            );
            run(
                "launchctl",
                &["bootstrap", &domain, &file.to_string_lossy()],
            )?;
            let _ = run(
                "launchctl",
                &["enable", &format!("{domain}/{}", spec.label)],
            );
        }
        Manager::Autostart => {
            fs::write(&file, autostart_entry(spec))?;
            spawn_detached(spec)?;
        }
    }
    status(spec)
}

/// Stops the service and removes it from login. The daemon stops with it.
pub fn uninstall(spec: &Spec) -> Result<(), Error> {
    let manager = manager();
    let file = service_file(manager, spec)?;
    match manager {
        Manager::Systemd => {
            let unit = format!("{}.service", spec.name);
            let _ = run("systemctl", &["--user", "disable", "--now", &unit]);
            remove_if_present(&file)?;
            run("systemctl", &["--user", "daemon-reload"])?;
        }
        Manager::Launchd => {
            let _ = run(
                "launchctl",
                &["bootout", &format!("gui/{}/{}", uid()?, spec.label)],
            );
            remove_if_present(&file)?;
        }
        Manager::Autostart => {
            remove_if_present(&file)?;
            let paths = Paths {
                data_dir: spec.data_dir.clone(),
                config_dir: spec.data_dir.clone(),
            };
            stop_daemon(&paths);
        }
    }
    Ok(())
}

/// Starts an installed service now (e.g. the app opened and the daemon isn't up).
pub fn start(spec: &Spec) -> Result<(), Error> {
    match manager() {
        Manager::Systemd => run(
            "systemctl",
            &["--user", "start", &format!("{}.service", spec.name)],
        ),
        Manager::Launchd => run(
            "launchctl",
            &["kickstart", &format!("gui/{}/{}", uid()?, spec.label)],
        ),
        Manager::Autostart => spawn_detached(spec),
    }
}

/// Starts the daemon in its own process group, so it outlives whoever started it.
pub fn spawn_detached(spec: &Spec) -> Result<(), Error> {
    use std::os::unix::process::CommandExt;
    Command::new(&spec.binary)
        .args(&spec.args)
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|e| Error::Command(format!("couldn't start {}: {e}", spec.binary.display())))
}

/// Asks the daemon running for `paths` (per its discovery file) to shut down cleanly.
pub fn stop_daemon(paths: &Paths) -> bool {
    let Ok(raw) = fs::read(paths.discovery_file()) else {
        return false;
    };
    match serde_json::from_slice::<mimi_protocol::Discovery>(&raw) {
        Ok(discovery) => terminate(discovery.pid),
        Err(_) => false,
    }
}

/// Sends SIGTERM to `pid`, letting the daemon save its state and remove its discovery
/// file. `kill(1)` avoids unsafe code.
pub fn terminate(pid: u32) -> bool {
    Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn remove_if_present(file: &Path) -> Result<(), Error> {
    match fs::remove_file(file) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn uid() -> Result<String, Error> {
    output("id", &["-u"]).map(|s| s.trim().to_owned())
}

fn output(program: &str, args: &[&str]) -> Result<String, Error> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Command(format!("couldn't run {program}: {e}")))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run(program: &str, args: &[&str]) -> Result<(), Error> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Command(format!("couldn't run {program}: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(Error::Command(format!(
            "{program} {} failed: {}",
            args.join(" "),
            err.trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appimage_services_run_the_image_itself() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Mimi.AppImage");
        std::fs::write(&image, b"").unwrap();
        assert_eq!(
            app_launch(Some(image.clone().into_os_string())),
            Some((image.clone(), vec!["--daemon".to_owned()]))
        );
        assert_eq!(app_launch(None), None);
        assert_eq!(app_launch(Some("".into())), None);
        assert_eq!(app_launch(Some("/gone/Mimi.AppImage".into())), None);

        let mut s = spec(false);
        s.binary = image.clone();
        s.args = vec!["--daemon".to_owned()];
        assert!(
            systemd_unit(&s).contains(&format!("ExecStart=\"{}\" \"--daemon\"\n", image.display()))
        );
        assert!(launchd_plist(&s).contains("<string>--daemon</string>"));
        assert!(autostart_entry(&s).contains("\" \"--daemon\"\n"));
    }

    fn spec(custom: bool) -> Spec {
        Spec::new(
            "/opt/Mimi App/mimid".into(),
            "/home/me/.local/share/mimi".into(),
            custom,
            None,
        )
    }

    #[test]
    fn default_install_uses_the_plain_names() {
        let s = spec(false);
        assert_eq!(s.name, "mimi");
        assert_eq!(s.label, "dev.mimi.daemon");
        assert!(s.env.is_empty());
    }

    #[test]
    fn custom_homes_never_clash_with_the_real_install() {
        let a = Spec::new(
            "/b/mimid".into(),
            "/home/me/project/.dev".into(),
            true,
            Some("7490".into()),
        );
        let b = Spec::new("/b/mimid".into(), "/tmp/other".into(), true, None);
        assert!(
            a.name.starts_with("mimi-") && a.name.len() == 13,
            "{}",
            a.name
        );
        assert_ne!(a.name, b.name);
        assert!(a.label.starts_with("dev.mimi.daemon."));
        assert_eq!(
            a.env,
            vec![
                ("MIMI_HOME".to_owned(), "/home/me/project/.dev".to_owned()),
                ("MIMI_PORT".to_owned(), "7490".to_owned())
            ]
        );
        // Stable across runs.
        assert_eq!(
            a.name,
            Spec::new("/x".into(), "/home/me/project/.dev".into(), true, None).name
        );
    }

    #[test]
    fn systemd_unit_quotes_paths_and_restarts_on_failure() {
        let mut s = spec(true);
        s.env.push(("X".into(), "50% of $HOME".into()));
        let unit = systemd_unit(&s);
        assert!(
            unit.contains("ExecStart=\"/opt/Mimi App/mimid\"\n"),
            "{unit}"
        );
        assert!(unit.contains("Environment=\"MIMI_HOME=/home/me/.local/share/mimi\"\n"));
        assert!(unit.contains("Environment=\"X=50%% of $HOME\"\n"));
        let odd = Spec::new("/opt/$weird/mimid".into(), "/d".into(), false, None);
        assert!(systemd_unit(&odd).contains("ExecStart=\"/opt/$$weird/mimid\"\n"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn launchd_plist_restarts_after_crashes_only() {
        let mut s = spec(false);
        s.binary = "/Applications/Mimi.app/Contents/MacOS/mimid".into();
        let plist = launchd_plist(&s);
        assert!(plist.contains("<string>dev.mimi.daemon</string>"));
        assert!(plist.contains("<string>/Applications/Mimi.app/Contents/MacOS/mimid</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(!plist.contains("EnvironmentVariables"));

        let s = Spec::new("/a&b/mimid".into(), "/tmp/<x>".into(), true, None);
        let plist = launchd_plist(&s);
        assert!(plist.contains("<string>/a&amp;b/mimid</string>"));
        assert!(plist.contains("<key>MIMI_HOME</key>\n    <string>/tmp/&lt;x&gt;</string>"));
    }

    #[test]
    fn autostart_entry_quotes_exec() {
        let s = spec(true);
        let entry = autostart_entry(&s);
        assert!(
            entry.contains(
                "Exec=env \"MIMI_HOME=/home/me/.local/share/mimi\" \"/opt/Mimi App/mimid\"\n"
            ),
            "{entry}"
        );
        assert_eq!(desktop_quote("a$b%c\"d"), "\"a\\$b%%c\\\"d\"");
    }

    #[test]
    fn finds_the_daemon_beside_the_app_then_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("mimi-desktop");
        fs::write(&app, "").unwrap();
        // Nothing beside it, nothing on PATH.
        assert_eq!(daemon_binary_from(&app, None, Some("".into())), None);

        let path_dir = tempfile::tempdir().unwrap();
        fs::write(path_dir.path().join("mimid"), "").unwrap();
        assert_eq!(
            daemon_binary_from(&app, None, Some(path_dir.path().as_os_str().to_owned())),
            Some(path_dir.path().join("mimid"))
        );

        // A sidecar (or target/<profile>/mimid) beside the app wins over PATH.
        fs::write(dir.path().join("mimid"), "").unwrap();
        assert_eq!(
            daemon_binary_from(&app, None, Some(path_dir.path().as_os_str().to_owned())),
            Some(dir.path().join("mimid"))
        );

        // An explicit override wins over everything, if it exists.
        let custom = dir.path().join("custom-mimid");
        fs::write(&custom, "").unwrap();
        assert_eq!(
            daemon_binary_from(&app, Some(custom.clone().into()), None),
            Some(custom)
        );
        assert_eq!(
            daemon_binary_from(&app, Some("/nope".into()), None),
            Some(dir.path().join("mimid"))
        );
    }
}
