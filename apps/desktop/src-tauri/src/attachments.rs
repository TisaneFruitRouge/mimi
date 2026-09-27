//! Email attachments on disk: safe file names, and which files are programs that must
//! never be opened straight from an email.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// Where opened attachments are written. Emptied when the app starts.
pub fn cache_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app.path().app_cache_dir()?.join("attachments"))
}

/// A file name that stays inside the folder it's written to: no separators, no
/// leading dots, no control characters, not too long.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        return "attachment".to_owned();
    }
    if cleaned.chars().count() <= 120 {
        return cleaned;
    }
    // Keep the extension when shortening.
    let ext = Path::new(&cleaned)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .filter(|e| e.chars().count() <= 10);
    let stem: String = cleaned.chars().take(100).collect();
    match ext {
        Some(e) => format!("{stem}.{e}"),
        None => stem,
    }
}

/// Programs, scripts, installers and launchers: opening one from an email could run
/// it. Files without an extension count too (they could be executables).
pub fn is_program(name: &str) -> bool {
    const PROGRAMS: &[&str] = &[
        "desktop",
        "sh",
        "bash",
        "zsh",
        "fish",
        "csh",
        "ksh",
        "run",
        "bin",
        "appimage",
        "flatpakref",
        "flatpak",
        "snap",
        "deb",
        "rpm",
        "pkg",
        "apk",
        "exe",
        "msi",
        "bat",
        "cmd",
        "com",
        "scr",
        "ps1",
        "psm1",
        "vbs",
        "vbe",
        "js",
        "jse",
        "wsf",
        "wsh",
        "hta",
        "cpl",
        "reg",
        "lnk",
        "jar",
        "py",
        "pyw",
        "pl",
        "rb",
        "php",
        "command",
        "tool",
        "app",
        "dmg",
        "iso",
        "img",
        "workflow",
        "scpt",
        "applescript",
        "action",
        "elf",
        "out",
        "so",
        "dll",
        "xpi",
        "crx",
        "service",
        "timer",
    ];
    match Path::new(name).extension() {
        Some(ext) => PROGRAMS.contains(&ext.to_string_lossy().to_lowercase().as_str()),
        None => true,
    }
}

/// `dir/name`, or `dir/name (2)`… if that's taken, so nothing in Downloads is replaced.
pub fn unused_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map_or_else(|| name.to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy()));
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){}", ext.as_deref().unwrap_or(""))))
        .find(|p| !p.exists())
        .expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_in_their_folder() {
        assert_eq!(safe_name("../../.bashrc"), "_.._.bashrc");
        assert_eq!(safe_name(".hidden"), "hidden");
        assert_eq!(safe_name("a/b\\c\u{0}.pdf"), "a_b_c_.pdf");
        assert_eq!(safe_name("   "), "attachment");
        let long = format!("{}.pdf", "x".repeat(300));
        assert!(safe_name(&long).ends_with(".pdf") && safe_name(&long).chars().count() < 120);
    }

    #[test]
    fn programs_are_recognised() {
        for name in [
            "Invoice.desktop",
            "setup.SH",
            "Mimi.AppImage",
            "run.exe",
            "README",
            "x.jar",
        ] {
            assert!(is_program(name), "{name}");
        }
        for name in [
            "Menu été.pdf",
            "photo.JPG",
            "notes.txt",
            "table.xlsx",
            "page.html",
        ] {
            assert!(!is_program(name), "{name}");
        }
    }

    #[test]
    fn nothing_in_downloads_is_replaced() {
        let dir = std::env::temp_dir().join(format!("mimi-att-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.sh"), "").unwrap();
        std::fs::write(dir.join("a (2).sh"), "").unwrap();
        assert_eq!(unused_path(&dir, "a.sh"), dir.join("a (3).sh"));
        assert_eq!(unused_path(&dir, "b.sh"), dir.join("b.sh"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
