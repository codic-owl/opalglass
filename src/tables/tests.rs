use super::*;
use crate::{
    markdown::InlineStyle,
    prepared::Prepared,
    rendering::{Alignment, Cell},
};

#[test]
fn slices_reconstruct_every_cell_and_share_complete_content() {
    let long = "é 👩‍💻 e\u{301} link ".repeat(8000);
    let row = TableRow {
        header: true,
        cells: vec![
            Cell {
                text: long.clone().into(),
                alignment: Alignment::Right,
                spans: vec![Span {
                    range: 0..long.len(),
                    style: InlineStyle {
                        bold: true,
                        target: Some("Target.md".into()),
                        ..Default::default()
                    },
                }],
            },
            Cell {
                text: "short".into(),
                alignment: Alignment::Center,
                spans: vec![],
            },
            Cell {
                text: "".into(),
                alignment: Alignment::None,
                spans: vec![],
            },
        ],
    };
    let slices = chunks(row);
    assert!(slices.len() > 100);
    assert!(slices[0].first);
    assert!(!slices[0].last);
    assert!(slices.last().unwrap().last);
    for column in 0..3 {
        assert_eq!(
            slices.iter().map(|s| s.text(column)).collect::<String>(),
            slices[0].row.cells[column].text.as_ref()
        );
    }
    for (index, slice) in slices.iter().enumerate() {
        assert!(Arc::ptr_eq(&slices[0].row, &slice.row));
        assert_eq!(slice.row.cells[0].text.as_ref(), long);
        assert_eq!(slice.row.cells[0].alignment, Alignment::Right);
        assert!(slice.text(0).len() <= MAX_CELL_BYTES);
        assert!(!slice.text(0).starts_with('\u{301}'));
        assert!(!slice.text(0).starts_with('\u{200d}'));
        let spans = slice.spans(0);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].range, 0..slice.text(0).len());
        assert_eq!(spans[0].style.target.as_deref(), Some("Target.md"));
        assert!(spans[0].style.bold);
        if index > 0 {
            assert!(slice.text(1).is_empty());
            assert!(slice.spans(1).is_empty());
        }
    }
}

#[test]
fn pathological_graphemes_are_bounded_without_losing_bytes() {
    let text = format!("a{}end", "\u{301}".repeat(20_000));
    let slices = chunks(TableRow {
        header: false,
        cells: vec![Cell {
            text: text.clone().into(),
            spans: vec![],
            alignment: Alignment::None,
        }],
    });
    assert_eq!(slices.iter().map(|s| s.text(0)).collect::<String>(), text);
    assert!(slices.iter().all(|s| s.text(0).len() <= MAX_CELL_BYTES));
}

#[test]
fn prepared_table_slices_keep_styles_targets_source_and_container_sections() {
    let label = format!("{}end", "é 😀 linked ".repeat(1000));
    let source = format!(
        "%%hidden%%\n\n> [!note]- A table\n> | Left | Right |\n> | :--- | ---: |\n> | **[[Target\\|{label}]]** | #tag note[^n] |\n\n[^n]: Definition\n\n# End\n"
    );
    let prepared = Prepared::new(&source);
    let body: Vec<_> = prepared
        .rows
        .iter()
        .filter_map(|r| r.table.as_ref())
        .filter(|c| !c.row.header)
        .collect();
    assert!(body.len() > 5);
    assert_eq!(body.iter().map(|c| c.text(0)).collect::<String>(), label);
    assert_eq!(body[0].text(1), "#tag note[1]");
    assert_eq!(prepared.tags, vec!["tag"]);
    assert!(body.iter().all(|c| {
        c.spans(0)
            .iter()
            .all(|s| s.style.bold && s.style.target.as_deref() == Some("Target"))
    }));
    assert!(
        prepared
            .rows
            .windows(2)
            .all(|p| p[0].source.end <= p[1].source.start)
    );
    for (index, row) in prepared
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.table.is_some())
    {
        assert!(source.get(row.source.clone()).is_some());
        assert_eq!(prepared.row_at_source(row.source.start), index);
        assert!(prepared.callouts[0].rows.contains(&index));
    }
    let (title, end) = prepared.headings.last().unwrap();
    assert_eq!(title, "End");
    assert_eq!(
        prepared.rows[*end].source.start,
        source.find("# End").unwrap()
    );
    let note = &prepared.footnotes.definitions[0];
    assert!(
        note.sections
            .iter()
            .all(|section| prepared.rows[section.start].text.contains("Definition"))
    );
}

#[test]
fn clipped_spans_keep_only_visible_parts_including_at_slice_edges() {
    let text = "a".repeat(2400);
    let slices = chunks(TableRow {
        header: false,
        cells: vec![Cell {
            text: text.into(),
            alignment: Alignment::None,
            spans: vec![
                Span {
                    range: 0..1024,
                    style: InlineStyle {
                        bold: true,
                        ..Default::default()
                    },
                },
                Span {
                    range: 1024..1100,
                    style: InlineStyle {
                        code: true,
                        ..Default::default()
                    },
                },
                Span {
                    range: 2000..2400,
                    style: InlineStyle {
                        italic: true,
                        ..Default::default()
                    },
                },
            ],
        }],
    });
    assert_eq!(slices.len(), 3);
    assert_eq!(slices[0].spans(0).len(), 1);
    let second = slices[1].spans(0);
    assert_eq!(second.len(), 2);
    assert_eq!(second[0].range, 0..76);
    assert!(second[0].style.code);
    assert_eq!(second[1].range, 976..1024);
    assert_eq!(slices[2].spans(0)[0].range, 0..352);
}
