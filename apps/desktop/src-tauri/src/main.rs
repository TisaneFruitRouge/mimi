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
    let Some(daemon) = daemon else {
        eprintln!("mimid isn't bundled next to this app");
        std::process::exit(1);
    };
    let err = std::process::Command::new(&daemon)
        .args(std::env::args().skip(2))
        .exec();
    eprintln!("couldn't start {}: {err}", daemon.display());
    std::process::exit(1);
}
