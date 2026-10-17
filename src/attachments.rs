//! Attachment imports never overwrite existing files; Markdown links stay portable.
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

pub enum Input {
    File(PathBuf),
    Bytes { name: String, bytes: Vec<u8> },
}
pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "svg"
        )
    })
}
pub fn supported(path: &Path) -> bool {
    is_image(path)
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Resolve a local attachment URL relative to a note, rejecting paths outside the
/// vault and symlink components. Only supported documents are opened externally.
pub fn resolve(root: &Path, note: &Path, target: &str) -> io::Result<PathBuf> {
    let mut decoded = Vec::new();
    let mut bytes = target.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = bytes
                .next()
                .and_then(|b| (b as char).to_digit(16))
                .ok_or_else(|| invalid("Invalid attachment URL"))?;
            let lo = bytes
                .next()
                .and_then(|b| (b as char).to_digit(16))
                .ok_or_else(|| invalid("Invalid attachment URL"))?;
            decoded.push((hi * 16 + lo) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let target = String::from_utf8(decoded).map_err(|_| invalid("Attachment path is not UTF-8"))?;
    if target.contains(['\\', '\0', ':']) {
        return Err(invalid("Attachment path must stay inside the vault"));
    }
    let mut relative = if target.starts_with('/') {
        PathBuf::new()
    } else {
        note.parent().unwrap_or(Path::new("")).to_owned()
    };
    for component in Path::new(target.trim_start_matches('/')).components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => (),
            Component::ParentDir if relative.pop() => (),
            _ => return Err(invalid("Attachment path must stay inside the vault")),
        }
    }
    if !supported(&relative) {
        return Err(invalid("Unsupported attachment type"));
    }
    let mut full = root.to_owned();
    for component in relative.components() {
        full.push(component);
        if fs::symlink_metadata(&full)?.file_type().is_symlink() {
            return Err(invalid("Attachments cannot follow symlinks"));
        }
    }
    if !full.is_file() {
        return Err(invalid("Attachment is not a file"));
    }
    Ok(full)
}

pub fn import(root: &Path, input: Input) -> io::Result<PathBuf> {
    let name = match &input {
        Input::File(path) => path
            .file_name()
            .ok_or_else(|| invalid("Attachment has no filename"))?
            .to_string_lossy()
            .into_owned(),
        Input::Bytes { name, .. } => name.clone(),
    };
    if !supported(Path::new(&name)) {
        return Err(invalid("Choose an image or PDF attachment"));
    }
    let extension = Path::new(&name)
        .extension()
        .unwrap()
        .to_string_lossy()
        .to_ascii_lowercase();
    let stem: String = Path::new(&name)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .chars()
        .take(100)
        .map(|c| {
            if c.is_alphanumeric() || "-_. ".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches([' ', '.']);
    let stem = if stem.is_empty() { "Attachment" } else { stem };
    let stem = stem.to_owned();
    // Prefix names reserved by Windows, even when the import occurs on Linux.
    let stem = if matches!(
        stem.to_ascii_uppercase().split('.').next().unwrap(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        format!("Attachment-{stem}")
    } else {
        stem
    };
    let directory = root.join("Attachments");
    if let Ok(meta) = fs::symlink_metadata(&directory)
        && (meta.file_type().is_symlink() || !meta.is_dir())
    {
        return Err(invalid(
            "Attachments must be a real folder inside the vault",
        ));
    }
    fs::create_dir_all(&directory)?;
    let mut temp = tempfile::NamedTempFile::new_in(&directory)?;
    match input {
        Input::File(path) => {
            if !fs::symlink_metadata(&path)?.is_file() {
                return Err(invalid("Drop regular image or PDF files"));
            }
            io::copy(&mut fs::File::open(path)?, &mut temp)?;
        }
        Input::Bytes { bytes, .. } => temp.write_all(&bytes)?,
    }
    temp.as_file().sync_all()?;
    let names: std::collections::HashSet<_> = fs::read_dir(&directory)?
        // A partial inventory cannot establish portable, case-insensitive names.
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    for suffix in 0..10_000 {
        let name = if suffix == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{suffix}.{extension}")
        };
        if names.contains(&name.to_lowercase()) {
            continue;
        }
        match temp.persist_noclobber(directory.join(&name)) {
            Ok(_) => return Ok(PathBuf::from("Attachments").join(name)),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => temp = error.file,
            Err(error) => return Err(error.error),
        }
    }
    Err(invalid("Too many attachments with the same name"))
}

pub fn link(note: &Path, attachment: &Path) -> String {
    let mut path = "../".repeat(note.parent().map_or(0, |p| p.components().count()));
    path.push_str(&attachment.to_string_lossy().replace('\\', "/"));
    let mut url = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    let label = attachment
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .replace(['[', ']', '\\'], "-");
    format!(
        "{}[{label}]({url})",
        if is_image(attachment) { "!" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_is_non_destructive_and_links_resolve_from_nested_notes() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("Photo.PNG");
        fs::write(&original, b"image bytes").unwrap();
        let first = import(root.path(), Input::File(original.clone())).unwrap();
        let second = import(
            root.path(),
            Input::Bytes {
                name: "photo.png".into(),
                bytes: b"another image".to_vec(),
            },
        )
        .unwrap();
        assert_ne!(
            first.to_string_lossy().to_lowercase(),
            second.to_string_lossy().to_lowercase()
        );
        assert_eq!(fs::read(&original).unwrap(), b"image bytes");
        assert_eq!(fs::read(root.path().join(&first)).unwrap(), b"image bytes");
        let pdf = import(
            root.path(),
            Input::Bytes {
                name: "éclair (one).pdf".into(),
                bytes: b"%PDF-1.7".to_vec(),
            },
        )
        .unwrap();
        let markdown = link(Path::new("Folder/Nested/Note.md"), &pdf);
        let target = markdown.split_once("](").unwrap().1.trim_end_matches(')');
        assert_eq!(
            resolve(root.path(), Path::new("Folder/Nested/Note.md"), target).unwrap(),
            root.path().join(pdf)
        );
        assert!(!markdown.starts_with('!'));
        assert!(link(Path::new("Note.md"), &first).starts_with("!["));
        assert!(resolve(root.path(), Path::new("Note.md"), "../escape.png").is_err());
        assert!(
            import(
                root.path(),
                Input::Bytes {
                    name: "script.exe".into(),
                    bytes: vec![]
                }
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn imports_and_links_refuse_symlink_escapes() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("Attachments")).unwrap();
        assert!(
            import(
                root.path(),
                Input::Bytes {
                    name: "x.png".into(),
                    bytes: vec![1]
                }
            )
            .is_err()
        );
        fs::write(outside.path().join("x.png"), [1]).unwrap();
        assert!(resolve(root.path(), Path::new("Note.md"), "Attachments/x.png").is_err());
    }
}
