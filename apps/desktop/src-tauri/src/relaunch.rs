//! Opening Mimi after installing a new version starts the new version.
//!
//! In background mode, closing the window only hides it, so the next launch reaches the
//! app that's still running (single instance) and would show the old version forever.
//! Instead, the app remembers the file it was started from, and when it's opened again
//! and that file has been replaced, it hands over to the new program.

use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The file this app was started from, and what it looked like then.
pub struct Launched {
    path: PathBuf,
    stamp: Option<Stamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: i64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        dev: meta.dev(),
        ino: meta.ino(),
        len: meta.len(),
        mtime: meta.mtime(),
    })
}

impl Launched {
    /// Call at startup, before anything could have replaced the program.
    pub fn now() -> Self {
        // An AppImage runs from a temporary mount; the file the user installed is $APPIMAGE.
        let path = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_default();
        let stamp = stamp(&path);
        Self { path, stamp }
    }

    /// Whether another program has been installed where this one was started from.
    pub fn replaced(&self) -> bool {
        matches!((self.stamp, stamp(&self.path)), (Some(then), Some(now)) if then != now)
    }

    /// Starts the new program once this one has exited. The caller exits right after.
    pub fn hand_over(&self) -> std::io::Result<()> {
        // What to start: the app bundle on macOS (so it's a proper app launch), the file
        // itself elsewhere.
        let (program, target) = match self
            .path
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
        {
            Some(bundle) if cfg!(target_os = "macos") => ("open", bundle.to_path_buf()),
            _ => ("exec", self.path.clone()),
        };
        // The new instance must not start while this one still holds the single-instance
        // lock, or it would hand itself back to us: a small shell waits for us to go.
        let script = format!(
            r#"i=0; while kill -0 "$1" 2>/dev/null && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; {program} "$2""#
        );
        Command::new("/bin/sh")
            .args(["-c", &script, "sh", &std::process::id().to_string()])
            .arg(&target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_replaced_in_place_counts_as_a_new_program() {
        let dir = std::env::temp_dir().join(format!("mimi-relaunch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Mimi.AppImage");
        std::fs::write(&file, b"old").unwrap();
        let launched = Launched {
            stamp: stamp(&file),
            path: file.clone(),
        };
        assert!(!launched.replaced());

        // What installers do: write a new file, then move it over the old one.
        let new = dir.join("new");
        std::fs::write(&new, b"newer").unwrap();
        std::fs::rename(&new, &file).unwrap();
        assert!(launched.replaced());

        // A missing file (mid-install) is not a reason to restart.
        std::fs::remove_file(&file).unwrap();
        assert!(!launched.replaced());
        let _ = std::fs::remove_dir_all(dir);
    }
}
