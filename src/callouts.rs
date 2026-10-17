//! Native callouts and a compact projection for folding virtualized rows.
use std::ops::Range;

mod projection;
pub use projection::Projection;

#[derive(Clone, Debug)]
pub struct Callout {
    pub source: Range<usize>,
    pub rows: Range<usize>,
    pub name: String,
    /// None is not foldable; true starts collapsed, false starts expanded.
    pub collapsed: Option<bool>,
}
impl Callout {
    pub(crate) fn parse(source: &str, range: Range<usize>) -> Option<(Self, Range<usize>, String)> {
        let line = source
            .get(range.clone())?
            .split('\n')
            .next()?
            .trim_end_matches('\r');
        let content = line.trim_start().strip_prefix('>')?.trim_start();
        let rest = content.strip_prefix("[!")?;
        let end = rest.find(']')?;
        let name = &rest[..end];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let mut title = &rest[end + 1..];
        let collapsed = if let Some(rest) = title.strip_prefix('-') {
            title = rest;
            Some(true)
        } else if let Some(rest) = title.strip_prefix('+') {
            title = rest;
            Some(false)
        } else {
            None
        };
        title = title.trim_start();
        let title_start = range.start + line.len() - title.len();
        let title_end = range.start + line.len();
        let mut default_title = name.to_lowercase();
        default_title[..1].make_ascii_uppercase();
        Some((
            Self {
                source: range,
                rows: 0..0,
                name: name.to_lowercase(),
                collapsed,
            },
            title_start..title_end,
            default_title,
        ))
    }
    pub fn color(&self) -> u32 {
        match self.name.as_str() {
            "abstract" | "summary" | "tldr" | "tip" | "hint" | "important" => 0x55b8bb,
            "success" | "check" | "done" => 0x65bd8b,
            "question" | "help" | "faq" | "warning" | "caution" | "attention" => 0xd9aa59,
            "failure" | "fail" | "missing" | "danger" | "error" | "bug" => 0xe27a86,
            "example" => 0xb5a0ed,
            "quote" | "cite" => 0xa5adbc,
            _ => 0x79a9e8,
        }
    }
    pub fn symbol(&self) -> &'static str {
        match self.name.as_str() {
            "success" | "check" | "done" => "✓",
            "question" | "help" | "faq" => "?",
            "warning" | "caution" | "attention" | "danger" | "error" => "!",
            "failure" | "fail" | "missing" | "bug" => "×",
            "quote" | "cite" => "❝",
            "tip" | "hint" | "important" => "◇",
            "example" => "☷",
            _ => "ⓘ",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{Kind, Prepared};

    #[test]
    fn callouts_keep_nested_blocks_titles_and_source_ranges() {
        let source = "%%hidden%%\n> [!WARNING]- **Careful** é\n> First paragraph\n> continued.\n>\n> - [ ] Task\n>\n> > [!tip]+\n> > Inner **body**\n>\n> After inner\n\n# Outside\n\n> plain quote\n";
        let prepared = Prepared::new(source);
        assert_eq!(prepared.callouts.len(), 2);
        assert_eq!(prepared.callouts[0].name, "warning");
        assert_eq!(prepared.callouts[0].collapsed, Some(true));
        assert_eq!(prepared.callouts[1].name, "tip");
        assert_eq!(prepared.callouts[1].collapsed, Some(false));
        assert_eq!(prepared.rows[0].text.as_ref(), "Careful é");
        assert!(prepared.rows[0].spans[0].style.bold);
        assert_eq!(
            prepared.rows[1].text.as_ref(),
            "First paragraph\ncontinued."
        );
        assert!(matches!(prepared.rows[2].kind, Kind::Task(false)));
        assert_eq!(&source[prepared.rows[2].task.clone().unwrap()], "[ ]");
        assert_eq!(prepared.rows[3].text.as_ref(), "Tip");
        assert_eq!(prepared.rows[4].text.as_ref(), "Inner body");
        assert_eq!(prepared.rows[5].text.as_ref(), "After inner");
        assert_eq!(prepared.callouts[0].rows, 0..6);
        assert_eq!(prepared.callouts[1].rows, 3..5);
        for pair in prepared.rows.windows(2) {
            assert!(
                pair[0].source.end <= pair[1].source.start,
                "{:?} {:?}",
                pair[0].source,
                pair[1].source
            );
        }
        assert!(
            prepared
                .rows
                .iter()
                .all(|r| source.get(r.source.clone()).is_some())
        );
    }

    #[test]
    fn folded_projection_skips_large_bodies_and_respects_nested_overrides() {
        let source = format!(
            "> [!note]- Outer\n>\n> > [!warning]- Inner\n> > hidden\n>\n{}\n# End",
            "> body\n>\n".repeat(2000)
        );
        let p = Prepared::new(&source);
        let mut projection = Projection::default();
        projection.rebuild(p.rows.len(), &p.callouts);
        assert_eq!(projection.len(p.rows.len()), 2);
        assert_eq!(projection.row(1), p.rows.len() - 1);
        assert_eq!(projection.list_index(1000), 0);
        projection
            .overrides
            .insert(p.callouts[0].source.start, false);
        projection.rebuild(p.rows.len(), &p.callouts);
        assert_eq!(projection.len(p.rows.len()), p.rows.len() - 1);
        assert_eq!(projection.list_index(2), 1);
        projection
            .overrides
            .insert(p.callouts[1].source.start, false);
        projection.rebuild(p.rows.len(), &p.callouts);
        assert_eq!(projection.len(p.rows.len()), p.rows.len());
        assert_eq!(projection.row(1000), 1000);
    }
}
