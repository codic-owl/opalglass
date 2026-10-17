use crate::{
    live::{Context, Kind, Line},
    prepared::Prepared,
};
use std::ops::Range;

fn physical_line(source: &str, needle: &str) -> Range<usize> {
    let at = source.find(needle).unwrap();
    let start = source[..at].rfind('\n').map_or(0, |at| at + 1);
    let end = source[at..].find('\n').map_or(source.len(), |end| at + end);
    start..end
}

fn project(source: &str, context: &Context, needle: &str, code: bool) -> Line {
    context.project(source, physical_line(source, needle), false, code, false)
}

#[test]
fn callout_metadata_tracks_nested_containers_and_physical_title_lines() {
    let source = "> [!WARNING]- **Careful** é\n> First **body**\n>\n> > [!tip]+\n> > Inner **body**\n>\n> After inner\n\nOutside\n";
    let prepared = Prepared::new(source);
    let context = &prepared.live;
    let callouts = context.callouts();
    assert_eq!(callouts.len(), 2);
    assert_eq!(callouts[0].callout.name, "warning");
    assert_eq!(callouts[0].callout.collapsed, Some(true));
    assert_eq!(callouts[0].depth, 1);
    assert_eq!(callouts[0].parent, None);
    assert_eq!(
        &source[callouts[0].title.clone()],
        "> [!WARNING]- **Careful** é"
    );
    assert_eq!(callouts[1].depth, 2);
    assert_eq!(callouts[1].parent, Some(0));
    assert_eq!(callouts[1].callout.collapsed, Some(false));
    assert_eq!(context.callout_at(callouts[1].title.start), Some(1));
    assert_eq!(
        context.callout_at(physical_line(source, "Inner **body**").start),
        Some(1)
    );
    assert_eq!(
        context.callout_at(physical_line(source, "After inner").start),
        Some(0)
    );
    assert_eq!(context.callout_at(source.find("Outside").unwrap()), None);
    let title = project(source, context, "Careful", false);
    assert_eq!(title.kind, Kind::CalloutTitle);
    assert_eq!(title.text, "Careful é");
    assert!(title.spans[0].style.bold);
    assert_eq!(project(source, context, "First", false).text, "First body");
    assert_eq!(
        project(source, context, "After inner", false).text,
        "After inner"
    );
    assert!(context.hidden_ranges().is_empty());
}

#[test]
fn default_titles_have_atomic_source_mapping_and_raw_reveal_keeps_markers() {
    let source = "> [!note]-\n> Body\n>\n> > [!tip]+\n> > Nested\n";
    let prepared = Prepared::new(source);
    for (needle, expected) in [("[!note]", "Note"), ("[!tip]", "Tip")] {
        let range = physical_line(source, needle);
        let title = prepared
            .live
            .project(source, range.clone(), false, false, false);
        assert_eq!(title.text, expected);
        assert_eq!(title.kind, Kind::CalloutTitle);
        let start = title.source_index(0);
        assert!((0..title.text.len()).all(|byte| title.source_index(byte) == start));
        assert_eq!(title.source_index(title.text.len()), range.len());
        let raw = prepared
            .live
            .project(source, range.clone(), true, false, false);
        assert_eq!(raw.text, &source[range.clone()]);
        assert!((0..=raw.text.len()).all(|byte| raw.source_index(byte) == byte));
    }
    let source = "> [!tip]+ %% hidden title %%\n> Body\n";
    let prepared = Prepared::new(source);
    assert_eq!(project(source, &prepared.live, "[!tip]", false).text, "Tip");
}

#[test]
fn container_masks_preserve_nested_headings_lists_and_ordinary_quotes() {
    let source = "> [!note]+ # Literal title\n> # Heading **é**\n> - [x] done\n> > ordinary **quote**\n>\n> > [!tip] Inner\n> > > ordinary inner\n";
    let prepared = Prepared::new(source);
    let context = &prepared.live;
    assert_eq!(
        project(source, context, "Literal title", false).text,
        "# Literal title"
    );
    let heading = project(source, context, "Heading", false);
    assert_eq!(heading.text, "Heading é");
    assert_eq!(heading.kind, Kind::Heading(1));
    assert_eq!(project(source, context, "done", false).text, "☑  done");
    let quoted = project(source, context, "ordinary **quote**", false);
    assert_eq!(quoted.text, "ordinary quote");
    assert_eq!(quoted.kind, Kind::Quote);
    let quoted = project(source, context, "ordinary inner", false);
    assert_eq!(quoted.text, "ordinary inner");
    assert_eq!(quoted.kind, Kind::Quote);
}

#[test]
fn code_in_callouts_loses_only_container_syntax() {
    let source = "> [!note] Code\n> ```rust\n> **literal** %% comment %% [[Target]]\n> ```\n>\n>     **indented**\n";
    let prepared = Prepared::new(source);
    let context = &prepared.live;
    let fence = project(source, context, "```rust", true);
    assert_eq!(fence.kind, Kind::Fence);
    assert_eq!(fence.text, "rust");
    let code = project(source, context, "**literal**", true);
    assert_eq!(code.kind, Kind::Code);
    assert_eq!(code.text, "**literal** %% comment %% [[Target]]");
    assert!(code.spans.is_empty());
    assert_eq!(
        project(source, context, "**indented**", true).text,
        "    **indented**"
    );
    let range = physical_line(source, "**literal**");
    assert_eq!(
        context
            .project(source, range.clone(), true, true, false)
            .text,
        &source[range]
    );
}

#[test]
fn unicode_titles_reference_links_and_slices_keep_source_positions() {
    let source = "> [!note]+ **hé 👩‍💻** and [é][ref]\n> Body **αβ**\n\n[ref]: Target.md\n";
    let prepared = Prepared::new(source);
    let range = physical_line(source, "hé");
    let title = prepared
        .live
        .project(source, range.clone(), false, false, false);
    assert_eq!(title.text, "hé 👩‍💻 and é");
    assert!(title.spans[0].style.bold);
    let link = title
        .spans
        .iter()
        .find(|s| s.style.target.as_deref() == Some("Target.md"))
        .unwrap();
    assert_eq!(&title.text[link.range.clone()], "é");
    assert_eq!(
        title.source_index(0),
        source.find("hé").unwrap() - range.start
    );
    assert!(title.mapping.windows(2).all(|pair| pair[0] <= pair[1]));
    for (at, _) in title.text.char_indices() {
        assert!(source.is_char_boundary(range.start + title.source_index(at)));
    }
    let split = source.find(" and").unwrap() - range.start;
    let first = title.slice(0..split);
    let last = title.slice(split..range.len());
    assert_eq!(format!("{}{}", first.text, last.text), title.text);
    assert!(first.spans.iter().all(|s| s.range.end <= first.text.len()));
    assert!(last.spans.iter().all(|s| s.range.end <= last.text.len()));
}

#[test]
fn metadata_stays_compact_for_large_callout_bodies() {
    let source = format!("> [!note]- Title\n{}\nOutside", "> body\n".repeat(10_000));
    let prepared = Prepared::new(&source);
    assert_eq!(prepared.live.callouts().len(), 1);
    assert!(prepared.live.hidden_ranges().is_empty());
    assert_eq!(project(&source, &prepared.live, "body", false).text, "body");
}

#[test]
fn callouts_in_nested_footnotes_keep_physical_ranges_and_hidden_notes_stay_hidden() {
    let source = "Main[^outer] and [^inner].\n\n[^outer]: Outer\n\n    > [^inner]: Nested\n    >\n    > > [!tip]+\n    > > Child **body**\n    >\n    > After inner\n\n    After outer\n\n[^unused]: Hidden\n\n    > [!note]\n    > Hidden body\n\nEnd\n";
    let prepared = Prepared::new(source);
    let context = &prepared.live;
    assert_eq!(context.callouts().len(), 2);
    let inner = physical_line(source, "[!tip]");
    assert_eq!(context.callouts()[0].title, inner);
    assert_eq!(context.callout_at(inner.start), Some(0));
    assert_eq!(project(source, context, "[!tip]", false).text, "Tip");
    assert_eq!(
        project(source, context, "Child **body**", false).text,
        "Child body"
    );
    assert!(project(source, context, "[!note]", false).text.is_empty());
    assert!(
        project(source, context, "Hidden body", false)
            .text
            .is_empty()
    );
    assert_eq!(
        context.footnote_at(source.find("After inner").unwrap()),
        Some(1)
    );
    assert_eq!(
        context.callout_at(source.find("After inner").unwrap()),
        None
    );
}

#[test]
fn footnote_relative_reading_depth_does_not_leave_callout_container_quotes_visible() {
    let source = "Main[^n].\n\n> [!note] Outer\n>\n> [^n]: Definition\n>\n>     > [!tip] Inner\n>     > Inner **body**\n>\n> Tail\n";
    let prepared = Prepared::new(source);
    let context = &prepared.live;
    assert_eq!(context.callouts().len(), 2);
    assert_eq!(context.callouts()[1].depth, 2);
    assert_eq!(context.callouts()[1].parent, Some(0));
    let body = project(source, context, "Inner **body**", false);
    assert_eq!(body.text, "Inner body");
    assert_eq!(body.kind, Kind::Text);
}

#[test]
fn default_callout_on_the_definition_line_retains_its_number_and_backlink() {
    let source = "Main[^n].\n\n[^n]: > [!tip]\n    > Body\n";
    let prepared = Prepared::new(source);
    let line = project(source, &prepared.live, "[!tip]", false);
    assert_eq!(line.text, "1. Tip");
    assert_eq!(line.kind, Kind::CalloutTitle);
    assert_eq!(
        line.spans[0].style.target,
        Some(format!(
            "{}{}",
            crate::footnotes::BACK_PREFIX,
            source.find("[^n]").unwrap()
        ))
    );
    assert!(line.mapping.windows(2).all(|pair| pair[0] <= pair[1]));
}
