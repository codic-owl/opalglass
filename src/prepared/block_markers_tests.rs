//! Block identifiers are source metadata, with one shared visibility map.
use super::Prepared;

#[test]
fn recognized_block_markers_hide_without_changing_source_targets_or_literal_carets() {
    let source = "%%lead%%Paragraph **é 👩‍💻** [link](Target.md) ^para\n\n- [ ] Task ^task\n- Last\n\n^list\n\n```txt\nLiteral ^fenced\n```\n\n^code\n\nInline `^inline` and escaped \\^escaped\n\nInvalid ^bad_id\n\nNonterminal ^middle text\n";
    let prepared = Prepared::new(source);
    for (id, beginning) in [
        ("para", "Paragraph"),
        ("task", "- [ ]"),
        ("list", "- [ ]"),
        ("code", "```txt"),
    ] {
        let block = &prepared.anchors.blocks[id];
        assert_eq!(&source[block.marker.clone()], format!("^{id}"));
        assert_eq!(block.source.start, source.find(beginning).unwrap());
        assert!(prepared.live.is_hidden(block.marker.start));
        assert!(
            prepared
                .rows
                .iter()
                .all(|row| !row.text.contains(&format!("^{id}"))),
            "recognized {id} marker leaked into Reading"
        );
    }
    let paragraph = prepared
        .rows
        .iter()
        .find(|row| row.text.starts_with("Paragraph"))
        .unwrap();
    assert_eq!(paragraph.text.trim_end(), "Paragraph é 👩‍💻 link");
    assert!(
        paragraph
            .spans
            .iter()
            .any(|span| span.style.target.as_deref() == Some("Target.md"))
    );
    assert!(
        prepared
            .rows
            .iter()
            .any(|row| row.code.as_deref() == Some("Literal ^fenced\n"))
    );
    for literal in ["^inline", "^escaped", "^bad_id", "^middle"] {
        assert!(prepared.rows.iter().any(|row| row.text.contains(literal)));
    }
    for row in &prepared.rows {
        assert!(source.get(row.source.clone()).is_some());
        if let Some(task) = &row.task {
            assert_eq!(&source[task.clone()], "[ ]");
        }
    }
}

#[test]
fn block_marker_live_projection_reveals_raw_syntax_and_keeps_unicode_mappings() {
    let source = "Before\n\nA **é 👩‍💻** tail ^block\n\n^standalone\n\nEnd\n";
    let prepared = Prepared::new(source);
    let start = source.find("A **").unwrap();
    let end = start + source[start..].find('\n').unwrap();
    let projection = prepared
        .live
        .project(source, start..end, false, false, false);
    assert_eq!(projection.text, "A é 👩‍💻 tail");
    for (at, ch) in projection.text.char_indices() {
        assert_eq!(
            source[projection.source_index(at) + start..].chars().next(),
            Some(ch)
        );
    }
    let raw = prepared
        .live
        .project(source, start..end, true, false, false);
    assert_eq!(raw.text, &source[start..end]);
    assert!(raw.text.ends_with("^block"));
    let standalone = source.find("^standalone").unwrap();
    assert!(
        prepared
            .live
            .project(
                source,
                standalone..standalone + "^standalone".len(),
                false,
                false,
                false
            )
            .text
            .is_empty()
    );
    let marker = &prepared.anchors.blocks["block"].marker;
    let reveal = prepared.live.reveal_range(marker.start..marker.start + 1);
    assert!(reveal.start <= marker.start && reveal.end >= marker.end);
}

#[test]
fn duplicate_block_markers_hide_every_occurrence_and_keep_the_first_target() {
    let source = "%%lead%%First **é** ^same\n\nSecond **👩‍💻** ^same\n\n- Item\n\n^same\n\nEnd\n";
    let prepared = Prepared::new(source);
    assert_eq!(prepared.anchors.blocks.len(), 1);
    assert_eq!(prepared.anchors.markers.len(), 3);
    let target = prepared.anchors.target("^same").unwrap();
    assert_eq!(target.start, source.find("First").unwrap());
    assert!(target.end <= source.find("Second").unwrap());
    assert!(prepared.rows.iter().all(|row| !row.text.contains("^same")));
    for marker in &prepared.anchors.markers {
        assert_eq!(&source[marker.clone()], "^same");
        assert!(prepared.live.is_hidden(marker.start));
        let start = source[..marker.start].rfind('\n').map_or(0, |n| n + 1);
        let end = source[marker.end..].find('\n').unwrap() + marker.end;
        let range = start..end;
        let line = prepared
            .live
            .project(source, range.clone(), false, false, false);
        assert!(!line.text.contains("^same"));
        assert_eq!(
            prepared
                .live
                .project(source, range.clone(), true, false, false)
                .text,
            source[range]
        );
        for (at, _) in line.text.char_indices() {
            assert!(source.is_char_boundary(start + line.source_index(at)));
        }
    }
    let standalone = source.rfind("^same").unwrap();
    assert!(
        prepared
            .live
            .project(source, standalone..standalone + 5, false, false, false)
            .text
            .is_empty()
    );
}
