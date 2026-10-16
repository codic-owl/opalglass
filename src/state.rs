//! Durable application preferences and workspace state.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub font_size: f32,
    pub line_numbers: bool,
    pub default_live_preview: bool,
    pub autosave: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            font_size: 14.,
            line_numbers: true,
            default_live_preview: false,
            autosave: true,
        }
    }
}
impl Preferences {
    pub fn load(directory: &Path) -> Self {
        let mut prefs: Self = std::fs::read(directory.join("preferences.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        prefs.font_size = if prefs.font_size.is_finite() {
            prefs.font_size.clamp(10., 28.)
        } else {
            14.
        };
        prefs
    }
    pub fn save(&self, directory: &Path) -> io::Result<()> {
        write_json(&directory.join("preferences.json"), self)
    }
}
pub fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing state directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_retains_baseline_and_never_overwrites_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let path = Path::new("A.md");
        std::fs::write(dir.path().join(path), "original\r\n").unwrap();
        let mut vault = crate::vault::Vault::open(dir.path()).unwrap();
        let record = Recovery {
            root: vault.root.clone(),
            path: path.into(),
            baseline: "original\r\n".into(),
            text: "unsaved\r\n".into(),
        };
        record.save(state.path()).unwrap();
        std::fs::write(dir.path().join(path), "external\r\n").unwrap();
        let (baseline, buffer) = load_document(&vault, path, Some(state.path())).unwrap();
        assert_eq!(baseline, "original\r\n");
        assert_eq!(buffer.text(), "unsaved\n");
        assert!(vault.save(path, &baseline, &record.text).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(path)).unwrap(),
            "external\r\n"
        );
        // An externally removed note can still be recovered into an unsaved tab.
        std::fs::remove_file(dir.path().join(path)).unwrap();
        assert_eq!(
            load_document(&vault, path, Some(state.path()))
                .unwrap()
                .1
                .text(),
            "unsaved\n"
        );
        // A crash after the file write but before record cleanup isn't a conflict.
        std::fs::write(dir.path().join(path), &record.text).unwrap();
        let (baseline, _) = load_document(&vault, path, Some(state.path())).unwrap();
        assert_eq!(baseline, record.text);
        assert!(
            Recovery::load(state.path(), &vault.root, path)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn preferences_persist_and_validate() {
        let dir = tempfile::tempdir().unwrap();
        let p = Preferences {
            font_size: 18.,
            line_numbers: false,
            default_live_preview: true,
            autosave: false,
        };
        p.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), p);
        std::fs::write(dir.path().join("preferences.json"), "{\"font_size\":500}").unwrap();
        assert_eq!(Preferences::load(dir.path()).font_size, 28.);
    }
}

pub fn last_vault(directory: &Path) -> Option<std::path::PathBuf> {
    std::fs::read(directory.join("last-vault.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<std::path::PathBuf>(&bytes).ok())
        .filter(|path| path.is_dir())
}

/// A durable unsaved edit. Baseline is retained so recovery never silently
/// overwrites a file that changed while the app was closed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recovery {
    pub root: std::path::PathBuf,
    pub path: std::path::PathBuf,
    pub baseline: String,
    pub text: String,
}
fn path_key(path: &Path) -> String {
    format!(
        "{:016x}",
        path.as_os_str()
            .as_encoded_bytes()
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b))
                .wrapping_mul(0x100000001b3))
    )
}
fn recovery_file(directory: &Path, root: &Path, path: &Path) -> std::path::PathBuf {
    directory
        .join("recovery")
        .join(path_key(root))
        .join(format!("{}.json", path_key(path)))
}
impl Recovery {
    pub fn save(&self, directory: &Path) -> io::Result<()> {
        write_json(&recovery_file(directory, &self.root, &self.path), self)
    }
    pub fn load(directory: &Path, root: &Path, path: &Path) -> io::Result<Option<Self>> {
        let bytes = match std::fs::read(recovery_file(directory, root, path)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let saved: Self = serde_json::from_slice(&bytes)?;
        if saved.root != root || saved.path != path {
            return Err(io::Error::other(
                "Recovery record belongs to a different note",
            ));
        }
        Ok(Some(saved))
    }
    pub fn remove(directory: &Path, root: &Path, path: &Path) -> io::Result<()> {
        let file = recovery_file(directory, root, path);
        match std::fs::remove_file(&file) {
            Ok(()) => {
                #[cfg(unix)]
                std::fs::File::open(file.parent().unwrap())?.sync_all()?;
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

pub fn load_document(
    vault: &crate::vault::Vault,
    path: &Path,
    directory: Option<&Path>,
) -> io::Result<(String, crate::buffer::Buffer)> {
    let disk = vault.read(path);
    if let Some(directory) = directory
        && let Some(recovery) = Recovery::load(directory, &vault.root, path)?
    {
        // A crash may happen after saving the file but before removing recovery.
        if disk.as_ref().is_ok_and(|text| text == &recovery.text) {
            Recovery::remove(directory, &vault.root, path)?;
        } else {
            return Ok((
                recovery.baseline,
                crate::buffer::Buffer::new(crate::document::normalize(&recovery.text)),
            ));
        }
    }
    disk.map(|text| {
        let buffer = crate::buffer::Buffer::new(crate::document::normalize(&text));
        (text, buffer)
    })
}
