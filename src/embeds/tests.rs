use super::{
    Ancestry, Budget, Key, Limits, ResolveError, SelectError, Stop, Target, group_spans, requests,
    resolve,
};
use crate::{prepared::Prepared, vault::Vault};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

fn target(fragment: Option<&str>) -> Target {
    Target {
        path: PathBuf::from("Target.md"),
        fragment: fragment.map(Arc::from),
    }
}
fn key(path: &str, source: std::ops::Range<usize>) -> Key {
    Key {
        path: PathBuf::from(path),
        source,
    }
}

#[test]
fn formatted_labels_group_once_but_adjacent_occurrences_keep_their_identity() {
    let source = "Before ![**bold** and *italic*](Target.md#Heading) ![[Target]]![[Target]] after [ordinary](Target.md). `![[Code]]`";
    let prepared = Prepared::new(source);
    let row = &prepared.rows[0];
    let grouped = requests(row);
    assert_eq!(grouped.len(), 3);
    assert_eq!(&row.text[grouped[0].range.clone()], "bold and italic");
    assert_eq!(grouped[0].target.as_ref(), "Target.md#Heading");
    assert_eq!(grouped[1].target.as_ref(), "Target");
    assert_ne!(grouped[1].image_start, grouped[2].image_start);
    assert!(
        grouped
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start)
    );
    assert_eq!(group_spans(&row.spans), grouped);
}

#[test]
fn resolution_uses_origin_metadata_without_reading_files_and_leaves_images_alone() {
    let folder = tempfile::tempdir().unwrap();
    fs::create_dir(folder.path().join("Folder")).unwrap();
    fs::create_dir(folder.path().join("Other")).unwrap();
    for path in [
        "Folder/Host.md",
        "Folder/Local.md",
        "Local.md",
        "Target Note.md",
        "Other/Unique.md",
    ] {
        fs::write(folder.path().join(path), "body").unwrap();
    }
    fs::write(folder.path().join("Folder/picture.png"), b"not loaded").unwrap();
    fs::write(folder.path().join("Folder/document.pdf"), b"not loaded").unwrap();
    let vault = Vault::discover(folder.path(), &AtomicBool::new(false)).unwrap();
    assert!(!vault.content_indexed());
    fs::remove_file(folder.path().join("Target Note.md")).unwrap();
    let origin = Path::new("Folder/Host.md");
    for (text, expected, fragment) in [
        ("Local", "Folder/Local.md", None),
        ("/Local", "Local.md", None),
        ("Unique", "Other/Unique.md", None),
        (
            "../Target%20Note.md#Éclair",
            "Target Note.md",
            Some("Éclair"),
        ),
        ("#^block", "Folder/Host.md", Some("^block")),
    ] {
        let resolved = resolve(&vault, origin, text).unwrap().unwrap();
        assert_eq!(resolved.path, Path::new(expected));
        assert_eq!(resolved.fragment.as_deref(), fragment);
    }
    for image in [
        "picture.png",
        "missing.JPG",
        "document.pdf",
        "https://example.com/Note.md",
        "//example.com/a",
        "data:image/png;base64,abc",
    ] {
        assert_eq!(resolve(&vault, origin, image), Ok(None), "{image}");
    }
    assert_eq!(
        resolve(&vault, origin, "Missing"),
        Err(ResolveError::MissingNote)
    );
    assert_eq!(
        resolve(&vault, origin, "../../outside.md"),
        Err(ResolveError::MissingNote)
    );
    // Nested callers use the resolved embedded path as their next origin.
    let embedded = resolve(&vault, origin, "Unique").unwrap().unwrap();
    assert_eq!(
        resolve(&vault, &embedded.path, "../Target%20Note.md")
            .unwrap()
            .unwrap()
            .path,
        Path::new("Target Note.md")
    );
}

#[test]
fn full_heading_and_block_selections_share_complete_context_and_exact_source_bounds() {
    let source = "---\ntitle: Metadata\n---\n# Intro\n\nOutside[^n].\n\n# **Éclair**\n\n[Reference][dest] and selected[^n].\n\n## Child\n\nUnicode 👩‍💻 paragraph ^block\n\n# End\n\n[dest]: Folder/Linked.md\n[^n]: Definition outside selected section.\n";
    let prepared = Arc::new(Prepared::new(source));
    let full = target(None).select(prepared.clone(), source.len()).unwrap();
    assert_eq!(full.source, 0..source.len());
    assert_eq!(full.rows, 0..prepared.rows.len());
    assert!(Arc::ptr_eq(&full.prepared, &prepared));
    let section = target(Some("%C3%89clair"))
        .select(prepared.clone(), source.len())
        .unwrap();
    assert_eq!(
        section.source,
        source.find("# **Éclair").unwrap()..source.find("# End").unwrap()
    );
    assert_eq!(section.source, prepared.anchors.target("Éclair").unwrap());
    let rows = &section.prepared.rows[section.rows.clone()];
    assert_eq!(rows[0].text.as_ref(), "Éclair");
    assert!(rows.iter().any(|row| row.text.contains("Child")));
    assert!(!rows.iter().any(|row| row.text.as_ref() == "End"));
    assert!(
        rows.iter()
            .flat_map(|row| &row.spans)
            .any(|span| span.style.target.as_deref() == Some("Folder/Linked.md"))
    );
    assert_eq!(
        section.prepared.footnotes.definitions[0].references.len(),
        2
    );
    assert!(section.prepared.footnotes.target_row(1).is_some());
    let block = target(Some("^block"))
        .select(prepared.clone(), source.len())
        .unwrap();
    assert_eq!(block.source, prepared.anchors.blocks["block"].source);
    assert_eq!(block.source.start, source.find("Unicode").unwrap());
    assert!(
        block.prepared.rows[block.rows.clone()]
            .iter()
            .all(|row| row.source.start < block.source.end && row.source.end > block.source.start)
    );
    assert!(Arc::ptr_eq(&section.prepared, &block.prepared));
    assert_eq!(block.key(), key("Target.md", block.source.clone()));
}

#[test]
fn block_selection_keeps_all_bounded_table_rows_and_complete_document_context() {
    let source = format!(
        "# Before\n\n| A | B |\n| - | - |\n| {} | Last |\n\n^table\n\n# After\n",
        "é ".repeat(3000)
    );
    let prepared = Arc::new(Prepared::new(&source));
    let selected = target(Some("^table"))
        .select(prepared.clone(), source.len())
        .unwrap();
    let rows = &selected.prepared.rows[selected.rows.clone()];
    assert!(rows.len() > 3);
    assert!(rows.iter().all(|row| row.table.is_some()));
    assert!(rows.last().unwrap().table.as_ref().unwrap().last);
    assert_eq!(selected.source, prepared.anchors.blocks["table"].source);
    assert!(rows[0].source.start >= selected.source.start);
    assert!(rows.last().unwrap().source.end <= selected.source.end);
}

#[test]
fn incomplete_missing_and_mismatched_preparations_are_explicit_errors() {
    let source = "# Heading\n\nBody ^block\n";
    let prepared = Arc::new(Prepared::new(source));
    assert_eq!(
        target(Some("Missing"))
            .select(prepared.clone(), source.len())
            .unwrap_err(),
        SelectError::MissingFragment
    );
    assert_eq!(
        target(Some("^missing"))
            .select(prepared.clone(), source.len())
            .unwrap_err(),
        SelectError::MissingFragment
    );
    assert_eq!(
        target(None).select(prepared, 1).unwrap_err(),
        SelectError::InvalidSourceLength
    );
    let opening = Arc::new(Prepared::opening(&format!(
        "# First\n\n{}",
        "text\n".repeat(4000)
    )));
    assert!(!opening.complete);
    assert_eq!(
        target(None).select(opening, 20000).unwrap_err(),
        SelectError::NotReady
    );
    let empty = Arc::new(Prepared::new(""));
    let selected = target(None).select(empty, 0).unwrap();
    assert!(selected.source.is_empty());
    assert!(selected.rows.is_empty());
}

#[test]
fn ancestry_uses_selected_source_identity_and_shared_sibling_instance_budget() {
    let root = Ancestry::root(key("A.md", 0..100));
    let mut budget = Budget::new(Limits {
        depth: 3,
        instances: 4,
    });
    assert!(matches!(
        root.enter(key("A.md", 0..100), &mut budget),
        Err(Stop::Cycle)
    ));
    let section = root.enter(key("A.md", 20..40), &mut budget).unwrap();
    let nested = section.enter(key("B.md", 0..80), &mut budget).unwrap();
    assert!(matches!(
        nested.enter(key("A.md", 20..40), &mut budget),
        Err(Stop::Cycle)
    ));
    let separate_section = nested.enter(key("A.md", 50..70), &mut budget).unwrap();
    assert!(matches!(
        separate_section.enter(key("C.md", 0..10), &mut budget),
        Err(Stop::Depth)
    ));
    root.enter(key("B.md", 0..80), &mut budget).unwrap();
    assert_eq!(budget.used(), 4);
    assert_eq!(budget.remaining(), 0);
    assert!(matches!(
        root.enter(key("B.md", 0..80), &mut budget),
        Err(Stop::Instances)
    ));
    assert_eq!(budget.used(), 4);
}
