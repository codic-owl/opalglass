//! Durable, bounded saved-note snapshots and resumable bulk-edit recovery.
//! Hidden vault storage is never included in note discovery or search.
use crate::vault::Vault;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn directory(root: &Path, parts: &[&str], create: bool) -> io::Result<PathBuf> {
    let mut path = root.to_owned();
    for part in std::iter::once(".opalglass-history").chain(parts.iter().copied()) {
        path.push(part);
        if create {
            match fs::create_dir(&path) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(invalid(
                "History must use real directories inside the vault",
            ));
        }
    }
    Ok(path)
}
fn regular(path: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(invalid("History records must be regular files"));
    }
    Ok(())
}
fn publish(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(&serde_json::to_vec(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
fn note_key(path: &Path) -> String {
    hash(&path.to_string_lossy().replace('\\', "/"))
}
#[derive(Serialize, Deserialize)]
struct Snapshot {
    path: PathBuf,
    text: String,
}
#[derive(Clone, Debug)]
pub struct Version {
    pub id: String,
    pub modified: SystemTime,
    pub bytes: u64,
}
impl Version {
    pub fn label(&self) -> String {
        let date = time::OffsetDateTime::from(self.modified);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            date.year(),
            date.month() as u8,
            date.day(),
            date.hour(),
            date.minute(),
            date.second()
        )
    }
}
pub fn versions(root: &Path, path: &Path) -> io::Result<Vec<Version>> {
    let directory = match directory(root, &["notes", &note_key(path)], false) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut versions = vec![];
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| valid_id(s))
        else {
            continue;
        };
        if path.extension().is_none_or(|s| s != "json") {
            continue;
        }
        regular(&path)?;
        let metadata = entry.metadata()?;
        versions.push(Version {
            id: id.into(),
            modified: metadata.modified()?,
            bytes: metadata.len(),
        });
    }
    versions.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
    Ok(versions)
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn read(root: &Path, path: &Path, id: &str) -> io::Result<String> {
    if !valid_id(id) {
        return Err(invalid("Invalid history version"));
    }
    let file = directory(root, &["notes", &note_key(path)], false)?.join(format!("{id}.json"));
    regular(&file)?;
    let record: Snapshot = serde_json::from_slice(&fs::read(file)?)?;
    if record.path != path || hash(&record.text) != id {
        return Err(invalid("History record does not match this note"));
    }
    Ok(record.text)
}
pub(crate) fn capture(root: &Path, path: &Path, text: &str) -> io::Result<()> {
    let folder = directory(root, &["notes", &note_key(path)], true)?;
    let id = hash(text);
    let record = folder.join(format!("{id}.json"));
    match fs::symlink_metadata(&record) {
        Ok(_) => {
            read(root, path, &id)?;
            return Ok(());
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    publish(
        &record,
        &Snapshot {
            path: path.into(),
            text: text.into(),
        },
    )?;
    // Keep the just-captured version plus at most 99 others, bounded to 64 MiB
    // per note (one oversized version is retained instead of silently lost).
    let mut bytes = record.metadata()?.len();
    let mut count = 1;
    for version in versions(root, path)?.into_iter().filter(|v| v.id != id) {
        bytes = bytes.saturating_add(version.bytes);
        count += 1;
        if count > 100 || bytes > 64 * 1024 * 1024 {
            fs::remove_file(folder.join(format!("{}.json", version.id)))?;
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BatchEntry {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}
#[derive(Serialize, Deserialize)]
struct Batch {
    entries: Vec<BatchEntry>,
    undone: bool,
}
pub(crate) fn checkpoint(root: &Path, entries: Vec<BatchEntry>) -> io::Result<()> {
    let folder = directory(root, &["batches"], true)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    publish(
        &folder.join(format!("{stamp:030}.json")),
        &Batch {
            entries,
            undone: false,
        },
    )
}
/// Checks every file before restoring any. Interrupted undo can be retried:
/// records already matching their old content are skipped, and outside edits
/// are never overwritten.
pub fn undo_latest(vault: &mut Vault) -> Result<usize, String> {
    let folder = directory(&vault.root, &["batches"], false).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            "No bulk replacement to undo".into()
        } else {
            e.to_string()
        }
    })?;
    let mut files = fs::read_dir(folder)
        .map_err(|e| e.to_string())?
        // Do not restore an older batch after silently missing a newer record.
        .collect::<io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect::<Vec<_>>();
    files.sort();
    for file in files.into_iter().rev() {
        regular(&file).map_err(|e| e.to_string())?;
        let mut batch: Batch = serde_json::from_slice(&fs::read(&file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if batch.undone {
            continue;
        }
        let mut restore = vec![];
        for entry in &batch.entries {
            let disk = vault
                .read(&entry.path)
                .map_err(|e| format!("{}: {e}", entry.path.display()))?;
            if disk == entry.before {
                continue;
            }
            if hash(&disk) != entry.after {
                return Err(format!(
                    "{} changed after the replacement. Undo stopped without overwriting it; saved versions remain available.",
                    entry.path.display()
                ));
            }
            restore.push((entry, disk));
        }
        for (entry, disk) in &restore {
            vault.save(&entry.path, disk, &entry.before).map_err(|e| {
                format!(
                    "Undo stopped at {}: {e}. Retry to resume.",
                    entry.path.display()
                )
            })?;
        }
        let count = restore.len();
        batch.undone = true;
        crate::state::write_json(&file, &batch).map_err(|e| e.to_string())?;
        return Ok(count);
    }
    Err("No bulk replacement to undo".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_versions_survive_restart_and_validate_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = Path::new("A.md");
        fs::write(root.path().join(path), "Original 👩‍💻\r\n").unwrap();
        let mut vault = Vault::open(root.path()).unwrap();
        vault.save(path, "Original 👩‍💻\r\n", "Second").unwrap();
        vault.save(path, "Second", "Third").unwrap();
        let reopened = Vault::open(root.path()).unwrap();
        assert_eq!(reopened.notes.len(), 1);
        let list = versions(&reopened.root, path).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            read(&reopened.root, path, &hash("Original 👩‍💻\r\n")).unwrap(),
            "Original 👩‍💻\r\n"
        );
        assert!(read(&reopened.root, path, "../A.md").is_err());
        assert!(vault.save(path, "stale", "overwrite").is_err());
        assert_eq!(versions(&reopened.root, path).unwrap().len(), 2);
    }
    #[test]
    fn bulk_recovery_survives_restart_and_refuses_unrelated_changes() {
        let root = tempfile::tempdir().unwrap();
        for name in ["A.md", "B.md"] {
            fs::write(root.path().join(name), "before\r\n").unwrap();
        }
        let mut vault = Vault::open(root.path()).unwrap();
        let plans = crate::changes::replace_preview(
            &vault,
            "before",
            "after",
            true,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(crate::changes::apply_batch(&mut vault, &plans), (2, None));
        drop(vault);
        let mut vault = Vault::open(root.path()).unwrap();
        fs::write(root.path().join("B.md"), "outside edit").unwrap();
        assert!(undo_latest(&mut vault).is_err());
        assert_eq!(vault.read(Path::new("A.md")).unwrap(), "after\r\n");
        fs::write(root.path().join("B.md"), "after\r\n").unwrap();
        // Simulate an interrupted restore that finished A before a crash.
        fs::write(root.path().join("A.md"), "before\r\n").unwrap();
        assert_eq!(undo_latest(&mut vault).unwrap(), 1);
        assert_eq!(vault.read(Path::new("B.md")).unwrap(), "before\r\n");
        assert!(undo_latest(&mut vault).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn history_symlinks_abort_before_note_write() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(root.path().join("A.md"), "original").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join(".opalglass-history")).unwrap();
        let mut vault = Vault::open(root.path()).unwrap();
        assert!(vault.save(Path::new("A.md"), "original", "next").is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("A.md")).unwrap(),
            "original"
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
