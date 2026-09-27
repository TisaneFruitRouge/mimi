//! The database encryption key. Kept in the OS keychain; where there is none (e.g. a
//! headless server) it falls back to an owner-only file, and `Status` reports which.

use std::fs;

use anyhow::bail;
use mimi_protocol::{KeyStorage, Paths};

use crate::fsutil::{random_hex, write_private};

/// Set to `file` to skip the keychain, e.g. for throwaway development instances.
pub const KEY_STORE_ENV: &str = "MIMI_KEY_STORE";

const KEYCHAIN_SERVICE: &str = "mimi";

/// The keychain service used before the project was renamed from Hearth to Mimi. Keys
/// found there are moved to [`KEYCHAIN_SERVICE`] on first use.
const LEGACY_KEYCHAIN_SERVICE: &str = "hearth";

pub struct DbKey {
    pub hex: String,
    pub storage: KeyStorage,
}

/// Loads the key for an existing database, or creates one for a new database.
/// Blocking: the keychain may show an unlock prompt.
pub fn load_or_create(paths: &Paths, db_exists: bool) -> anyhow::Result<DbKey> {
    let file = paths.data_dir.join("db.key");
    let entry = keychain_entry(paths);

    if let Some(entry) = &entry {
        match entry.get_password() {
            Ok(hex) => {
                return Ok(DbKey {
                    hex,
                    storage: KeyStorage::Keychain,
                });
            }
            Err(keyring::Error::NoEntry) => {
                if let Some(hex) = take_legacy_key(paths, entry) {
                    return Ok(DbKey {
                        hex,
                        storage: KeyStorage::Keychain,
                    });
                }
            }
            Err(e) => tracing::warn!("could not read the key from the keychain: {e}"),
        }
    }
    if let Ok(hex) = fs::read_to_string(&file) {
        return Ok(DbKey {
            hex: hex.trim().to_owned(),
            storage: KeyStorage::File,
        });
    }
    if db_exists {
        bail!(
            "the database exists but its encryption key is neither in the keychain nor in {}",
            file.display()
        );
    }

    let hex = random_hex(32)?;
    if let Some(entry) = &entry {
        match entry.set_password(&hex) {
            Ok(()) => {
                return Ok(DbKey {
                    hex,
                    storage: KeyStorage::Keychain,
                });
            }
            Err(e) => tracing::warn!("could not save the key to the keychain: {e}"),
        }
    }
    tracing::warn!(
        "no usable keychain; storing the database key in {} (owner-only)",
        file.display()
    );
    write_private(&file, hex.as_bytes())?;
    Ok(DbKey {
        hex,
        storage: KeyStorage::File,
    })
}

/// Moves a key stored under the pre-rename service name to the current one.
fn take_legacy_key(paths: &Paths, entry: &keyring::Entry) -> Option<String> {
    let legacy = keyring::Entry::new(LEGACY_KEYCHAIN_SERVICE, &keychain_user(paths)).ok()?;
    let hex = legacy.get_password().ok()?;
    match entry.set_password(&hex) {
        Ok(()) => {
            let _ = legacy.delete_credential();
            tracing::info!("moved the database key to its new keychain name");
        }
        // Still usable this time; the move is retried on the next start.
        Err(e) => tracing::warn!("could not move the database key in the keychain: {e}"),
    }
    Some(hex)
}

fn keychain_user(paths: &Paths) -> String {
    // One entry per data directory, so separate instances never share a key.
    format!("database-key:{}", paths.data_dir.display())
}

fn keychain_entry(paths: &Paths) -> Option<keyring::Entry> {
    if std::env::var(KEY_STORE_ENV).is_ok_and(|v| v == "file") {
        return None;
    }
    match keyring::Entry::new(KEYCHAIN_SERVICE, &keychain_user(paths)) {
        Ok(entry) => Some(entry),
        Err(e) => {
            tracing::warn!("keychain unavailable: {e}");
            None
        }
    }
}
