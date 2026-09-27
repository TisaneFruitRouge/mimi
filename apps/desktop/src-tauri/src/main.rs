// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // A login service started from an AppImage runs the image with `--daemon` (its
    // files only exist while it runs); hand over to the bundled daemon.
    if std::env::args().nth(1).as_deref() == Some(mimi_service::DAEMON_FLAG) {
        run_bundled_daemon();
    }
    mimi_desktop_lib::run()
}

fn run_bundled_daemon() -> ! {
    use std::os::unix::process::CommandExt;
    let daemon = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("mimid")))
        .filter(|p| p.is_file());
    let Some(mut daemon) = daemon else {
        eprintln!("mimid isn't bundled next to this app");
        std::process::exit(1);
    };
    // An AppImage's files vanish the moment its mount goes, which a service manager
    // stopping everything at once does before the daemon has finished shutting down.
    // Run a copy that stays put instead.
    if std::env::var_os("APPIMAGE").is_some() {
        match outside_the_image(&daemon) {
            Ok(copy) => daemon = copy,
            Err(e) => eprintln!(
                "couldn't copy the assistant out of the AppImage ({e}); running it in place"
            ),
        }
    }
    let err = std::process::Command::new(&daemon)
        .args(std::env::args().skip(2))
        .exec();
    eprintln!("couldn't start {}: {err}", daemon.display());
    std::process::exit(1);
}

/// Copies `mimid` and the model runtime beside it (the bundle's `../lib/<app>/llama`)
/// to `<data>/app/<build>/`, once per build, and returns the copy of `mimid`. Other
/// builds' copies are removed.
fn outside_the_image(daemon: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::fs;
    use std::time::UNIX_EPOCH;
    let paths = mimi_protocol::Paths::resolve().map_err(std::io::Error::other)?;
    let meta = fs::metadata(daemon)?;
    let built = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let build = format!("{}-{}-{built}", env!("CARGO_PKG_VERSION"), meta.len());
    let apps = paths.data_dir.join("app");
    let target = apps.join(&build);
    if !target.join("mimid").is_file() {
        let staging = apps.join(format!(".{build}.{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging)?;
        fs::copy(daemon, staging.join("mimid"))?;
        let bin = daemon.parent().unwrap_or(std::path::Path::new("."));
        let llama = fs::read_dir(bin.join("../lib"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|app| app.path().join("llama"))
            .chain([bin.join("llama")])
            .find(|p| p.is_dir());
        if let Some(llama) = llama {
            copy_dir(&llama, &staging.join("llama"))?;
        }
        let _ = fs::remove_dir_all(&target);
        fs::rename(&staging, &target)?;
    }
    for old in fs::read_dir(&apps)?.flatten() {
        if old.file_name() != std::ffi::OsStr::new(&build) {
            let _ = fs::remove_dir_all(old.path());
        }
    }
    Ok(target.join("mimid"))
}

/// Copies a folder, keeping symlinks as symlinks (shared libraries use them).
fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
        } else if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}
