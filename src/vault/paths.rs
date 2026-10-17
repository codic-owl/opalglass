//! Portable user names and confined native filesystem paths.
use super::*;
use std::path::Component;

fn portable_name(path: &Path) -> io::Result<String> {
    path.components()
        .map(|part| match part {
            Component::Normal(name) => name
                .to_str()
                .ok_or_else(|| invalid("Vault paths must use valid Unicode")),
            _ => Err(invalid("Note paths must stay inside the vault")),
        })
        .collect::<io::Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

impl Vault {
    /// Paths must stay relative, visible and portable between Windows and Unix.
    pub fn note_path(name: &str) -> io::Result<PathBuf> {
        let name = name.trim();
        if name.is_empty()
            || name.contains(['\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
            || name.chars().any(char::is_control)
        {
            return Err(invalid(
                "Use a note name without reserved filename characters",
            ));
        }
        let mut path = PathBuf::from(name);
        for component in path.components() {
            let Component::Normal(part) = component else {
                return Err(invalid("Note paths must stay inside the vault"));
            };
            let part = part.to_string_lossy();
            let stem = part.split('.').next().unwrap_or("").to_uppercase();
            if part.starts_with('.')
                || part.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(invalid("That name is hidden or reserved on Windows"));
            }
        }
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            path.as_mut_os_string().push(".md");
        }
        Ok(path)
    }
    pub(super) fn checked(&self, relative: &Path) -> io::Result<PathBuf> {
        // Use path components, not the native separator: Windows paths contain
        // backslashes even though the user-facing note syntax uses forward slashes.
        if Self::note_path(&portable_name(relative)?)? != relative {
            return Err(invalid("Expected a Markdown note path"));
        }
        self.checked_entry(relative)
    }
    /// A visible vault-relative path, with the same confinement rules as notes.
    pub fn folder_path(name: &str) -> io::Result<PathBuf> {
        // Validate components without changing the final extension.
        Self::note_path(name)?;
        Ok(PathBuf::from(name.trim()))
    }
    /// Convert a validated native vault-relative path to portable user-name syntax.
    /// Literal backslashes in Unix filenames remain invalid, rather than becoming
    /// separators that redirect an action to another entry.
    pub fn path_name(path: &Path) -> io::Result<String> {
        let name = portable_name(path)?;
        if Self::folder_path(&name)? != path {
            return Err(invalid("Invalid vault path"));
        }
        Ok(name)
    }
    pub(crate) fn validate_entry_path(path: &Path) -> io::Result<()> {
        Self::path_name(path).map(|_| ())
    }
    pub(super) fn checked_entry(&self, path: &Path) -> io::Result<PathBuf> {
        Self::validate_entry_path(path)?;
        let mut full = self.root.clone();
        for component in path.components() {
            full.push(component);
            match fs::symlink_metadata(&full) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(invalid("Symlinks are not followed inside vaults"));
                }
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        Ok(full)
    }
    pub(super) fn check_destination(
        &self,
        path: &Path,
        excluding: Option<&Path>,
    ) -> io::Result<()> {
        let full = self.checked_entry(path)?;
        let parent = full
            .parent()
            .ok_or_else(|| invalid("Invalid destination"))?;
        let name = full.file_name().unwrap().to_string_lossy().to_lowercase();
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            if excluding.is_some_and(|p| self.root.join(p) == entry.path()) {
                continue;
            }
            if entry.file_name().to_string_lossy().to_lowercase() == name {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "The destination already exists",
                ));
            }
        }
        Ok(())
    }
}
