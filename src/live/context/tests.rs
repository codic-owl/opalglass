use crate::{live::Line, prepared::Prepared};

fn project(source: &str, prepared: &Prepared, needle: &str) -> (usize, Line) {
    let start = source.find(needle).unwrap();
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |n| start + n);
    (
        start,
        prepared
            .live
            .project(source, start..end, false, false, false),
    )
}

#[test]
fn document_reference_links_preserve_labels_styles_and_unicode_offsets() {
    let source = "%%hidden%%[**é 👩‍💻**][mixed label] and [short] and [collapsed][]%%tail%%\n\n[mixed label]: <Folder/Target.md#A Heading> \"title\"\n[short]: Target.md\n[collapsed]: Other.md\n";
    let prepared = Prepared::new(source);
    let (_, line) = project(source, &prepared, "%%hidden%%");
    assert_eq!(line.text, "é 👩‍💻 and short and collapsed");
    let targets: Vec<_> = line
        .spans
        .iter()
        .filter_map(|s| s.style.target.as_deref())
        .collect();
    assert_eq!(
        targets,
        vec!["Folder/Target.md#A Heading", "Target.md", "Other.md"]
    );
    assert!(line.spans[0].style.bold);
    for (at, _) in line.text.char_indices() {
        assert!(source.is_char_boundary(line.source_index(at)));
    }
    assert_eq!(line.source_index(0), source.find('é').unwrap());
    let (start, definition) = project(source, &prepared, "[short]:");
    assert!(definition.text.is_empty());
    let end = source[start..].find('\n').unwrap() + start;
    assert_eq!(
        prepared
            .live
            .project(source, start..end, true, false, false)
            .text,
        &source[start..end]
    );
}

#[test]
fn multiline_comments_hide_only_comment_contents_and_reveal_exact_source() {
    let source = "Before %%start\nsecret [[Missing]] é\nend%% after **bold**\n\n`%%literal%%`\n\n[undefined][unknown]\n";
    let p = Prepared::new(source);
    assert_eq!(project(source, &p, "Before").1.text, "Before");
    assert_eq!(project(source, &p, "secret").1.text, "");
    assert_eq!(project(source, &p, "end%%").1.text, " after bold");
    assert_eq!(project(source, &p, "`%%literal%%`").1.text, "%%literal%%");
    assert_eq!(
        project(source, &p, "[undefined]").1.text,
        "[undefined][unknown]"
    );
    let start = source.find("secret").unwrap();
    let end = source[start..].find('\n').unwrap() + start;
    let revealed = p.live.reveal_range(start..end);
    assert!(revealed.start <= source.find("%%start").unwrap());
    assert!(revealed.end >= source.find("end%%").unwrap() + 5);
    assert_eq!(
        p.live.project(source, start..end, true, false, false).text,
        &source[start..end]
    );
}

#[test]
fn html_comments_share_hidden_ranges_with_reading_and_keep_code_literal() {
    let source = "---\ntitle: '<!-- metadata -->'\n---\nVisible <!--\nhidden **é**\n--> after.\n\n`<!-- code -->`\n";
    let p = Prepared::new(source);
    assert!(project(source, &p, "hidden").1.text.is_empty());
    assert_eq!(project(source, &p, "--> after").1.text, " after.");
    assert_eq!(project(source, &p, "`<!-- code").1.text, "<!-- code -->");
    assert!(p.rows.iter().all(|r| !r.text.contains("hidden")));
}

#[test]
fn named_inline_and_nested_footnotes_share_reading_numbers_and_source_ranges() {
    let source = "Text[^b] then[^a] and again[^b]. Inline ^[body **é** with[^a]].\n\n[^a]: Alpha\n[^b]: Beta\n";
    let p = Prepared::new(source);
    let (_, line) = project(source, &p, "Text");
    assert_eq!(line.text, p.rows[0].text.as_ref());
    assert_eq!(line.text, "Text[1] then[2] and again[1-1]. Inline [3].");
    let first = line
        .spans
        .iter()
        .find(|s| s.style.target.as_deref() == Some("opalglass-footnote:1"))
        .unwrap();
    assert_eq!(
        line.source_index(first.range.start),
        source.find("[^b]").unwrap()
    );
    assert_eq!(
        line.source_index(first.range.end),
        source.find("[^b]").unwrap() + 4
    );
    assert!(line.mapping.windows(2).all(|p| p[0] <= p[1]));
    for (at, _) in line.text.char_indices() {
        assert!(source.is_char_boundary(line.source_index(at)));
    }
}

#[test]
fn raw_links_inside_inline_notes_remain_addressable_while_the_note_is_collapsed() {
    let source = "Text ^[body [inner][ref]].\n\n[ref]: Target.md\n";
    let p = Prepared::new(source);
    assert_eq!(
        p.live.link_target_at(source.find("inner").unwrap()),
        Some("Target.md")
    );
    let (_, line) = project(source, &p, "Text");
    assert_eq!(line.text, "Text [1].");
    assert_eq!(
        line.spans[0].style.target.as_deref(),
        Some("opalglass-footnote:1")
    );
}

#[test]
fn multiline_inline_notes_and_long_line_slices_never_duplicate_hidden_contents() {
    let source = "Before ^[inline\nsecond **é** line] after.\n";
    let p = Prepared::new(source);
    assert_eq!(project(source, &p, "Before").1.text, "Before [1]");
    assert_eq!(project(source, &p, "second").1.text, " after.");
    let source = format!(
        "{} [named][ref] text[^note]\n\n[ref]: Target.md\n[^note]: Body\n",
        "é".repeat(200)
    );
    let p = Prepared::new(&source);
    let end = source.find('\n').unwrap();
    let line = p.live.project(&source, 0..end, false, false, false);
    let rebuilt: String = crate::document::source_rows(&source[..end])
        .iter()
        .map(|r| line.slice(r.clone()).text)
        .collect();
    assert_eq!(rebuilt, line.text);
    assert!(line.text.ends_with("named text[1]"));
}

#[test]
fn setext_headings_use_the_document_parse_without_hiding_active_markers() {
    let source = "Title **é**\n===========\n\nBody\n";
    let p = Prepared::new(source);
    let (_, line) = project(source, &p, "Title");
    assert_eq!(line.kind, crate::live::Kind::Heading(1));
    assert_eq!(line.text, "Title é");
    assert!(project(source, &p, "===").1.text.is_empty());
}

#[test]
fn multiline_setext_titles_style_every_line_and_hide_only_the_final_marker() {
    for source in [
        "First **é**\nsecond _line_\n---\n\nBody\n",
        "> First **é**\n> second _line_\n> ---\n\nBody\n",
        "First **é**\r\nsecond _line_\r\n===\r\n\r\nBody\r\n",
    ] {
        let p = Prepared::new(source);
        let level = if source.contains("===") { 1 } else { 2 };
        for (needle, expected) in [("First", "First é"), ("second", "second line")] {
            let start = source[..source.find(needle).unwrap()]
                .rfind('\n')
                .map_or(0, |n| n + 1);
            let end = source[start..].find('\n').unwrap() + start;
            let line = p.live.project(source, start..end, false, false, false);
            assert_eq!(line.kind, crate::live::Kind::Heading(level), "{source}");
            assert_eq!(line.text.trim_end_matches('\r'), expected, "{source}");
            for (at, _) in line.text.char_indices() {
                assert!(source.is_char_boundary(start + line.source_index(at)));
            }
        }
        let marker = if level == 1 { "===" } else { "---" };
        let at = source.find(marker).unwrap();
        let start = source[..at].rfind('\n').map_or(0, |n| n + 1);
        let end = source[start..].find('\n').unwrap() + start;
        assert!(
            p.live
                .project(source, start..end, false, false, false)
                .text
                .is_empty()
        );
        assert_eq!(
            p.live.project(source, start..end, true, false, false).text,
            source[start..end]
        );
        assert_eq!(project(source, &p, "Body").1.kind, crate::live::Kind::Text);
    }
}

#[test]
fn comments_touching_frontmatter_leave_properties_visible_in_editor_context() {
    for prefix in ["", "\u{feff}"] {
        for comment in ["<!--hidden-->", "%%hidden%%"] {
            let source = format!("{prefix}---\ntitle: Visible\n---\n{comment}\n\nBody\n");
            let p = Prepared::new(&source);
            assert_eq!(project(&source, &p, "title:").1.text, "title: Visible");
            assert!(!p.live.is_hidden(source.find("title:").unwrap()));
            assert_eq!(project(&source, &p, comment).1.text, "");
            assert_eq!(project(&source, &p, "Body").1.text, "Body");
        }
    }
}
