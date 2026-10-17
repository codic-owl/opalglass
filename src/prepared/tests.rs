use super::*;
#[test]
fn code_colors_keep_exact_source_ranges_inside_containers_and_hidden_metadata() {
    let source = "---\ntitle: Example\n---\n%%hidden%%\n> ```js\n> const café = \"你好\";\n> /* comment\n> continuation */\n> ```\n\n- Item\n\n  ```python\n  print(\"hello\")\n  ```\n";
    let opening = Prepared::opening(source);
    assert!(opening.complete);
    assert!(!opening.analysis_complete());
    assert!(opening.source_syntax.is_empty());
    assert!(
        opening
            .rows
            .iter()
            .all(|r| r.spans.iter().all(|s| s.style.syntax_color.is_none()))
    );
    let p = Prepared::new(source);
    assert!(p.analysis_complete());
    assert!(!p.source_syntax.is_empty());
    assert!(
        p.source_syntax
            .windows(2)
            .all(|s| s[0].range.end <= s[1].range.start)
    );
    assert!(
        p.source_syntax
            .iter()
            .all(|s| source.get(s.range.clone()).is_some())
    );
    let painted = p
        .source_syntax
        .iter()
        .map(|s| &source[s.range.clone()])
        .collect::<String>();
    assert_eq!(
        painted,
        "const café = \"你好\";\n/* comment\ncontinuation */\nprint(\"hello\")\n"
    );
    assert_eq!(
        p.rows
            .iter()
            .filter_map(|r| r.code.as_ref())
            .next()
            .unwrap()
            .as_ref(),
        "const café = \"你好\";\n/* comment\ncontinuation */\n"
    );
    assert!(Prepared::opening("# Plain\n\nNo code").analysis_complete());
}

#[test]
fn code_chunks_keep_multiline_color_and_complete_copy_text() {
    let code = format!("/*\n{}*/\nconst x = 42;\n", "é comment\n".repeat(400));
    let source = format!("```javascript\n{code}```\n");
    let p = Prepared::new(&source);
    let rows = p
        .rows
        .iter()
        .filter(|r| matches!(r.kind, Kind::Code))
        .collect::<Vec<_>>();
    assert!(rows.len() > 2);
    assert_eq!(
        rows.iter().map(|r| r.text.as_ref()).collect::<String>(),
        code
    );
    let color = rows[0].spans[0].style.syntax_color;
    assert_eq!(rows[1].spans[0].style.syntax_color, color);
    assert_eq!(rows[1].code.as_deref(), Some(code.as_str()));
    for row in rows {
        for span in &row.spans {
            assert!(row.text.get(span.range.clone()).is_some());
        }
    }
}

#[test]
fn preview_source_ranges_track_blocks_and_long_chunks() {
    let source = format!(
        "\r\n# Héading\r\n\r\nA **bold** paragraph\r\ncontinued.\r\n\r\n> quote\r\n- [x] task\r\n\r\n```rust\r\n{}\r\n```\r\n\r\n---\r\n\r\n# End",
        "let x = 你好;\r\n".repeat(500)
    );
    let prepared = Prepared::new(&source);
    assert_eq!(
        prepared.rows[0].source.start,
        source.find("# Héading").unwrap()
    );
    assert_eq!(
        prepared.rows[1].source.start,
        source.find("A **bold**").unwrap()
    );
    assert_eq!(
        prepared.rows[2].source.start,
        source.find("> quote").unwrap()
    );
    assert_eq!(prepared.rows[3].source.start, source.find("- [x]").unwrap());
    let code: Vec<_> = prepared
        .rows
        .iter()
        .filter(|r| matches!(r.kind, Kind::Code))
        .collect();
    assert!(code.len() > 2);
    assert_eq!(code[0].source.start, source.find("```rust").unwrap());
    for pair in code.windows(2) {
        assert_eq!(pair[0].source.end, pair[1].source.start);
    }
    assert_eq!(
        prepared.rows.last().unwrap().source,
        source.find("# End").unwrap()..source.len()
    );
    assert!(
        prepared
            .rows
            .windows(2)
            .all(|r| r[0].source.end <= r[1].source.start)
    );
    let unfinished = Prepared::new("# Before\n\n```\nhello");
    assert_eq!(unfinished.rows.last().unwrap().source, 10..19);
}

#[test]
fn opening_preview_is_bounded_and_short_notes_are_complete() {
    let short = "# Hello\n\nText with [[Note]] and #tag.";
    let small = Prepared::opening(short);
    assert!(small.complete);
    assert_eq!(small.tags, vec!["tag"]);
    assert_eq!(small.links.len(), 1);

    let large = format!("{short}\n\n{}", "More text 👩‍💻 e\u{301}.\n\n".repeat(10_000));
    let initial = Prepared::opening(&large);
    let full = Prepared::new(&large);
    assert!(!initial.complete);
    assert!(full.complete);
    assert!(initial.rows.len() < full.rows.len() / 10);
    for (early, complete) in initial.rows.iter().zip(&full.rows).take(10) {
        assert_eq!(early.text, complete.text);
    }
    // A huge first line must also be bounded and tolerate a multibyte cut.
    let initial = Prepared::opening(&"界".repeat(10_000));
    assert!(!initial.complete);
    assert!(initial.rows.iter().map(|r| r.text.len()).sum::<usize>() <= 8192);
    let initial = Prepared::opening(&format!("```\n{}\n```", "code\n".repeat(10_000)));
    assert!(!initial.complete);
    assert!(initial.rows[0].code.as_ref().unwrap().len() < 8192);
}

#[test]
fn code_chunks_share_complete_literal_copy_text() {
    let code = "let message = \"你好 **literal**\";\n".repeat(100);
    let prepared = Prepared::new(&format!("```rust\n{code}```\n"));
    assert!(prepared.rows.len() > 1);
    let first = prepared.rows[0].code.as_ref().unwrap();
    assert_eq!(first.as_ref(), code);
    for row in &prepared.rows {
        assert!(
            row.spans
                .iter()
                .all(|s| s.style.target.is_none() && !s.style.image && s.style.highlight.is_none())
        );
        assert!(Arc::ptr_eq(first, row.code.as_ref().unwrap()));
    }
}
#[test]
fn chunks_preserve_unicode_spans_targets_and_heading_locations() {
    let label = "ş😀e\u{301} ".repeat(700);
    let source = format!(
        "# First\n\n**[[Target|{label}]]**\n\n# Last\n\n#tag `#ignored`\n```\n#hidden\n```"
    );
    let prepared = Prepared::new(&source);
    let chunks: Vec<_> = prepared
        .rows
        .iter()
        .filter(|r| {
            matches!(r.kind, Kind::Paragraph)
                && r.spans
                    .iter()
                    .any(|s| s.style.target.as_deref() == Some("Target"))
        })
        .collect();
    assert!(chunks.len() > 1);
    assert_eq!(
        chunks.iter().map(|r| r.text.as_ref()).collect::<String>(),
        label.trim()
    );
    for row in chunks {
        assert!(source.get(row.source.clone()).is_some());
        for span in &row.spans {
            assert!(row.text.get(span.range.clone()).is_some());
            assert!(span.style.bold);
            assert_eq!(span.style.target.as_deref(), Some("Target"));
        }
    }
    assert_eq!(prepared.tags, markdown::tags(&source));
    let (_, last) = prepared.headings.last().unwrap();
    assert!(matches!(prepared.rows[*last].kind, Kind::Heading(1)));
    assert_eq!(prepared.rows[*last].text.as_ref(), "Last");
}
