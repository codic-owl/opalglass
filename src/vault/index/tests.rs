use super::*;

fn incoming(vault: &Vault, path: &str) -> Vec<PathBuf> {
    vault
        .backlinks(Path::new(path))
        .iter()
        .map(|n| n.path.clone())
        .collect()
}

#[test]
fn backlinks_include_used_markdown_references_and_deduplicate_mixed_links() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Folder")).unwrap();
    for note in ["Target note.md", "Unused.md", "Comment.md", "Code.md"] {
        fs::write(dir.path().join(note), "body").unwrap();
    }
    fs::write(dir.path().join("Folder/Source.md"), "---\nexample: '[yaml](../Unused.md)'\n---\n[[Target note]]\n[inline](../Target%20note.md#Heading)\n![embed](../Target%20note.md)\n[reference][dest]\n[shortcut]\n\n[dest]: ../Target%20note.md\n[shortcut]: ../Target%20note.md\n[unused]: ../Unused.md\n\n%% [hidden](../Comment.md) %%\n`[code](../Code.md)`\n[external](https://example.com/Unused.md)\n[self](Source.md)\n").unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    assert_eq!(
        incoming(&vault, "Target note.md"),
        [PathBuf::from("Folder/Source.md")]
    );
    for path in ["Unused.md", "Comment.md", "Code.md", "Folder/Source.md"] {
        assert!(
            incoming(&vault, path).is_empty(),
            "Unexpected backlink to {path}"
        );
    }
}

#[test]
fn markdown_backlinks_follow_saves_external_changes_moves_and_missing_targets() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Target.md"), "target").unwrap();
    fs::write(dir.path().join("Source.md"), "[target](Target.md)").unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    assert_eq!(incoming(&vault, "Target.md"), [PathBuf::from("Source.md")]);
    let old = vault.clone();
    vault
        .save(
            Path::new("Source.md"),
            "[target](Target.md)",
            "[later](Missing.md)",
        )
        .unwrap();
    assert!(incoming(&vault, "Target.md").is_empty());
    assert_eq!(incoming(&old, "Target.md"), [PathBuf::from("Source.md")]);
    vault.create("Missing", "new").unwrap();
    assert_eq!(incoming(&vault, "Missing.md"), [PathBuf::from("Source.md")]);
    vault
        .move_entry(Path::new("Missing.md"), Path::new("Renamed.md"))
        .unwrap();
    assert!(incoming(&vault, "Missing.md").is_empty());
    assert_eq!(incoming(&vault, "Renamed.md"), [PathBuf::from("Source.md")]);
    fs::write(dir.path().join("Source.md"), "[external edit](Target.md)").unwrap();
    vault.refresh().unwrap();
    assert!(incoming(&vault, "Renamed.md").is_empty());
    assert_eq!(incoming(&vault, "Target.md"), [PathBuf::from("Source.md")]);
    fs::remove_file(dir.path().join("Source.md")).unwrap();
    vault.refresh().unwrap();
    assert!(incoming(&vault, "Target.md").is_empty());
}
