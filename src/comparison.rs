//! Bounded-time line comparisons, computed off the UI thread.
use similar::{ChangeTag, TextDiff};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Removed,
    Added,
    Divider,
}
#[derive(Clone, Debug)]
pub struct Row {
    pub kind: Kind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}
pub fn compare(old: &str, new: &str) -> Vec<Row> {
    if old == new {
        return vec![];
    }
    let diff = TextDiff::configure()
        .timeout(Duration::from_millis(200))
        .diff_lines(old, new);
    let mut rows = vec![];
    for group in diff.grouped_ops(3) {
        if !rows.is_empty() {
            rows.push(Row {
                kind: Kind::Divider,
                old_line: None,
                new_line: None,
                text: "⋯ unchanged lines ⋯".into(),
            });
        }
        for op in group {
            for change in diff.iter_changes(&op) {
                let kind = match change.tag() {
                    ChangeTag::Equal => Kind::Context,
                    ChangeTag::Delete => Kind::Removed,
                    ChangeTag::Insert => Kind::Added,
                };
                let value = change.value();
                // Chunk very long physical lines too: virtualizing rows must not
                // leave a single multi-megabyte line for the text shaper.
                for (index, range) in
                    crate::document::chunks(value.trim_end_matches(['\r', '\n']), 512)
                        .into_iter()
                        .enumerate()
                {
                    rows.push(Row {
                        kind,
                        old_line: (index == 0)
                            .then(|| change.old_index().map(|i| i + 1))
                            .flatten(),
                        new_line: (index == 0)
                            .then(|| change.new_index().map(|i| i + 1))
                            .flatten(),
                        text: value[range].into(),
                    });
                }
                if !value.ends_with('\n') {
                    rows.push(Row {
                        kind,
                        old_line: None,
                        new_line: None,
                        text: "⏎ No newline at end of file".into(),
                    });
                }
            }
        }
    }
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_keeps_distant_changes_line_numbers_unicode_and_newline_changes() {
        let common = "same\n".repeat(500);
        let rows = compare(&format!("old\n{common}tail\n"), &format!("new\n{common}尾"));
        assert!(rows.len() < 30);
        assert!(
            rows.iter()
                .any(|r| r.kind == Kind::Removed && r.text == "old" && r.old_line == Some(1))
        );
        assert!(
            rows.iter()
                .any(|r| r.kind == Kind::Added && r.text == "尾" && r.new_line == Some(502))
        );
        assert!(rows.iter().any(|r| r.kind == Kind::Divider));
        assert!(rows.iter().any(|r| r.text.contains("No newline")));
        assert!(compare("same", "same").is_empty());
        assert!(
            compare("x", &"界".repeat(30_000))
                .iter()
                .all(|r| r.text.len() <= 516)
        );
    }
}
