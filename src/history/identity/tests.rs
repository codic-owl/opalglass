use super::*;
use crate::{history, vault::Vault};

fn saved_note(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
    history::capture(root, Path::new(path), &format!("older {text}")).unwrap();
}

fn texts(root: &Path, path: &str) -> Vec<String> {
    history::versions(root, Path::new(path))
        .unwrap()
        .iter()
        .map(|v| history::read(root, Path::new(path), &v.id).unwrap())
        .collect()
}

#[test]
fn saved_versions_follow_repeated_note_and_folder_moves_without_mixing_reused_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "Folder/A.md", "A");
    saved_note(root, "Folder/B.md", "B");
    let mut vault = Vault::open(root).unwrap();
    vault
        .move_entry(Path::new("Folder/A.md"), Path::new("Folder/Renamed.md"))
        .unwrap();
    vault
        .move_entry(Path::new("Folder"), Path::new("Moved"))
        .unwrap();
    let mut vault = Vault::open(root).unwrap();
    assert_eq!(texts(root, "Moved/Renamed.md"), ["older A"]);
    assert_eq!(texts(root, "Moved/B.md"), ["older B"]);
    vault
        .save(Path::new("Moved/Renamed.md"), "A", "A edited")
        .unwrap();
    let versions = texts(root, "Moved/Renamed.md");
    assert!(versions.contains(&"A".into()) && versions.contains(&"older A".into()));
    vault.create("Folder/A", "unrelated").unwrap();
    vault
        .save(Path::new("Folder/A.md"), "unrelated", "new edits")
        .unwrap();
    assert_eq!(texts(root, "Folder/A.md"), ["unrelated"]);
    assert!(texts(root, "Moved/Renamed.md").contains(&"older A".into()));
    assert!(texts(root, "Folder/Renamed.md").is_empty());
}

#[test]
fn destination_history_is_not_merged_and_collisions_leave_source_history_intact() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "A.md", "A");
    saved_note(root, "B.md", "B");
    let mut vault = Vault::open(root).unwrap();
    assert!(
        vault
            .move_entry(Path::new("A.md"), Path::new("B.md"))
            .is_err()
    );
    assert_eq!(texts(root, "A.md"), ["older A"]);
    assert_eq!(texts(root, "B.md"), ["older B"]);
    fs::remove_file(root.join("B.md")).unwrap();
    vault
        .move_entry(Path::new("A.md"), Path::new("B.md"))
        .unwrap();
    assert_eq!(texts(root, "B.md"), ["older A"]);
    assert!(texts(root, "A.md").is_empty());
}

#[test]
fn capture_keeps_its_selected_identity_when_another_app_moves_and_reuses_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "A.md", "A");
    // Pause a capture after route selection, then interleave another instance's
    // rename and creation before the capture reads its existing snapshot.
    let selected = location(root, Path::new("A.md")).unwrap();
    let mut vault = Vault::open(root).unwrap();
    vault
        .move_entry(Path::new("A.md"), Path::new("B.md"))
        .unwrap();
    vault.create("A", "unrelated").unwrap();
    history::capture_at(root, &selected, "older A").unwrap();
    assert_eq!(texts(root, "B.md"), ["older A"]);
    assert!(texts(root, "A.md").is_empty());
}

#[test]
fn capture_prunes_only_its_selected_identity_after_a_move_and_old_name_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "A.md", "A");
    let selected = location(root, Path::new("A.md")).unwrap();
    let mut vault = Vault::open(root).unwrap();
    vault
        .move_entry(Path::new("A.md"), Path::new("B.md"))
        .unwrap();
    vault.create("A", "unrelated").unwrap();
    history::capture(root, Path::new("A.md"), "older A").unwrap();
    for index in 0..99 {
        history::capture(
            root,
            Path::new("A.md"),
            &format!("unrelated version {index}"),
        )
        .unwrap();
    }
    // Both identities contain this hash. Make it the new note's oldest entry so
    // enumerating the reused path would delete the moved note's only snapshot.
    let reused = location(root, Path::new("A.md")).unwrap();
    let folder = directory(root, &["notes", &reused.key], false).unwrap();
    fs::File::options()
        .write(true)
        .open(folder.join(format!("{}.json", hash("older A"))))
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
        .unwrap();
    history::capture_at(root, &selected, "in-flight baseline").unwrap();
    let moved = texts(root, "B.md");
    assert_eq!(moved.len(), 2);
    assert!(moved.contains(&"older A".into()));
    assert!(moved.contains(&"in-flight baseline".into()));
    assert_eq!(texts(root, "A.md").len(), 100);
}

#[test]
fn interrupted_moves_recover_on_either_side_of_the_filesystem_rename() {
    for committed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        saved_note(root, "A.md", "A");
        {
            let _lock = lock(root, true).unwrap();
            prepare(root, Path::new("A.md"), Path::new("B.md"), &["A.md".into()]).unwrap();
            if committed {
                crate::platform::rename_new(&root.join("A.md"), &root.join("B.md")).unwrap();
            }
            // Simulate exit before publishing routes/removing the journal.
        }
        let (live, retired) = if committed {
            ("B.md", "A.md")
        } else {
            ("A.md", "B.md")
        };
        assert_eq!(texts(root, live), ["older A"]);
        assert!(texts(root, retired).is_empty());
        assert!(!root.join(".opalglass-history").join(PENDING).exists());
        let mut vault = Vault::open(root).unwrap();
        vault.save(Path::new(live), "A", "edited").unwrap();
        assert!(texts(root, live).contains(&"older A".into()));
    }
}

#[test]
fn case_only_rename_and_recovery_use_actual_entry_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "Note.md", "A");
    {
        let _lock = lock(root, true).unwrap();
        prepare(
            root,
            Path::new("Note.md"),
            Path::new("note.md"),
            &["Note.md".into()],
        )
        .unwrap();
        // Use the OS operation here to exercise journal recovery independently
        // of platform-specific case-rename support in the move primitive.
        fs::rename(root.join("Note.md"), root.join("note.md")).unwrap();
    }
    assert_eq!(texts(root, "note.md"), ["older A"]);
    assert!(texts(root, "Note.md").is_empty());
}

#[test]
fn ambiguous_interrupted_move_preserves_the_journal_and_both_notes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "A.md", "A");
    {
        let _lock = lock(root, true).unwrap();
        prepare(root, Path::new("A.md"), Path::new("B.md"), &["A.md".into()]).unwrap();
        fs::write(root.join("B.md"), "external").unwrap();
    }
    let error = history::versions(root, Path::new("A.md")).unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
    assert!(root.join(".opalglass-history").join(PENDING).exists());
    assert_eq!(fs::read_to_string(root.join("A.md")).unwrap(), "A");
    assert_eq!(fs::read_to_string(root.join("B.md")).unwrap(), "external");
    fs::remove_file(root.join("B.md")).unwrap();
    assert_eq!(texts(root, "A.md"), ["older A"]);
}

#[test]
fn committed_history_remains_readable_when_route_publication_must_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "A.md", "A");
    {
        let _lock = lock(root, true).unwrap();
        prepare(root, Path::new("A.md"), Path::new("B.md"), &["A.md".into()]).unwrap();
        crate::platform::rename_new(&root.join("A.md"), &root.join("B.md")).unwrap();
        // Deterministic metadata-publication failure, including under root.
        fs::create_dir(root.join(".opalglass-history").join(ROUTES)).unwrap();
    }
    assert_eq!(texts(root, "B.md"), ["older A"]);
    assert!(root.join(".opalglass-history").join(PENDING).exists());
    fs::remove_dir(root.join(".opalglass-history").join(ROUTES)).unwrap();
    assert_eq!(texts(root, "B.md"), ["older A"]);
    assert!(!root.join(".opalglass-history").join(PENDING).exists());
}

#[test]
fn history_routes_are_portable_and_reject_storage_path_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    saved_note(root, "Folder/A.md", "A");
    let mut vault = Vault::open(root).unwrap();
    vault
        .move_entry(Path::new("Folder/A.md"), Path::new("Folder/B.md"))
        .unwrap();
    let file = root.join(".opalglass-history").join(ROUTES);
    let mut routes = read_json::<Routes>(&file).unwrap().unwrap();
    assert!(routes.contains_key("Folder/B.md"));
    routes.get_mut("Folder/B.md").unwrap().key = "../../escape".into();
    crate::state::write_json(&file, &routes).unwrap();
    assert!(history::versions(root, Path::new("Folder/B.md")).is_err());
}
