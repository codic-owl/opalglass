//! Stable snapshot locations with a recoverable rename journal.
use super::{directory, hash, invalid, note_key, publish, regular};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

type Routes = BTreeMap<String, Location>;
const ROUTES: &str = "identities.json";
const PENDING: &str = "pending-move.json";

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Location {
    pub key: String,
    pub origin: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    from: String,
    to: String,
    before: Routes,
    after: Routes,
}

fn portable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn legacy(path: &Path) -> Location {
    Location {
        key: note_key(path),
        origin: PathBuf::from(portable(path)),
    }
}

fn validate_path(path: &Path) -> io::Result<()> {
    crate::vault::Vault::validate_entry_path(path)
}

fn validate(routes: &Routes) -> io::Result<()> {
    for (path, location) in routes {
        validate_path(Path::new(path))?;
        validate_path(&location.origin)?;
        if !super::valid_id(&location.key) {
            return Err(invalid("Invalid history storage key"));
        }
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match regular(path) {
        Ok(()) => Ok(Some(serde_json::from_slice(&fs::read(path)?)?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

// Serialize history-route transactions across app instances. Existing histories
// without identity metadata remain readable without creating any files.
fn lock(root: &Path, create: bool) -> io::Result<Option<fs::File>> {
    let dir = match directory(root, &[], create) {
        Ok(dir) => dir,
        Err(e) if !create && e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let path = dir.join("identity.lock");
    match regular(&path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
        }
        Err(e) => return Err(e),
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(Some(file))
}

// Check directory-entry spelling as well as existence so case-only renames
// recover correctly on case-insensitive Windows and macOS volumes.
fn exists_exact(root: &Path, relative: &str) -> io::Result<bool> {
    let mut directory = root.to_owned();
    for component in Path::new(relative).components() {
        let name = component.as_os_str();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let mut found = false;
        for entry in entries {
            let entry = entry?;
            if entry.file_name() == name {
                if entry.file_type()?.is_symlink() {
                    return Err(invalid("History move paths cannot contain symlinks"));
                }
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        directory.push(name);
    }
    Ok(true)
}

fn effective(root: &Path, pending: &Pending) -> io::Result<Routes> {
    validate_path(Path::new(&pending.from))?;
    validate_path(Path::new(&pending.to))?;
    validate(&pending.before)?;
    validate(&pending.after)?;
    match (
        exists_exact(root, &pending.from)?,
        exists_exact(root, &pending.to)?,
    ) {
        (true, false) => Ok(pending.before.clone()),
        (false, true) => Ok(pending.after.clone()),
        _ => Err(invalid(
            "An interrupted move has ambiguous source/destination paths. Restore the expected location before continuing; saved histories have been preserved.",
        )),
    }
}

fn settle(root: &Path, pending: &Pending, routes: &Routes) -> io::Result<()> {
    let dir = directory(root, &[], false)?;
    // Persist the filesystem rename before retiring its recovery record.
    #[cfg(unix)]
    for path in [&pending.from, &pending.to] {
        fs::File::open(
            root.join(path)
                .parent()
                .ok_or_else(|| invalid("Missing move parent"))?,
        )?
        .sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = pending;
    crate::state::write_json(&dir.join(ROUTES), routes)?;
    fs::remove_file(dir.join(PENDING))?;
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

fn load(root: &Path, require_settled: bool) -> io::Result<Routes> {
    let dir = match directory(root, &[], false) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Routes::new()),
        Err(e) => return Err(e),
    };
    if let Some(pending) = read_json::<Pending>(&dir.join(PENDING))? {
        let routes = effective(root, &pending)?;
        let result = settle(root, &pending, &routes);
        if require_settled {
            result?;
        }
        // The durable journal remains authoritative if metadata publication
        // fails. Reading saved versions does not require another successful write.
        return Ok(routes);
    }
    let routes = read_json::<Routes>(&dir.join(ROUTES))?.unwrap_or_default();
    validate(&routes)?;
    Ok(routes)
}

pub(super) fn location(root: &Path, path: &Path) -> io::Result<Location> {
    let _lock = lock(root, false)?;
    Ok(load(root, false)?
        .remove(&portable(path))
        .unwrap_or_else(|| legacy(path)))
}

fn fresh(root: &Path, path: &Path) -> io::Result<Location> {
    let notes = directory(root, &["notes"], true)?;
    loop {
        let nonce = tempfile::Builder::new()
            .rand_bytes(16)
            .tempfile_in(&notes)?;
        let key = hash(&nonce.path().to_string_lossy());
        match fs::create_dir(notes.join(&key)) {
            Ok(()) => {
                return Ok(Location {
                    key,
                    origin: PathBuf::from(portable(path)),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
    }
}

fn prepare(root: &Path, from: &Path, to: &Path, notes: &[PathBuf]) -> io::Result<Pending> {
    let before = load(root, true)?;
    let mut after = before.clone();
    for path in notes {
        let location = before
            .get(&portable(path))
            .cloned()
            .unwrap_or_else(|| legacy(path));
        let destination = crate::vault::moved_path(path, from, to);
        after.insert(portable(&destination), location);
        // A subsequently created note at the old path starts with empty history.
        after.insert(portable(path), fresh(root, path)?);
    }
    let pending = Pending {
        from: portable(from),
        to: portable(to),
        before,
        after,
    };
    publish(&directory(root, &[], true)?.join(PENDING), &pending)?;
    Ok(pending)
}

pub(crate) fn move_entry(root: &Path, from: &Path, to: &Path, notes: &[PathBuf]) -> io::Result<()> {
    if notes.is_empty() {
        return crate::platform::rename_new(&root.join(from), &root.join(to));
    }
    let _lock = lock(root, true)?;
    let pending = prepare(root, from, to, notes)?;
    if let Err(error) = crate::platform::rename_new(&root.join(from), &root.join(to)) {
        // The rename failed, so retain the original routes. A leftover journal
        // is harmless unless a concurrent external writer makes recovery ambiguous.
        let _ = settle(root, &pending, &pending.before);
        return Err(error);
    }
    // After rename succeeds, callers must not roll back edits under old paths.
    // Failure to publish routes leaves the durable intent for the next lookup.
    let _ = settle(root, &pending, &pending.after);
    Ok(())
}

#[cfg(test)]
mod tests;
