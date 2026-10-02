//! Voice files: one archive per recognizer or voice pack, downloaded like models
//! (resumable, checked against the catalog's SHA-256), then unpacked into
//! `<data>/voice/<id>/`. That folder appears only once everything is in it, so its
//! existence means "ready".

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::catalog::{self, Archive};
use crate::AppState;
use crate::runtime::download::{Fetch, fetch};

/// Where the catalog's archives live; tests point [`super::Voice::base`] elsewhere.
pub const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";

pub fn voice_dir(paths: &mimi_protocol::Paths) -> PathBuf {
    paths.data_dir.join("voice")
}

pub fn pack_dir(paths: &mimi_protocol::Paths, id: &str) -> PathBuf {
    voice_dir(paths).join(id)
}

pub fn is_ready(paths: &mimi_protocol::Paths, id: &str) -> bool {
    pack_dir(paths, id).is_dir()
}

/// Starts downloading a pack, unless it's ready or already on its way.
pub fn start(state: &Arc<AppState>, id: &str) -> Result<(), String> {
    let archive = catalog::archive(id).ok_or("Mimi doesn't know where to download that from.")?;
    if is_ready(&state.paths, id) {
        return Ok(());
    }
    let cancel = CancellationToken::new();
    {
        let mut downloads = state.voice.downloads();
        if downloads.contains_key(id) {
            return Ok(());
        }
        downloads.insert(id.to_owned(), (cancel.clone(), 0));
    }
    state.voice.errors().remove(id);
    super::publish(state);
    let state = state.clone();
    let id = id.to_owned();
    tokio::spawn(async move {
        let outcome = run(&state, &id, archive, &cancel).await;
        state.voice.downloads().remove(&id);
        match outcome {
            Ok(()) => tracing::info!(pack = %id, "voice files downloaded"),
            Err(_) if cancel.is_cancelled() => {}
            Err(e) => {
                tracing::warn!(pack = %id, "voice download failed: {e}");
                state.voice.errors().insert(id.clone(), e);
            }
        }
        super::publish(&state);
    });
    Ok(())
}

/// Stops a download. Returns whether one was running.
pub fn cancel(state: &AppState, id: &str) -> bool {
    state
        .voice
        .downloads()
        .get(id)
        .map(|(c, _)| c.cancel())
        .is_some()
}

/// Deletes a pack and anything left of its download.
pub fn remove(paths: &mimi_protocol::Paths, id: &str) -> std::io::Result<()> {
    let dir = voice_dir(paths);
    for path in [
        dir.join(id),
        dir.join(format!("{id}.unpacking")),
        dir.join(format!("{id}.tar.bz2")),
        dir.join(format!("{id}.tar.bz2.part")),
    ] {
        let removed = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        match removed {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

async fn run(
    state: &Arc<AppState>,
    id: &str,
    archive: &Archive,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let dir = voice_dir(&state.paths);
    let file = dir.join(format!("{id}.tar.bz2"));
    let url = match state.voice.base() {
        Some(base) => archive.url.replacen(RELEASES, &base, 1),
        None => archive.url.clone(),
    };
    let target = Fetch {
        url,
        part: dir.join(format!("{id}.tar.bz2.part")),
        dest: file.clone(),
        bytes: archive.bytes,
        sha256: &archive.sha256,
        host: "GitHub",
        what: "voice",
    };
    // Room for the archive and what it unpacks to.
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the voice folder: {e}"))?;
    crate::runtime::download::check_space(&dir, archive.bytes * 3, "voice")?;
    let mut last = 0;
    fetch(&state.http, &target, cancel, |_, done| {
        state.voice.set_progress(id, done);
        // `fetch` reports at most four times a second.
        if done != last {
            last = done;
            super::publish(state);
        }
    })
    .await?;
    let into = pack_dir(&state.paths, id);
    tokio::task::spawn_blocking(move || {
        let unpacked = unpack(&file, &into);
        let _ = fs::remove_file(&file);
        unpacked
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Unpacks a `.tar.bz2` into `into`, without its top folder, and only then moves it into
/// place. Only plain files and folders, and only inside `into`.
fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("Couldn't unpack the voice files: {e}");
    let mut staging = into.as_os_str().to_owned();
    staging.push(".unpacking");
    let staging = PathBuf::from(staging);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(fail)?;
    let reader = bzip2::read::BzDecoder::new(fs::File::open(archive).map_err(fail)?);
    let mut tar = tar::Archive::new(reader);
    for entry in tar.entries().map_err(fail)? {
        let mut entry = entry.map_err(fail)?;
        let path = entry.path().map_err(fail)?.into_owned();
        let mut parts = path.components();
        // The archive's own folder.
        parts.next();
        let rest: PathBuf = parts.as_path().to_path_buf();
        if rest.as_os_str().is_empty() {
            continue;
        }
        if !rest.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err("The voice files are damaged. Try again.".to_owned());
        }
        let dest = staging.join(&rest);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&dest).map_err(fail)?;
        } else if kind.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(fail)?;
            }
            // Streamed: some model files are larger than a small computer's free memory.
            let mut out = fs::File::create(&dest).map_err(fail)?;
            std::io::copy(&mut entry, &mut out).map_err(fail)?;
        }
        // Links and anything else are left out.
    }
    let _ = fs::remove_dir_all(into);
    fs::rename(&staging, into).map_err(fail)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    /// A `.tar.bz2` with `top/<name>` for each file given.
    pub(crate) fn archive_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("top/{name}"), *data)
                .unwrap();
        }
        let tar = tar.into_inner().unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&tar).unwrap();
        bz.finish().unwrap()
    }

    #[test]
    fn archives_unpack_without_their_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pack.tar.bz2");
        fs::write(
            &file,
            archive_of(&[("model.onnx", b"m"), ("espeak-ng-data/en_dict", b"d")]),
        )
        .unwrap();
        let into = dir.path().join("pack");
        unpack(&file, &into).unwrap();
        assert_eq!(fs::read(into.join("model.onnx")).unwrap(), b"m");
        assert_eq!(fs::read(into.join("espeak-ng-data/en_dict")).unwrap(), b"d");
        assert!(!dir.path().join("pack.unpacking").exists());
    }

    #[test]
    fn nothing_is_written_outside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pack.tar.bz2");
        // `tar::Builder` refuses `..` itself, so write the header by hand.
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        {
            let name = &mut header.as_gnu_mut().unwrap().name;
            let path = b"top/../../escaped";
            name[..path.len()].copy_from_slice(path);
        }
        header.set_cksum();
        let mut tar = tar::Builder::new(Vec::new());
        tar.append(&header, &b"x"[..]).unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&tar.into_inner().unwrap()).unwrap();
        fs::write(&file, bz.finish().unwrap()).unwrap();

        let into = dir.path().join("a/pack");
        fs::create_dir_all(dir.path().join("a")).unwrap();
        assert!(unpack(&file, &into).is_err());
        assert!(!dir.path().join("escaped").exists());
        assert!(!into.exists());
    }
}
