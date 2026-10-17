use crate::{callouts::Projection, prepared::Prepared};

#[test]
fn references_number_in_use_order_and_definitions_display_at_end() {
    let source = "---\ntitle: Hidden\n---\n[^a]: First **definition**.\n\nStart[^b] then[^a] and again[^a].\n\n[^b]: Second with [link](Target.md).\n\n[^unused]: Not referenced.\n\n# End\n";
    let prepared = Prepared::new(source);
    let mut projection = Projection::default();
    projection.rebuild(prepared.rows.len(), &prepared.callouts);
    projection.with_footnotes(prepared.rows.len(), &prepared.footnotes, &prepared.callouts);
    let texts = (0..projection.len(prepared.rows.len()))
        .map(|i| prepared.rows[projection.row(i)].text.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            "Start[1] then[2] and again[2-1].",
            "End",
            "Second with link.",
            "First definition."
        ]
    );
    let first = &prepared.footnotes.definitions[0];
    assert_eq!(first.label.as_ref(), "b");
    assert_eq!(first.references, [source.find("[^b] then").unwrap()]);
    let second = &prepared.footnotes.definitions[1];
    assert_eq!(
        second.references,
        [
            source.find("[^a] and").unwrap(),
            source.find("[^a].").unwrap()
        ]
    );
    for i in 0..projection.len(prepared.rows.len()) {
        assert_eq!(projection.list_index(projection.row(i)), i);
    }
    assert!(
        prepared
            .rows
            .windows(2)
            .all(|p| p[0].source.end <= p[1].source.start)
    );
    assert!(
        prepared
            .rows
            .iter()
            .flat_map(|r| r.spans.iter())
            .any(|s| s.style.bold)
    );
    let reference = prepared
        .rows
        .iter()
        .find(|r| r.text.starts_with("Start"))
        .unwrap();
    assert_eq!(
        reference.spans[0].style.target.as_deref(),
        Some("opalglass-footnote:1")
    );
}

#[test]
fn folded_footnote_callouts_keep_their_own_projection_and_context() {
    let source = "Main[^n] and outer[^outer].\n\n[^n]: First\n\n    > [!tip]- Details\n    > Hidden body\n\n> [!note]- Outer\n>\n> [^outer]: Detached definition\n>\n> Ordinary hidden body\n";
    let p = Prepared::new(source);
    assert_eq!(p.callouts.len(), 2);
    let mut projection = Projection::default();
    projection.rebuild(p.rows.len(), &p.callouts);
    projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
    let texts = (0..projection.len(p.rows.len()))
        .map(|i| p.rows[projection.row(i)].text.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            "Main[1] and outer[2].",
            "Outer",
            "First",
            "Details",
            "Detached definition"
        ]
    );
    let inner = &p.callouts[0];
    assert_eq!(
        projection.row(projection.list_index(inner.rows.end - 1)),
        inner.rows.start
    );
    let detached = p.footnotes.target_row(2).unwrap();
    assert_eq!(p.rows[detached].quotes, 0);
    assert_eq!(p.rows[detached].indent, 0);
    projection.overrides.insert(inner.source.start, false);
    projection.rebuild(p.rows.len(), &p.callouts);
    projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
    assert!(
        (0..projection.len(p.rows.len()))
            .any(|i| p.rows[projection.row(i)].text.as_ref() == "Hidden body")
    );
    for i in 0..projection.len(p.rows.len()) {
        assert_eq!(projection.list_index(projection.row(i)), i);
    }
}

#[test]
fn definition_references_follow_main_text_and_cycles_are_bounded() {
    let source = "[^a]: See[^b] and self[^a].\n\nMain[^a].\n\n[^b]: Last.\n\n[^unused]: Has[^ghost].\n\n[^ghost]: Hidden.";
    let p = Prepared::new(source);
    assert_eq!(
        p.footnotes
            .definitions
            .iter()
            .filter(|d| d.number > 0)
            .count(),
        2
    );
    assert_eq!(
        p.rows[p.footnotes.target_row(1).unwrap()].text.as_ref(),
        "See[2] and self[1-1]."
    );
    assert_eq!(
        p.rows[p.footnotes.target_row(2).unwrap()].text.as_ref(),
        "Last."
    );
    assert!(p.rows.iter().any(|r| r.text.as_ref() == "Main[1]."));
}

#[test]
fn code_tables_duplicate_definitions_and_unused_definitions_never_lose_other_text() {
    let source = "A[^code] and B[^table].\n\n[^code]:\n    ```rust\n    let value = 1;\n    ```\n\nOrdinary paragraph\n\n[^table]:\n    | a | b |\n    | - | - |\n    | c | d |\n\n[^code]: Duplicate\n\nAfter\n";
    let p = Prepared::new(source);
    let code = p.rows.iter().find_map(|r| r.code.as_ref()).unwrap();
    assert_eq!(code.as_ref(), "let value = 1;\n");
    assert!(p.rows.iter().any(|r| r.table.is_some()));
    let mut projection = Projection::default();
    projection.rebuild(p.rows.len(), &p.callouts);
    projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
    let visible = (0..projection.len(p.rows.len()))
        .map(|i| p.rows[projection.row(i)].text.as_ref())
        .collect::<Vec<_>>();
    assert!(visible.contains(&"Ordinary paragraph"));
    assert!(visible.contains(&"After"));
    assert!(
        p.rows
            .iter()
            .all(|r| source.get(r.source.clone()).is_some())
    );
}

#[test]
fn inline_footnotes_match_reference_fixture_and_preserve_formatting() {
    let source = "A reference[^n] and another[^n]. Inline ^[An inline footnote].\n\n[^n]: A **footnote** with detail.\n\nFormatted ^[**bold** and `code` and [link](Target.md)].\n\nEscaped \\^[literal] and `^[code]`.\n\nNested ^[outer [brackets] end].\n\nMultiline ^[first\nsecond].\n\nEmpty ^[].";
    let p = Prepared::new(source);
    assert_eq!(p.footnotes.definitions.len(), 6);
    let mut projection = Projection::default();
    projection.rebuild(p.rows.len(), &p.callouts);
    projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
    let texts = (0..projection.len(p.rows.len()))
        .map(|i| p.rows[projection.row(i)].text.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            "A reference[1] and another[1-1]. Inline [2].",
            "Formatted [3].",
            "Escaped ^[literal] and ^[code].",
            "Nested [4].",
            "Multiline [5].",
            "Empty [6].",
            "A footnote with detail.",
            "An inline footnote",
            "bold and code and link",
            "outer [brackets] end",
            "first\nsecond",
            ""
        ]
    );
    let formatted = &p.rows[p.footnotes.target_row(3).unwrap()];
    assert!(formatted.spans.iter().any(|s| s.style.bold));
    assert!(formatted.spans.iter().any(|s| s.style.code));
    assert!(
        formatted
            .spans
            .iter()
            .any(|s| s.style.target.as_deref() == Some("Target.md"))
    );
    for pair in p.rows.windows(2) {
        assert!(
            pair[0].source.end <= pair[1].source.start,
            "{:?} {:?}",
            pair[0].source,
            pair[1].source
        );
    }
    let reference_end = p.rows[0].source.end;
    assert_eq!(p.row_at_source(reference_end), 0);
}

#[test]
fn unmatched_inline_markers_are_bounded_and_code_stays_literal() {
    let malformed = "^[".repeat(20_000);
    let p = Prepared::new(&malformed);
    assert!(p.footnotes.definitions.is_empty());
    let p = Prepared::new(
        "`^[literal]`\n\n```\n^[also literal]\n```\n\nReal ^[é &amp; `]` [site](Target.md)].",
    );
    assert_eq!(p.footnotes.definitions.len(), 1);
    assert_eq!(
        p.rows[p.footnotes.target_row(1).unwrap()].text.as_ref(),
        "é & ] site"
    );
}
