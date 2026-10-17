// These cases intentionally pass and compare collections of hidden intervals.
#![allow(clippy::single_range_in_vec_init)]

use super::{hidden_rows, subtract};
use opalglass::live;

#[test]
fn hidden_constructs_remove_their_lines_but_keep_real_blank_lines_and_visible_text() {
    let source = "before\n\n  %% first\n\nlast %%  \n\na %%one%% b\n%%a%%  %%b%%\nafter";
    let rows = opalglass::document::source_rows(source);
    let start = source.find("%% first").unwrap();
    let end = source.find("last %%").unwrap() + "last %%".len();
    let one = source.find("%%one%%").unwrap();
    let a = source.find("%%a%%").unwrap();
    let b = source.find("%%b%%").unwrap();
    let hidden = hidden_rows(
        source,
        &rows,
        &[start..end, one..one + 7, a..a + 5, b..b + 5],
        |_| false,
        |_| false,
    );
    let visible = live::rows::Rows::new(rows.len(), hidden);
    let kept = (0..visible.len())
        .map(|i| &source[rows[visible.row(i)].clone()])
        .collect::<Vec<_>>();
    assert_eq!(kept, ["before", "", "", "a %%one%% b", "after"]);
}

#[test]
fn thousands_of_comment_lines_produce_one_gap_and_reveal_only_the_active_interval() {
    let source = format!("top\n<!--{}-->\nend", "line\n".repeat(4000));
    let rows = opalglass::document::source_rows(&source);
    let end = source.find("-->").unwrap() + 3;
    let base = hidden_rows(&source, &rows, &[4..end], |_| false, |_| false);
    assert_eq!(base, [1..rows.len() - 1]);
    let mut selected = Vec::new();
    subtract(base[0].clone(), Some(&(10..13)), &mut selected);
    assert_eq!(selected, [1..10, 13..rows.len() - 1]);
    let visible = live::rows::Rows::new(rows.len(), selected);
    assert_eq!(visible.len(), 5);
    assert_eq!(
        (0..visible.len())
            .map(|i| visible.row(i))
            .collect::<Vec<_>>(),
        [0, 10, 11, 12, rows.len() - 1]
    );
}

#[test]
fn a_synthetic_footnote_prefix_keeps_an_otherwise_empty_definition_row() {
    let source = "[^used]: \n\n[^unused]: \nend";
    let rows = opalglass::document::source_rows(source);
    let unused = source.find("[^unused]").unwrap();
    let hidden = hidden_rows(
        source,
        &rows,
        &[0..9, unused..unused + 11],
        |range| range.start == 0,
        |_| false,
    );
    assert_eq!(hidden, [2..3]);
}

#[test]
fn hidden_intervals_outside_the_opening_index_do_not_hide_its_last_row() {
    let source = "first\nsecond\nthird\n<!-- later -->";
    let all = opalglass::document::source_rows(source);
    let start = source.find("<!--").unwrap();
    assert!(
        hidden_rows(
            source,
            &all[..2],
            &[start..source.len()],
            |_| false,
            |_| false
        )
        .is_empty()
    );
}

#[test]
fn comments_inside_callouts_compact_after_container_markers_are_removed() {
    let source = "Start\n\n> [!note]+ %%hidden title%%\n> %% hidden only %%\n>\n> Visible\n>\n> <!-- multi\n> end -->\n>\n> > [!tip]+ %%another hidden title%%\n> > content\n\nEnd\n";
    let rows = opalglass::document::source_rows(source);
    let context = opalglass::prepared::Prepared::new(source).live;
    let boundaries = std::cell::Cell::new(0);
    let hidden = hidden_rows(
        source,
        &rows,
        context.hidden_ranges(),
        |range| context.footnote_prefix_in(range.clone()),
        |range| {
            boundaries.set(boundaries.get() + 1);
            context
                .project(source, range.clone(), false, false, false)
                .text
                .trim()
                .is_empty()
        },
    );
    let visible = live::rows::Rows::new(rows.len(), hidden);
    let kept = (0..visible.len())
        .map(|i| &source[rows[visible.row(i)].clone()])
        .collect::<Vec<_>>();
    assert!(!kept.contains(&"> %% hidden only %%"));
    assert!(!kept.contains(&"> <!-- multi"));
    assert!(!kept.contains(&"> end -->"));
    assert!(kept.contains(&"> [!note]+ %%hidden title%%"));
    assert!(kept.contains(&"> > [!tip]+ %%another hidden title%%"));
    assert!(kept.contains(&">"));
    // The multiline comment already covers its closing row's quote prefix,
    // so that boundary takes the raw fast path. Only the other four boundary
    // rows need container projection; interior rows never do.
    assert_eq!(boundaries.get(), 4);
}
