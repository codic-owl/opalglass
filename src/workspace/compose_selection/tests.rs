use super::{Rows, select};
use opalglass::{callouts::Projection, embeds::Target, prepared::Prepared};
use std::sync::Arc;

fn selected(source: &str, fragment: Option<&str>) -> (Arc<Prepared>, Projection, Rows) {
    let prepared = Arc::new(Prepared::new(source));
    let mut projection = Projection::default();
    projection.rebuild(prepared.rows.len(), &prepared.callouts);
    projection.with_footnotes(prepared.rows.len(), &prepared.footnotes, &prepared.callouts);
    let selection = Target {
        path: "Source.md".into(),
        fragment: fragment.map(Into::into),
    }
    .select(prepared.clone(), source.len())
    .unwrap();
    let rows = select(&selection, &projection, fragment.is_none());
    (prepared, projection, rows)
}

fn texts(prepared: &Prepared, projection: &Projection, rows: &Rows) -> Vec<String> {
    rows.logical
        .iter()
        .flat_map(|range| range.clone())
        .map(|row| prepared.rows[projection.row(row)].text.to_string())
        .collect()
}

#[test]
fn whole_notes_keep_the_complete_cached_projection_including_its_footer_order() {
    let source = "# Note\n\nFirst[^b], second[^a].\n\n[^a]: A\n[^b]: B\n[^unused]: Hidden\n";
    let (prepared, projection, rows) = selected(source, None);
    assert_eq!(rows.logical.len(), 1);
    assert_eq!(rows.logical[0], 0..projection.len(prepared.rows.len()));
    assert_eq!(rows.physical.len(), 1);
    assert_eq!(rows.physical[0], 0..prepared.rows.len());
    let displayed = texts(&prepared, &projection, &rows);
    assert_eq!(&displayed[displayed.len() - 2..], ["B", "A"]);
    assert!(!displayed.iter().any(|text| text.contains("Hidden")));
}

#[test]
fn selected_sections_append_only_used_definitions_and_their_nested_reference_closure() {
    let source = "# Intro\n\nOther[^other].\n\n# Selected\n\nHere[^used].\n\n# End\n\n[^other]: Unrelated\n[^used]: Main with nested[^nested].\n[^nested]: Nested **body**.\n[^unused]: Unused\n";
    let (prepared, projection, rows) = selected(source, Some("Selected"));
    assert_eq!(
        texts(&prepared, &projection, &rows),
        [
            "Selected",
            "Here[2].",
            "Main with nested[3].",
            "Nested body."
        ]
    );
    assert!(
        rows.logical
            .windows(2)
            .all(|pair| pair[0].end < pair[1].start)
    );
}

#[test]
fn a_selected_callout_keeps_its_fold_while_unrelated_footer_sections_stay_out() {
    let source = "# Selected\n\n> [!note]- Folded\n> Hidden **body**.\n\nVisible[^used].\n\n# End\n\n[^used]: Used footer\n[^unused]: Unused footer\n";
    let (prepared, projection, rows) = selected(source, Some("Selected"));
    assert_eq!(
        texts(&prepared, &projection, &rows),
        ["Selected", "Folded", "Visible[1].", "Used footer"]
    );
}

#[test]
fn inline_footnote_definitions_keep_nested_named_footers_outside_the_selected_section() {
    let source = "# Selected\n\nHere ^[outer[^nested]].\n\n# End\n\n[^nested]: Nested note\n";
    let (prepared, projection, rows) = selected(source, Some("Selected"));
    let displayed = texts(&prepared, &projection, &rows);
    assert_eq!(
        displayed,
        ["Selected", "Here [1].", "outer[2]", "Nested note"]
    );
}
