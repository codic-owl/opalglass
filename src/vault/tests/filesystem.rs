use super::*;

#[cfg(unix)]
#[test]
fn trash_rejects_symlinks_and_malicious_restore_records() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("A.md"), "note").unwrap();
    symlink(outside.path(), dir.path().join(".trash")).unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    assert!(vault.trash_entry(Path::new("A.md")).is_err());
    assert!(dir.path().join("A.md").exists());
    fs::remove_file(dir.path().join(".trash")).unwrap();
    vault.trash_entry(Path::new("A.md")).unwrap();
    let entry = fs::read_dir(dir.path().join(".trash"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(entry.join("original.json"), "\"../outside.md\"").unwrap();
    assert!(vault.restore_last_trashed().is_err());
    assert!(entry.join("content").exists());
}

#[test]
fn rename_supports_case_changes_and_updates_links() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Note.md"), "").unwrap();
    fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    vault
        .move_entry(Path::new("Note.md"), Path::new("note.md"))
        .unwrap();
    assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[note]]");
    // On case-insensitive filesystems both spellings still resolve. Inspect
    // stored directory-entry names to verify the rename's actual casing.
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(names.iter().any(|name| name == "note.md"));
    assert!(!names.iter().any(|name| name == "Note.md"));
    assert!(vault.note(Path::new("note.md")).is_some());
    assert!(vault.note(Path::new("Note.md")).is_none());
}

#[cfg(unix)]
#[test]
fn moves_and_new_folders_reject_symlink_ancestors() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Note.md"), "note").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("Link")).unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    assert!(vault.create_folder("Link/Folder").is_err());
    assert!(
        vault
            .move_entry(Path::new("Note.md"), Path::new("Link/Note.md"))
            .is_err()
    );
    assert!(
        vault
            .move_entry(Path::new("Link"), Path::new("Moved"))
            .is_err()
    );
    assert!(dir.path().join("Note.md").is_file());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn refresh_detects_same_length_rewrites_with_restored_mtime() {
    use std::{os::unix::fs::MetadataExt, time::Instant};

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Note.md");
    fs::write(&path, "before").unwrap();
    let original = fs::metadata(&path).unwrap();
    let modified = original.modified().unwrap();
    let changed = |metadata: &fs::Metadata| (metadata.ctime(), metadata.ctime_nsec());
    let mut vault = Vault::open(temp.path()).unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    loop {
        fs::write(&path, "after!").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let rewritten = fs::metadata(&path).unwrap();
        assert_eq!(rewritten.len(), original.len());
        assert_eq!(rewritten.modified().unwrap(), modified);
        if changed(&rewritten) != changed(&original) {
            break;
        }
        // Multiple writes can share one ctime tick on a fast CI runner. Wait
        // for that precondition instead of assuming nanosecond clock updates.
        assert!(
            Instant::now() < deadline,
            "filesystem ctime did not advance"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(vault.refresh().unwrap());
    assert!(vault.notes[0].matches_text("after!"));
    assert!(!vault.notes[0].matches_text("before"));
}

#[test]
fn filenames_are_portable_and_confined() {
    for name in [
        "",
        "../escape",
        "/absolute",
        "a/../../b",
        "a\\b",
        "CON.md",
        "NUL",
        ".hidden",
        "a:",
        "foo.",
    ] {
        assert!(Vault::note_path(name).is_err(), "{name}");
    }
    assert_eq!(
        Vault::note_path("Ideas/İstanbul").unwrap(),
        PathBuf::from("Ideas/İstanbul.md")
    );
}

#[test]
fn native_nested_paths_preserve_history_across_moves_and_trash_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    let source = Path::new("Projects").join("Source.md");
    let destination = Path::new("Archive").join("Moved.md");
    assert_eq!(vault.create("Projects/Source", "original").unwrap(), source);
    vault.save(&source, "original", "edited").unwrap();
    vault.create_folder("Archive").unwrap();
    vault.move_entry(&source, &destination).unwrap();

    // Stored origins are deserialized with native separators on Windows. Reading
    // routes after reopening exercises that validation as well as the move itself.
    let mut reopened = Vault::open(dir.path()).unwrap();
    assert_eq!(reopened.read(&destination).unwrap(), "edited");
    let versions = crate::history::versions(dir.path(), &destination).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        crate::history::read(dir.path(), &destination, &versions[0].id).unwrap(),
        "original"
    );
    reopened.trash_entry(&destination).unwrap();
    assert!(!dir.path().join(&destination).exists());
    assert_eq!(reopened.restore_last_trashed().unwrap(), destination);
    assert_eq!(reopened.read(&destination).unwrap(), "edited");
}

#[test]
fn native_path_validation_preserves_confinement_and_portable_names() {
    let nested = Path::new("Projects").join("İstanbul.md");
    Vault::validate_entry_path(&nested).unwrap();
    assert_eq!(Vault::path_name(&nested).unwrap(), "Projects/İstanbul.md");
    #[cfg(unix)]
    assert!(Vault::path_name(Path::new(r"Projects\İstanbul.md")).is_err());
    for invalid in [
        PathBuf::new(),
        PathBuf::from("/absolute"),
        Path::new("Projects").join("..").join("Outside.md"),
        Path::new("Projects").join("CON.md"),
        Path::new("Projects").join("trailing."),
        Path::new("Projects").join(".hidden"),
        PathBuf::from(r"C:\Outside.md"),
        PathBuf::from(r"\\server\share\Outside.md"),
        PathBuf::from(r"\\?\C:\Outside.md"),
    ] {
        assert!(Vault::path_name(&invalid).is_err(), "{}", invalid.display());
    }
    // User-entered names retain forward-slash syntax on every platform.
    assert!(Vault::folder_path(r"Projects\New folder").is_err());
    assert!(Vault::note_path(r"Projects\New note").is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_vault() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
    let mut v = Vault::open(temp.path()).unwrap();
    assert!(v.create("link/escape", "no").is_err());
    assert!(!outside.path().join("escape.md").exists());
}

#[test]
fn refresh_reuses_unchanged_notes_and_updates_search_and_links() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Source.md"), "[[Target]] [[Target]]").unwrap();
    fs::write(temp.path().join("Target.md"), "old body").unwrap();
    let mut vault = Vault::open(temp.path()).unwrap();
    let snapshot = vault.clone();
    assert!(!vault.refresh().unwrap());
    assert!(Arc::ptr_eq(&vault.notes, &snapshot.notes));
    assert_eq!(vault.backlinks(Path::new("Target.md")).len(), 1);

    let target = temp.path().join("Target.md");
    fs::write(&target, "new body with more text").unwrap();
    assert!(vault.refresh().unwrap());
    assert!(Arc::ptr_eq(&vault.notes[0], &snapshot.notes[0]));
    assert!(!Arc::ptr_eq(&vault.notes[1], &snapshot.notes[1]));
    assert!(vault.notes[1].matches_text("new body with more text"));
    assert!(snapshot.notes[1].matches_text("old body"));
    assert!(!snapshot.notes[1].matches_text("new body with more text"));
    assert!(!vault.refresh().unwrap());

    fs::remove_file(temp.path().join("Source.md")).unwrap();
    assert!(vault.refresh().unwrap());
    assert!(vault.backlinks(Path::new("Target.md")).is_empty());
    assert!(vault.search("Source").is_empty());
}
