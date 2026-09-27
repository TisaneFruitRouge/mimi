use std::path::PathBuf;

use directories::ProjectDirs;

/// Overrides every mimi directory. Used for development and tests so several
/// instances can run side by side.
pub const HOME_ENV: &str = "MIMI_HOME";

#[derive(Debug, thiserror::Error)]
#[error("could not determine a home directory for mimi data")]
pub struct NoHomeDir;

/// Where mimi keeps its files on this machine.
///
/// Linux: `~/.local/share/mimi` and `~/.config/mimi`.
/// macOS: `~/Library/Application Support/dev.mimi.mimi` for both.
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self, NoHomeDir> {
        if let Some(home) = std::env::var_os(HOME_ENV) {
            let home = PathBuf::from(home);
            return Ok(Self {
                data_dir: home.clone(),
                config_dir: home,
            });
        }
        let dirs = ProjectDirs::from("dev", "mimi", "mimi").ok_or(NoHomeDir)?;
        Ok(Self {
            data_dir: dirs.data_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
        })
    }

    pub fn discovery_file(&self) -> PathBuf {
        self.data_dir.join("daemon.json")
    }
}
