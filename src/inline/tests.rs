use super::*;

#[test]
fn bare_links_keep_boundaries_formatting_and_source_positions() {
    let source = "é https://example.com/path?x=1&amp;y=2. (https://en.wikipedia.org/wiki/Foo_(bar)). **HTTP://example.com/bold** www.example.com test@example.com";
    let (text, spans, mapping) = parse(source, true);
    let links: Vec<_> = spans
        .iter()
        .filter_map(|span| {
            span.style
                .target
                .as_deref()
                .map(|target| (&text[span.range.clone()], target))
        })
        .collect();
    assert_eq!(
        links,
        vec![
            (
                "https://example.com/path?x=1&y=2",
                "https://example.com/path?x=1&y=2"
            ),
            (
                "https://en.wikipedia.org/wiki/Foo_(bar)",
                "https://en.wikipedia.org/wiki/Foo_(bar)"
            ),
            ("HTTP://example.com/bold", "HTTP://example.com/bold"),
            ("www.example.com", "http://www.example.com"),
            ("test@example.com", "mailto:test@example.com"),
        ]
    );
    assert!(
        spans
            .iter()
            .any(|span| span.style.bold && span.style.target.is_some())
    );
    assert_eq!(mapping.len(), text.len() + 1);
    assert!(mapping.windows(2).all(|pair| pair[0] <= pair[1]));
    for (at, _) in text.char_indices() {
        assert!(source.is_char_boundary(mapping[at]));
    }
    let at = text.find("example.com/bold").unwrap();
    assert_eq!(mapping[at], source.find("example.com/bold").unwrap());
}

#[test]
fn bare_links_exclude_code_html_and_explicit_link_labels() {
    let source = "`https://example.com/code` [https://example.com/label](Target.md) [[Target|https://example.com/wiki]] <span data-url=\"https://example.com/attribute\">HTML</span> example.com ftp://example.com root@localhost https://例え.テスト/道";
    let (_, spans, _) = parse(source, true);
    let targets: Vec<_> = spans
        .iter()
        .filter_map(|span| span.style.target.as_deref())
        .collect();
    assert_eq!(targets, vec!["Target.md", "Target"]);
    let prepared = crate::prepared::Prepared::new(
        "https://example.com/reading\n\n```\nhttps://example.com/fenced\n```\n",
    );
    let targets: Vec<_> = prepared
        .rows
        .iter()
        .flat_map(|row| row.spans.iter())
        .filter_map(|span| span.style.target.as_deref())
        .collect();
    assert_eq!(targets, vec!["https://example.com/reading"]);
}

#[test]
fn bare_links_handle_unicode_paths_fragments_and_escaped_schemes() {
    let source = r"https://example.com/é#part https\://example.com/escaped mailto:test@example.com";
    let (text, spans, _) = parse(source, true);
    let links: Vec<_> = spans
        .iter()
        .filter_map(|span| {
            span.style
                .target
                .as_deref()
                .map(|target| (&text[span.range.clone()], target))
        })
        .collect();
    assert_eq!(
        links,
        vec![
            ("https://example.com/é#part", "https://example.com/é#part"),
            ("test@example.com", "mailto:test@example.com"),
        ]
    );
}

#[test]
fn highlighted_urls_exclude_hidden_markers_and_keep_code_literal() {
    let source = "==🔴https://example.com/é== ==`https://example.com/code`==";
    let (text, spans, mapping) = parse(source, true);
    assert_eq!(text, "https://example.com/é https://example.com/code");
    let links: Vec<_> = spans
        .iter()
        .filter_map(|span| span.style.target.as_deref())
        .collect();
    assert_eq!(links, vec!["https://example.com/é"]);
    assert_eq!(mapping[0], source.find("https").unwrap());
    assert_eq!(spans[0].style.highlight, Some(HighlightColor::Red));
}
#[test]
fn highlights_preserve_nested_styles_colors_code_and_edit_positions() {
    let source = "==outer **bold** and `code`== [==link==](Target.md) ==🔴é👩‍💻==";
    let (text, spans, mapping) = parse(source, true);
    assert_eq!(text, "outer bold and code link é👩‍💻");
    assert_eq!(mapping.len(), text.len() + 1);
    assert!(mapping.windows(2).all(|p| p[0] <= p[1]));
    for (at, _) in text.char_indices() {
        assert!(source.is_char_boundary(mapping[at]));
    }
    assert!(spans.iter().any(|s| s.style.bold
        && s.style.highlight == Some(HighlightColor::Default)
        && &text[s.range.clone()] == "bold"));
    assert!(
        spans.iter().any(|s| s.style.code
            && s.style.highlight.is_some()
            && &text[s.range.clone()] == "code")
    );
    assert!(
        spans
            .iter()
            .any(|s| s.style.target.as_deref() == Some("Target.md") && s.style.highlight.is_some())
    );
    let red = spans
        .iter()
        .find(|s| s.style.highlight == Some(HighlightColor::Red))
        .unwrap();
    assert_eq!(&text[red.range.clone()], "é👩‍💻");
    assert_eq!(mapping[red.range.start], source.find('é').unwrap());
    let mut edited = source.to_owned();
    let start = text.find("bold").unwrap();
    edited.replace_range(mapping[start]..mapping[start] + 4, "strong");
    assert!(edited.contains("**strong**"));
    for literal in [
        r"\==escaped==",
        "==unclosed",
        "== spaced ==",
        "`==literal==`",
    ] {
        assert!(
            parse(literal, true)
                .1
                .iter()
                .all(|s| s.style.highlight.is_none())
        );
    }
    let (text, spans, _) = parse(
        "==marked== ==🔴red== ==🟠orange== ==🟡yellow== ==🟢green== ==🔵blue== ==🟣purple==",
        false,
    );
    assert_eq!(text, "marked red orange yellow green blue purple");
    assert_eq!(spans.len(), 7);
    assert_eq!(parse("==across\nlines==", false).0, "across\nlines");
    assert_eq!(parse("[[Target|==alias==]]", false).0, "==alias==");
    assert_eq!(parse("==🔴==", false).0, "");
    assert!(parse("==first\n\nsecond==", false).1.is_empty());
    let (text, spans, _) = parse("==#work==", false);
    assert_eq!(text, "#work");
    assert_eq!(spans[0].style.target.as_deref(), Some("opalglass-tag:work"));
}
#[test]
fn commonmark_delimiters_entities_links_and_code_preserve_editable_offsets() {
    let source = "***both*** snake_case_name &amp; &#233; [a **label**](<Some (note).md> \"title\") ``a ` b``";
    let (text, spans, mapping) = parse(source, true);
    assert_eq!(text, "both snake_case_name & é a label a ` b");
    assert_eq!(mapping.len(), text.len() + 1);
    assert!(mapping.windows(2).all(|p| p[0] <= p[1]));
    assert!(mapping.iter().all(|p| *p <= source.len()));
    assert!(
        spans
            .iter()
            .any(|s| s.style.bold && s.style.italic && &text[s.range.clone()] == "both")
    );
    let label = spans
        .iter()
        .find(|s| s.style.bold && s.style.target.is_some())
        .unwrap();
    assert_eq!(label.style.target.as_deref(), Some("Some (note).md"));
    assert_eq!(&source[mapping[label.range.start]..][..5], "label");
    let code = spans.iter().find(|s| s.style.code).unwrap();
    assert_eq!(&text[code.range.clone()], "a ` b");
}
#[test]
fn comments_keep_unicode_edit_positions_and_empty_image_labels_have_targets() {
    let source = "%%hidden 👩‍💻%%é **word** `%%literal%%`";
    let (text, _, mapping) = parse(source, true);
    assert_eq!(text, "é word %%literal%%");
    assert_eq!(mapping[0], source.find('é').unwrap());
    assert_eq!(
        mapping[text.find("word").unwrap()],
        source.find("word").unwrap()
    );
    assert!(mapping.windows(2).all(|p| p[0] <= p[1]));
    let (text, spans, _) = parse("before ![](Image.png) after", false);
    assert_eq!(text, "before Image.png after");
    assert!(
        spans
            .iter()
            .any(|s| s.style.image && s.style.target.as_deref() == Some("Image.png"))
    );
    let links = crate::links::references("%%hidden%%[[Target|alias]]");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].destination, 12..18);
    assert!(
        crate::markdown::wiki_links("%%[[hidden]]%% [[Shown]]")
            .iter()
            .all(|l| l.target == "Shown")
    );
}
