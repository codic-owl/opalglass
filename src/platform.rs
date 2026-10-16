use std::{io, path::Path};

/// Rename without replacing a destination created by another process.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)
}
#[cfg(windows)]
pub fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path contains a null character",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let source = wide(source)?;
    let destination = wide(destination)?;
    // MoveFileW requires a nonexistent destination. Both buffers are valid,
    // null-terminated UTF-16 and live for the duration of this call.
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileW(source.as_ptr(), destination.as_ptr())
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn rename_new(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Atomic no-replace moves are not available on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_and_directory_moves_never_replace_existing_destinations() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        assert!(rename_new(&root.path().join("a"), &root.path().join("b")).is_err());
        assert_eq!(std::fs::read_to_string(root.path().join("b")).unwrap(), "b");
        rename_new(&root.path().join("a"), &root.path().join("c")).unwrap();
        for name in ["d", "e"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        assert!(rename_new(&root.path().join("d"), &root.path().join("e")).is_err());
        rename_new(&root.path().join("d"), &root.path().join("f")).unwrap();
    }
}
