use crate::{live::Kind, prepared::Prepared};

fn projection(source: &str, prepared: &Prepared, needle: &str) -> crate::live::Line {
    let at = source.find(needle).unwrap();
    let start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = source[at..].find('\n').map_or(source.len(), |i| at + i);
    prepared
        .live
        .project(source, start..end, false, false, false)
}

#[test]
fn definitions_show_numbers_styles_and_backlinks_with_exact_unicode_source_mapping() {
    let source = "Before[^é] and inline ^[inline].\n\n[^é]: A **définition** with [link](Target.md).\n\n    Next *paragraph*.\n\nAfter\n";
    let prepared = Prepared::new(source);
    let line = projection(source, &prepared, "[^é]:");
    assert_eq!(line.text, "1. A définition with link.");
    assert_eq!(line.source_index(0), 0);
    let local = source.find("[^é]:").unwrap();
    let display = line.text.find("définition").unwrap();
    assert_eq!(
        local + line.source_index(display),
        source.find("définition").unwrap()
    );
    assert!(
        line.spans
            .iter()
            .any(|s| s.style.bold && &line.text[s.range.clone()] == "définition")
    );
    assert!(
        line.spans
            .iter()
            .any(|s| s.style.target.as_deref() == Some("Target.md"))
    );
    assert_eq!(
        line.spans[0].style.target,
        Some(format!(
            "{}{}",
            crate::footnotes::BACK_PREFIX,
            source.find("[^é]").unwrap()
        ))
    );
    assert!(line.mapping.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(
        projection(source, &prepared, "    Next").text,
        "Next paragraph."
    );
    assert_eq!(prepared.live.footnote_at(local), Some(1));
    assert_eq!(
        prepared.live.footnote_at(source.find("After").unwrap()),
        None
    );
}

#[test]
fn editing_a_definition_reveals_its_complete_raw_multiline_source() {
    let source = "Text[^n].\n\n[^n]: First\n\n    Continued **text**.\n\nAfter\n";
    let prepared = Prepared::new(source);
    let at = source.find("Continued").unwrap();
    let reveal = prepared.live.reveal_range(at..at + 1);
    assert!(reveal.start <= source.find("[^n]:").unwrap());
    assert!(reveal.end >= at + "Continued **text**.".len());
    let start = source.find("[^n]:").unwrap();
    let end = source[start..].find('\n').unwrap() + start;
    assert_eq!(
        prepared
            .live
            .project(source, start..end, true, false, false)
            .text,
        "[^n]: First"
    );
    let code = "    literal **code**";
    // Actual code ranges always bypass definition transformations.
    assert_eq!(
        prepared
            .live
            .project(code, 0..code.len(), false, true, false)
            .kind,
        Kind::Code
    );
}

#[test]
fn unused_and_commented_definitions_stay_hidden_without_hiding_neighbor_text() {
    let source = "%%hidden%%Text[^used].\n\n[^used]: %%comment%%Visible\n\n[^unused]: Hide me\n\n    And me\n\nAfter\n\n`[^fake]: literal`\n";
    let prepared = Prepared::new(source);
    assert_eq!(projection(source, &prepared, "[^used]:").text, "1. Visible");
    assert!(projection(source, &prepared, "[^unused]:").text.is_empty());
    assert!(projection(source, &prepared, "    And me").text.is_empty());
    assert_eq!(projection(source, &prepared, "After").text, "After");
    assert_eq!(
        projection(source, &prepared, "`[^fake]").text,
        "[^fake]: literal"
    );
    assert!(prepared.live.is_hidden(source.find("Hide me").unwrap()));
    assert_eq!(
        prepared
            .live
            .footnote_at(source.find("[^unused]:").unwrap()),
        Some(0)
    );
}

#[test]
fn long_definition_chunks_reconstruct_complete_text_without_duplicate_numbers() {
    let source = format!(
        "Text[^long].\n\n[^long]: {} **tail**\n\nAfter",
        "é👩‍💻 ".repeat(2000)
    );
    let prepared = Prepared::new(&source);
    let start = source.find("[^long]:").unwrap();
    let end = source[start..].find('\n').unwrap() + start;
    let physical = prepared
        .live
        .project(&source, start..end, false, false, false);
    let rebuilt: String = crate::document::source_rows(&source[start..end])
        .into_iter()
        .map(|range| physical.slice(range).text)
        .collect();
    assert_eq!(rebuilt, physical.text);
    assert!(physical.text.starts_with("1. é👩‍💻 "));
    assert_eq!(physical.text.matches("1. ").count(), 1);
    for (at, _) in physical.text.char_indices() {
        assert!(source.is_char_boundary(start + physical.source_index(at)));
    }
}

#[test]
fn nested_definitions_restore_the_enclosing_note_and_resolve_innermost_intervals() {
    let source = "Main[^outer] and [^inner].\n\n[^outer]: Outer\n\n    > Quote\n    > [^inner]: Nested\n    >\n    > After inner\n\n    After outer\n\nEnd\n";
    let prepared = Prepared::new(source);
    for (needle, label) in [
        ("Outer", "outer"),
        ("Quote", "outer"),
        ("Nested", "inner"),
        ("After inner", "outer"),
        ("After outer", "outer"),
    ] {
        let row = prepared
            .rows
            .iter()
            .position(|row| row.text.as_ref() == needle)
            .unwrap();
        assert_eq!(
            prepared
                .footnotes
                .at_row(row)
                .map(|note| note.label.as_ref()),
            Some(label),
            "{needle}"
        );
    }
    let inner = source.find("[^inner]:").unwrap();
    assert_eq!(prepared.live.footnote_at(inner), Some(2));
    assert_eq!(
        prepared
            .live
            .footnote_at(source.find("After inner").unwrap()),
        Some(1)
    );
    assert_eq!(
        prepared
            .live
            .footnote_at(source.find("After outer").unwrap()),
        Some(1)
    );
    assert_eq!(prepared.live.footnote_at(source.find("End").unwrap()), None);
    let reveal = prepared.live.reveal_range(inner..inner + 1);
    assert_eq!(reveal.start, source.find("[^outer]:").unwrap());
    assert!(reveal.end > source.find("After outer").unwrap());
    let line = projection(source, &prepared, "[^inner]:");
    assert_eq!(line.text, "2. Nested");
    for (index, _) in line.text.char_indices() {
        let start = source[..inner].rfind('\n').unwrap() + 1;
        assert!(source.is_char_boundary(start + line.source_index(index)));
    }
}

#[test]
fn referenced_nested_definitions_remain_visible_inside_an_unused_parent() {
    let source = "Main[^inner].\n\n[^outer]: Hidden parent\n\n    > [^inner]: Visible child\n    >\n\n    Hidden tail\n\nEnd\n";
    let prepared = Prepared::new(source);
    assert!(projection(source, &prepared, "[^outer]:").text.is_empty());
    assert_eq!(
        projection(source, &prepared, "[^inner]:").text,
        "1. Visible child"
    );
    assert!(
        !prepared
            .live
            .is_hidden(source.find("Visible child").unwrap())
    );
    assert!(prepared.live.is_hidden(source.find("Hidden tail").unwrap()));
    assert_eq!(
        prepared
            .live
            .footnote_at(source.find("Visible child").unwrap()),
        Some(1)
    );
    assert_eq!(
        prepared
            .live
            .footnote_at(source.find("Hidden tail").unwrap()),
        Some(0)
    );
}

#[test]
fn duplicate_labels_keep_both_source_definitions_and_restore_the_parent_after_a_nested_copy() {
    let source = "Main[^same].\n\n[^same]: First\n\n    > [^same]: Nested copy\n    >\n\n    Restored parent\n\n[^same]: Last\n\nEnd\n";
    let prepared = Prepared::new(source);
    for needle in ["First", "Nested copy", "Restored parent", "Last"] {
        let row = prepared
            .rows
            .iter()
            .position(|row| row.text.as_ref() == needle)
            .unwrap();
        assert_eq!(
            prepared.footnotes.at_row(row).map(|note| note.number),
            Some(1),
            "{needle}"
        );
    }
    for needle in ["[^same]: First", "[^same]: Nested", "[^same]: Last"] {
        assert!(
            projection(source, &prepared, needle)
                .text
                .starts_with("1. ")
        );
    }
    assert_eq!(
        prepared
            .live
            .footnote_at(source.find("Restored parent").unwrap()),
        Some(1)
    );
    assert_eq!(prepared.live.footnote_at(source.find("End").unwrap()), None);
}
