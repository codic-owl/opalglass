//! Read-only native Markdown validation across an existing vault; no text is logged.
use opalglass::{links, prepared::Prepared, vault::Vault};
use std::{sync::atomic::AtomicBool, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args_os().nth(1).expect("vault path required");
    let start = Instant::now();
    let vault = Vault::discover(root, &AtomicBool::new(false))?;
    let (mut bytes, mut rows, mut references) = (0, 0, 0);
    for note in vault.notes.iter() {
        let source = vault.read(&note.path)?;
        bytes += source.len();
        for reference in links::references(&source) {
            assert!(
                source.get(reference.destination).is_some(),
                "Invalid reference offset in {}",
                note.path.display()
            );
            references += 1;
        }
        let document = Prepared::new(&source);
        for span in document.source_syntax.iter() {
            assert!(
                source.get(span.range.clone()).is_some(),
                "Invalid syntax source span in {}",
                note.path.display()
            );
        }
        assert!(
            document
                .source_syntax
                .windows(2)
                .all(|s| s[0].range.end <= s[1].range.start),
            "Unsorted syntax spans in {}",
            note.path.display()
        );
        for pair in document.rows.windows(2) {
            assert!(
                pair[0].source.end <= pair[1].source.start,
                "Unsorted source rows in {}: {:?} {:?} followed by {:?} {:?}",
                note.path.display(),
                pair[0].kind,
                pair[0].source,
                pair[1].kind,
                pair[1].source
            );
        }
        let mut projection = opalglass::callouts::Projection::default();
        projection.rebuild(document.rows.len(), &document.callouts);
        projection.with_footnotes(document.rows.len(), &document.footnotes, &document.callouts);
        for index in 0..projection.len(document.rows.len()) {
            let row = projection.row(index);
            assert!(
                row < document.rows.len(),
                "Invalid projected row in {}",
                note.path.display()
            );
            assert_eq!(
                projection.list_index(row),
                index,
                "Projection round trip in {}",
                note.path.display()
            );
        }
        for reference in document
            .footnotes
            .definitions
            .iter()
            .flat_map(|d| &d.references)
        {
            assert!(
                source.is_char_boundary(*reference),
                "Invalid footnote return offset in {}",
                note.path.display()
            );
        }
        rows += document.rows.len();
        for row in &document.rows {
            assert!(
                source.get(row.source.clone()).is_some(),
                "Invalid source bounds in {}",
                note.path.display()
            );
            for span in &row.spans {
                assert!(
                    row.text.get(span.range.clone()).is_some(),
                    "Invalid display span in {}",
                    note.path.display()
                );
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"notes": vault.notes.len(), "bytes": bytes, "rows": rows, "references": references, "seconds": start.elapsed().as_secs_f64()})
    );
    Ok(())
}
