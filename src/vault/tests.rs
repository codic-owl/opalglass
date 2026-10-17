use super::*;

mod filesystem;

#[test]
fn moves_preserve_markdown_references_images_and_wiki_embeds_without_reformatting() {
    let dir = tempfile::tempdir().unwrap();
    for folder in ["Folder", "Nested", "Attachments"] {
        fs::create_dir(dir.path().join(folder)).unwrap();
    }
    let attachment = "Attachments/é image.png";
    fs::write(dir.path().join(attachment), b"original image bytes").unwrap();
    fs::write(dir.path().join("Folder/Ref.md"), "Reference\n").unwrap();
    let note = "---\nexample: '[keep](../Attachments/%C3%A9%20image.png)'\n---\n[local](Ref.md#Heading)\n![picture](../Attachments/%C3%A9%20image.png \"caption\")\n[download][asset]\n\n[asset]: <../Attachments/%C3%A9%20image.png> 'title'\n\n![[é image.png|120]]\n`![code](../Attachments/%C3%A9%20image.png)`\n%% [comment](../Attachments/%C3%A9%20image.png) %%\n";
    fs::write(dir.path().join("Folder/Note.md"), note).unwrap();
    fs::write(
        dir.path().join("Home.md"),
        "[note](Folder/Note.md#Heading) [[Folder/Note|alias]]\n",
    )
    .unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    vault
        .move_entry(Path::new("Folder"), Path::new("Nested/Renamed"))
        .unwrap();
    let moved = fs::read_to_string(dir.path().join("Nested/Renamed/Note.md")).unwrap();
    assert!(moved.contains("[local](Ref.md#Heading)"));
    assert!(moved.contains("![picture](../../Attachments/%C3%A9%20image.png \"caption\")"));
    assert!(moved.contains("[asset]: <../../Attachments/%C3%A9%20image.png> 'title'"));
    assert!(moved.contains("example: '[keep](../Attachments/%C3%A9%20image.png)'"));
    assert!(moved.contains("`![code](../Attachments/%C3%A9%20image.png)`"));
    assert!(moved.contains("%% [comment](../Attachments/%C3%A9%20image.png) %%"));
    assert_eq!(
        fs::read_to_string(dir.path().join("Home.md")).unwrap(),
        "[note](Nested/Renamed/Note.md#Heading) [[Nested/Renamed/Note|alias]]\n"
    );
    vault
        .move_entry(
            Path::new(attachment),
            Path::new("Attachments/Renamed image.png"),
        )
        .unwrap();
    let moved = fs::read_to_string(dir.path().join("Nested/Renamed/Note.md")).unwrap();
    assert!(moved.contains("![picture](../../Attachments/Renamed%20image.png \"caption\")"));
    assert!(moved.contains("![[Attachments/Renamed image.png|120]]"));
    assert_eq!(
        fs::read(dir.path().join("Attachments/Renamed image.png")).unwrap(),
        b"original image bytes"
    );
    assert!(
        vault
            .files
            .contains(&PathBuf::from("Attachments/Renamed image.png"))
    );
    assert_eq!(
        vault.resolve_url(
            Path::new("Nested/Renamed/Note.md"),
            "../../Attachments/Renamed%20image.png"
        ),
        Some("Attachments/Renamed image.png".into())
    );
}
#[test]
fn trash_preserves_folders_attachments_links_and_restores_without_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Folder")).unwrap();
    fs::write(dir.path().join("Folder/A.md"), "saved edits").unwrap();
    fs::write(dir.path().join("Folder/photo.bin"), [0, 1, 255]).unwrap();
    fs::write(dir.path().join("B.md"), "[[Folder/A]]").unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    vault.trash_entry(Path::new("Folder")).unwrap();
    assert!(!dir.path().join("Folder").exists());
    assert_eq!(vault.read(Path::new("B.md")).unwrap(), "[[Folder/A]]");
    assert!(vault.resolve(Path::new("B.md"), "Folder/A").is_none());
    assert!(vault.notes.iter().all(|n| !n.path.starts_with(".trash")));
    // Trash survives a new Vault instance and refuses an occupied target.
    let mut vault = Vault::open(dir.path()).unwrap();
    fs::create_dir(dir.path().join("Folder")).unwrap();
    assert!(vault.restore_last_trashed().is_err());
    fs::remove_dir(dir.path().join("Folder")).unwrap();
    assert_eq!(
        vault.restore_last_trashed().unwrap(),
        PathBuf::from("Folder")
    );
    assert_eq!(
        fs::read(dir.path().join("Folder/photo.bin")).unwrap(),
        vec![0, 1, 255]
    );
    assert_eq!(vault.read(Path::new("Folder/A.md")).unwrap(), "saved edits");
    assert!(vault.resolve(Path::new("B.md"), "Folder/A").is_some());
    assert!(vault.restore_last_trashed().is_err());
    for invalid in ["", "..", "../outside", ".trash", "/tmp"] {
        assert!(vault.trash_entry(Path::new(invalid)).is_err());
    }
}
#[test]
fn updating_existing_note_changes_only_its_backlinks_and_preserves_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in [
        ("A", "[[Target]]"),
        ("B", "[[Other]]"),
        ("C", "[[Target]]"),
        ("Other", ""),
        ("Target", ""),
    ] {
        fs::write(dir.path().join(format!("{name}.md")), text).unwrap();
    }
    let mut vault = Vault::open(dir.path()).unwrap();
    let snapshot = vault.clone();
    vault.update_note(
        Path::new("B.md"),
        "[[Target]] [[Target|alias]] [[B]] [[missing]]",
    );
    let sources = |vault: &Vault, target| {
        vault
            .backlinks(Path::new(target))
            .iter()
            .map(|n| n.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        sources(&vault, "Target.md"),
        ["A.md", "B.md", "C.md"].map(PathBuf::from)
    );
    assert!(sources(&vault, "Other.md").is_empty());
    assert!(sources(&vault, "B.md").is_empty());
    assert_eq!(
        sources(&snapshot, "Target.md"),
        ["A.md", "C.md"].map(PathBuf::from)
    );
    assert_eq!(sources(&snapshot, "Other.md"), [PathBuf::from("B.md")]);
    let indexes = vault.indexes.clone();
    vault.update_note(Path::new("B.md"), "Edited [[Target#heading]]");
    assert!(Arc::ptr_eq(&indexes, &vault.indexes));
    vault.update_note(Path::new("B.md"), "removed links");
    assert_eq!(
        sources(&vault, "Target.md"),
        ["A.md", "C.md"].map(PathBuf::from)
    );
}
#[test]
fn discovery_resolves_paths_without_reading_contents_then_indexes_search_and_backlinks() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Folder/Empty")).unwrap();
    fs::write(
        dir.path().join("Folder/Source.md"),
        "[[Target]] [[Target]] needle in body",
    )
    .unwrap();
    fs::write(dir.path().join("Folder/Target.md"), "# Target").unwrap();
    let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
    assert!(!vault.content_indexed());
    assert!(
        vault
            .notes
            .iter()
            .all(|n| n.matches_text("") && n.links.is_empty())
    );
    assert!(vault.folders.contains(&PathBuf::from("Folder/Empty")));
    assert_eq!(
        vault.resolve(Path::new("Folder/Source.md"), "Target"),
        Some("Folder/Target.md".into())
    );
    assert_eq!(vault.search("Source").len(), 1);
    assert!(vault.search("needle in body").is_empty());
    assert_eq!(
        vault.read(Path::new("Folder/Target.md")).unwrap(),
        "# Target"
    );
    let mut progress = Vec::new();
    assert!(
        vault
            .refresh_with_progress(&AtomicBool::new(false), |count| progress.push(count))
            .unwrap()
    );
    assert_eq!(progress, [1, 2]);
    assert!(vault.content_indexed());
    assert!(vault.search("needle in body").is_empty());
    assert!(
        vault
            .notes
            .iter()
            .any(|n| n.matches_text("[[Target]] [[Target]] needle in body"))
    );
    assert_eq!(vault.backlinks(Path::new("Folder/Target.md")).len(), 1);
    assert!(!vault.refresh().unwrap());
}
#[test]
fn discovery_survives_invalid_note_text_and_failed_indexing_preserves_inventory() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Broken.md"), [0xff, 0xfe]).unwrap();
    let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
    let before = vault.clone();
    assert_eq!(vault.notes.len(), 1);
    assert!(vault.refresh().is_err());
    assert!(!vault.content_indexed());
    assert!(Arc::ptr_eq(&vault.notes, &before.notes));
    assert!(Arc::ptr_eq(&vault.indexes, &before.indexes));
    fs::write(dir.path().join("Broken.md"), "fixed").unwrap();
    assert!(vault.refresh().unwrap());
    assert!(vault.content_indexed());
}
#[test]
fn cancelled_discovery_and_indexing_do_not_install_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("A.md"), "body").unwrap();
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        Vault::discover(dir.path(), &cancelled).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
    let before = vault.clone();
    assert_eq!(
        vault.refresh_cancellable(&cancelled).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert!(Arc::ptr_eq(&vault.notes, &before.notes));
    assert!(Arc::ptr_eq(&vault.indexes, &before.indexes));
    assert!(!vault.content_indexed());
}
#[test]
fn saves_and_moves_from_discovered_vault_preserve_unindexed_links() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("A.md"), "[[B]]\r\n").unwrap();
    fs::write(dir.path().join("B.md"), "target").unwrap();
    fs::write(dir.path().join("C.md"), "[[B|Alias]]").unwrap();
    let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
    let baseline = vault.read(Path::new("A.md")).unwrap();
    vault
        .save(Path::new("A.md"), &baseline, "edited [[B]]\r\n")
        .unwrap();
    assert!(!vault.content_indexed());
    vault
        .move_entry(Path::new("B.md"), Path::new("Renamed.md"))
        .unwrap();
    assert!(vault.content_indexed());
    assert_eq!(
        fs::read_to_string(dir.path().join("A.md")).unwrap(),
        "edited [[Renamed]]\r\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("C.md")).unwrap(),
        "[[Renamed|Alias]]"
    );
}
#[test]
fn rewrite_refuses_unaddressable_destination_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Note.md"), "").unwrap();
    fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    for name in ["New#Heading.md", "New]]Name.md"] {
        assert!(
            vault
                .move_entry(Path::new("Note.md"), Path::new(name))
                .is_err()
        );
        assert!(dir.path().join("Note.md").exists());
        assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[Note]]");
    }
}
#[test]
fn failed_link_write_rolls_back_previous_edits_and_preserves_external_change() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Note.md"), "source").unwrap();
    fs::write(dir.path().join("A.md"), "[[Note]]").unwrap();
    fs::write(dir.path().join("B.md"), "external").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    assert!(
        vault
            .commit_move(
                &[
                    ("A.md".into(), "[[Note]]".into(), "[[Moved]]".into()),
                    ("B.md".into(), "[[Note]]".into(), "[[Moved]]".into())
                ],
                &dir.path().join("Note.md"),
                &dir.path().join("Moved.md")
            )
            .is_err()
    );
    assert_eq!(vault.read(Path::new("A.md")).unwrap(), "[[Note]]");
    assert_eq!(vault.read(Path::new("B.md")).unwrap(), "external");
    assert!(dir.path().join("Note.md").exists());
}
#[test]
fn failed_rename_rolls_back_link_writes_and_never_clobbers_destination() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Note.md"), "source").unwrap();
    fs::write(dir.path().join("Existing.md"), "destination").unwrap();
    fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    // Simulate a destination appearing after preflight: link writes have
    // already happened when the atomic NOREPLACE rename rejects it.
    assert!(
        vault
            .commit_move(
                &[("Index.md".into(), "[[Note]]".into(), "[[Existing]]".into())],
                &dir.path().join("Note.md"),
                &dir.path().join("Existing.md")
            )
            .is_err()
    );
    assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[Note]]");
    assert_eq!(vault.read(Path::new("Note.md")).unwrap(), "source");
    assert_eq!(vault.read(Path::new("Existing.md")).unwrap(), "destination");
}
#[test]
fn move_folder_preserves_incoming_outgoing_aliases_headings_and_line_endings() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Old/Empty")).unwrap();
    fs::create_dir(dir.path().join("New")).unwrap();
    fs::write(
        dir.path().join("Old/Note.md"),
        "[[Sibling#Heading|local]]\r\n[[#Self]]\r\n[[Missing]]",
    )
    .unwrap();
    fs::write(dir.path().join("Old/Sibling.md"), "# Heading").unwrap();
    fs::write(
        dir.path().join("Index.md"),
        "![[Old/Note#Self|Alias 😀]]\r\n`[[Old/Note]]`\r\n```\r\n[[Old/Note]]\r\n```\r\n",
    )
    .unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    vault
        .move_entry(Path::new("Old"), Path::new("New/Renamed"))
        .unwrap();
    assert!(!dir.path().join("Old").exists());
    assert!(dir.path().join("New/Renamed/Empty").is_dir());
    assert_eq!(
        vault.read(Path::new("Index.md")).unwrap(),
        "![[New/Renamed/Note#Self|Alias 😀]]\r\n`[[Old/Note]]`\r\n```\r\n[[Old/Note]]\r\n```\r\n"
    );
    assert_eq!(
        vault.read(Path::new("New/Renamed/Note.md")).unwrap(),
        "[[Sibling#Heading|local]]\r\n[[#Self]]\r\n[[Missing]]"
    );
    assert_eq!(
        vault.resolve(Path::new("New/Renamed/Note.md"), "Sibling#Heading"),
        Some("New/Renamed/Sibling.md".into())
    );
    assert_eq!(vault.backlinks(Path::new("New/Renamed/Note.md")).len(), 1);
    assert!(vault.folders.contains(&PathBuf::from("New/Renamed/Empty")));
    assert_eq!(vault.search("Renamed").len(), 2);
}
#[test]
fn move_note_preserves_local_targets_despite_destination_shadowing() {
    let dir = tempfile::tempdir().unwrap();
    for path in ["A", "B/A"] {
        fs::create_dir_all(dir.path().join(path)).unwrap();
    }
    for (path, text) in [
        ("A/Note.md", "[[Sibling]] [[Other]]"),
        ("A/Sibling.md", ""),
        ("B/Sibling.md", ""),
        ("B/A/Sibling.md", ""),
        ("Other.md", ""),
        ("Index.md", "[[Note|Keep label]]"),
    ] {
        fs::write(dir.path().join(path), text).unwrap();
    }
    let mut vault = Vault::open(dir.path()).unwrap();
    vault
        .move_entry(Path::new("A/Note.md"), Path::new("B/Renamed.md"))
        .unwrap();
    assert_eq!(
        vault.read(Path::new("B/Renamed.md")).unwrap(),
        "[[/A/Sibling]] [[Other]]"
    );
    assert_eq!(
        vault.read(Path::new("Index.md")).unwrap(),
        "[[B/Renamed|Keep label]]"
    );
    assert_eq!(
        vault.resolve(Path::new("B/Renamed.md"), "/A/Sibling"),
        Some("A/Sibling.md".into())
    );
}
#[test]
fn folder_actions_reject_collisions_escapes_and_recursive_moves_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Note.md"), "note").unwrap();
    fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    vault.create_folder("Empty").unwrap();
    assert!(vault.folders.contains(&PathBuf::from("Empty")));
    assert!(vault.create_folder("empty").is_err());
    for to in [
        "Index.md",
        "index.md",
        "../Outside.md",
        "/Outside.md",
        "Missing/Note.md",
    ] {
        assert!(
            vault
                .move_entry(Path::new("Note.md"), Path::new(to))
                .is_err(),
            "{to}"
        );
    }
    assert!(
        vault
            .move_entry(Path::new("Empty"), Path::new("Empty/Nested"))
            .is_err()
    );
    assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[Note]]");
    assert_eq!(vault.read(Path::new("Note.md")).unwrap(), "note");
    fs::create_dir(dir.path().join("External")).unwrap();
    assert!(vault.refresh().unwrap());
    assert!(vault.folders.contains(&PathBuf::from("External")));
    fs::remove_dir(dir.path().join("External")).unwrap();
    assert!(vault.refresh().unwrap());
    assert!(!vault.folders.contains(&PathBuf::from("External")));
}
#[test]
fn title_index_tracks_ambiguity_and_local_saves_update_backlinks() {
    let temp = tempfile::tempdir().unwrap();
    let mut vault = Vault::open(temp.path()).unwrap();
    let source = vault.create("Source", "[[Target]]").unwrap();
    let target = vault.create("a/Target", "").unwrap();
    assert_eq!(vault.resolve(&source, "Target"), Some(target.clone()));
    assert_eq!(vault.backlinks(&target).len(), 1);
    let other = vault.create("b/Target", "").unwrap();
    assert_eq!(vault.resolve(&source, "Target"), None);
    assert!(vault.backlinks(&target).is_empty());
    assert_eq!(vault.resolve(&source, "missing/Target"), None);
    vault
        .save(&source, "[[Target]]", "[[a/Target]] [[a/Target]]")
        .unwrap();
    assert_eq!(vault.backlinks(&target).len(), 1);
    fs::remove_file(temp.path().join(other)).unwrap();
    assert!(vault.refresh().unwrap());
    assert_eq!(vault.resolve(&source, "Target"), Some(target));
}

#[test]
fn path_filter_does_not_search_bodies_and_observes_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    let mut vault = Vault::open(temp.path()).unwrap();
    vault.create("Note", "searchable").unwrap();
    assert!(vault.search_indices("SEARCHABLE").is_empty());
    assert_eq!(vault.search_indices("NOTE"), vec![0]);
    assert!(
        vault
            .search_indices_cancellable("NOTE", &AtomicBool::new(true))
            .is_empty()
    );
}

#[test]
fn create_save_reopen_and_conflict_preserves_disk() {
    let temp = tempfile::tempdir().unwrap();
    let mut v = Vault::open(temp.path()).unwrap();
    let path = v.create("Ideas/Hello", "first").unwrap();
    v.save(&path, "first", "second").unwrap();
    assert_eq!(
        Vault::open(temp.path()).unwrap().read(&path).unwrap(),
        "second"
    );
    fs::write(temp.path().join(&path), "external").unwrap();
    assert!(matches!(
        v.save(&path, "second", "mine"),
        Err(SaveError::Conflict)
    ));
    assert_eq!(v.read(&path).unwrap(), "external");
    assert!(v.create("Ideas/Hello", "overwrite").is_err());
    assert!(v.create("ideas/hello", "case overwrite").is_err());
}
#[test]
fn links_resolve_relative_and_ambiguous_titles_stay_unresolved() {
    let temp = tempfile::tempdir().unwrap();
    let mut v = Vault::open(temp.path()).unwrap();
    let a = v.create("a/Note", "[[Other#Heading|Read]]").unwrap();
    let b = v.create("a/Other", "").unwrap();
    v.create("b/Other", "").unwrap();
    assert_eq!(v.resolve(&a, "Other#Heading"), Some(b.clone()));
    assert_eq!(v.resolve(Path::new("Root.md"), "Other"), None);
    assert_eq!(v.backlinks(&b)[0].path, a);
    assert!(v.search("heading").is_empty());
    assert_eq!(v.backlinks(&b)[0].links[0].target, "Other#Heading");
}
