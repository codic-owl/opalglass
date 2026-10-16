//! Export the native renderer's prepared rows for the Obsidian fixture inventory.
use opalglass::prepared::Prepared;
use serde_json::{Value, json};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let coverage: Value =
        serde_json::from_str(include_str!("../tests/fixtures/obsidian-coverage.json"))?;
    let mut cases = vec![];
    for case in coverage["cases"].as_array().unwrap() {
        let source = case["markdown"].as_str().unwrap();
        let document = Prepared::new(source);
        let rows: Vec<_> = document.rows.iter().map(|row| json!({
            "kind": format!("{:?}", row.kind), "source": [row.source.start, row.source.end],
            "text": row.text.as_ref(), "indent": row.indent, "quotes": row.quotes,
            "language": row.language.as_deref(), "task": row.task.as_ref().map(|r| [r.start, r.end]),
            "spans": row.spans.iter().map(|s| json!({"range": [s.range.start, s.range.end], "style": format!("{:?}", s.style)})).collect::<Vec<_>>(),
            "table": row.table.as_ref().map(|t| json!({"header": t.header, "cells": t.cells.iter().map(|c| json!({"text": c.text.as_ref(), "alignment": format!("{:?}", c.alignment), "spans": format!("{:?}", c.spans)})).collect::<Vec<_>>()})),
        })).collect();
        let mut projection = opalglass::callouts::Projection::default();
        projection.rebuild(document.rows.len(), &document.callouts);
        projection.with_footnotes(document.rows.len(), &document.footnotes, &document.callouts);
        let displayed_rows = (0..projection.len(document.rows.len()))
            .map(|i| projection.row(i))
            .collect::<Vec<_>>();
        let footnotes = document
            .footnotes
            .definitions
            .iter()
            .map(|d| {
                json!({
                    "label": d.label.as_ref(), "number": d.number,
                    "sections": d.sections.iter().map(|r| [r.start, r.end]).collect::<Vec<_>>(),
                    "references": d.references,
                })
            })
            .collect::<Vec<_>>();
        cases.push(json!({"id": case["id"], "rows": rows, "displayed_rows": displayed_rows, "footnotes": footnotes}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"stage": "Native preparation, not a visual parity assertion", "cases": cases})
        )?
    );
    Ok(())
}
