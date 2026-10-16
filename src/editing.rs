//! Small Markdown editing transactions shared by every editor presentation.
use crate::buffer::Buffer;

/// Literal matches with byte offsets into the original UTF-8 source.
pub fn find(text: &str, query: &str, case_sensitive: bool) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if case_sensitive {
        return text
            .match_indices(query)
            .map(|(i, s)| i..i + s.len())
            .collect();
    }
    if text.is_ascii() && query.is_ascii() {
        return text
            .to_ascii_lowercase()
            .match_indices(&query.to_ascii_lowercase())
            .map(|(i, s)| i..i + s.len())
            .collect();
    }
    let mut folded = String::new();
    let mut positions = Vec::new();
    for (i, c) in text.char_indices() {
        for lower in c.to_lowercase() {
            folded.push(lower);
            positions.extend(std::iter::repeat_n((i, i + c.len_utf8()), lower.len_utf8()));
        }
    }
    let mut result = Vec::new();
    for (i, s) in folded.match_indices(&query.to_lowercase()) {
        let range = positions[i].0..positions[i + s.len() - 1].1;
        if result.last() != Some(&range) {
            result.push(range);
        }
    }
    result
}

#[derive(Clone, Debug)]
pub struct ListPrefix {
    pub indent: String,
    pub marker: String,
    pub end: usize,
    pub task: Option<bool>,
}

pub fn list_prefix(line: &str) -> Option<ListPrefix> {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - body.len()];
    let (marker, mut end) =
        if body.starts_with("- ") || body.starts_with("* ") || body.starts_with("+ ") {
            (body[..2].to_owned(), 2)
        } else {
            let digits = body.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0
                || digits > 9
                || !matches!(body.as_bytes().get(digits), Some(b'.' | b')'))
                || body.as_bytes().get(digits + 1) != Some(&b' ')
            {
                return None;
            }
            (body[..digits + 2].to_owned(), digits + 2)
        };
    let task = match body.get(end..end + 4) {
        Some("[ ] ") => Some(false),
        Some("[x] " | "[X] ") => Some(true),
        _ => None,
    };
    if task.is_some() {
        end += 4;
    }
    Some(ListPrefix {
        indent: indent.into(),
        marker,
        end: indent.len() + end,
        task,
    })
}

pub fn newline(buffer: &mut Buffer) {
    let start = buffer.line_start(buffer.cursor());
    let end = buffer.line_end(buffer.cursor());
    let line = &buffer.text()[start..end];
    let Some(prefix) = list_prefix(line) else {
        buffer.insert("\n");
        return;
    };
    if buffer.cursor() < start + prefix.end {
        buffer.insert("\n");
        return;
    }
    if line[prefix.end..].trim().is_empty() && buffer.selection().is_empty() {
        buffer.replace(start..end, "");
        return;
    }
    let marker = if prefix.marker.as_bytes()[0].is_ascii_digit() {
        let delimiter = prefix.marker.len() - 2;
        let number = prefix.marker[..delimiter].parse::<u64>().unwrap_or(0) + 1;
        format!("{number}{} ", &prefix.marker[delimiter..delimiter + 1])
    } else {
        prefix.marker
    };
    buffer.insert(&format!(
        "\n{}{marker}{}",
        prefix.indent,
        if prefix.task.is_some() { "[ ] " } else { "" }
    ));
}

pub fn indent(buffer: &mut Buffer, outdent: bool) {
    let selection = buffer.selection();
    let start = buffer.line_start(selection.start);
    let last =
        if selection.end > selection.start && buffer.line_start(selection.end) == selection.end {
            selection.end - 1
        } else {
            selection.end
        };
    let end = buffer.line_end(last);
    let lines = &buffer.text()[start..end];
    let mut output = String::new();
    let mut changes = Vec::new();
    let mut offset = start;
    for (i, line) in lines.split('\n').enumerate() {
        if i > 0 {
            output.push('\n');
        }
        let removed = if outdent {
            if line.starts_with('\t') {
                1
            } else {
                line.bytes().take(4).take_while(|&c| c == b' ').count()
            }
        } else {
            0
        };
        if !outdent {
            output.push_str("    ");
        }
        output.push_str(&line[removed..]);
        changes.push((offset, removed, if outdent { 0 } else { 4 }));
        offset += line.len() + 1;
    }
    let map = |pos: usize| {
        let mut delta = 0isize;
        for &(at, removed, added) in &changes {
            if pos < at {
                break;
            }
            delta += added as isize - removed.min(pos - at) as isize;
        }
        (pos as isize + delta).max(0) as usize
    };
    let (anchor, cursor) = (map(buffer.anchor()), map(buffer.cursor()));
    buffer.replace(start..end, &output);
    buffer.select(anchor, cursor);
}

pub fn wrap(buffer: &mut Buffer, marker: &str) {
    let range = buffer.selection();
    let before = range.start.checked_sub(marker.len());
    if let Some(start) = before
        && buffer.text().get(start..range.start) == Some(marker)
        && buffer.text().get(range.end..range.end + marker.len()) == Some(marker)
    {
        let text = buffer.selected().to_owned();
        buffer.replace(start..range.end + marker.len(), &text);
        buffer.select(start, start + text.len());
    } else {
        let text = format!("{marker}{}{marker}", buffer.selected());
        buffer.replace(range.clone(), &text);
        buffer.select(range.start + marker.len(), range.end + marker.len());
    }
}

pub fn toggle_task(buffer: &mut Buffer) {
    let range = buffer.selection();
    let start = buffer.line_start(range.start);
    let last = if range.end > range.start && buffer.line_start(range.end) == range.end {
        range.end - 1
    } else {
        range.end
    };
    let end = buffer.line_end(last);
    let text = buffer.text()[start..end]
        .split('\n')
        .map(|line| {
            if let Some(prefix) = list_prefix(line) {
                format!(
                    "{}{}[{}] {}",
                    prefix.indent,
                    prefix.marker,
                    if prefix.task == Some(false) { "x" } else { " " },
                    &line[prefix.end..]
                )
            } else {
                let body = line.trim_start_matches([' ', '\t']);
                format!("{}- [ ] {body}", &line[..line.len() - body.len()])
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    buffer.replace(start..end, &text);
    if !range.is_empty() {
        buffer.select(start, start + text.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_find_returns_original_unicode_byte_ranges() {
        let text = "CAFÉ café İ 👩‍💻";
        let found = find(text, "café", false);
        assert_eq!(
            found.iter().map(|r| &text[r.clone()]).collect::<Vec<_>>(),
            vec!["CAFÉ", "café"]
        );
        assert_eq!(find(text, "café", true).len(), 1);
        let dotted = find(text, "i", false);
        assert_eq!(&text[dotted[0].clone()], "İ");
        assert_eq!(find(text, "👩‍💻", false).len(), 1);
        assert!(find(text, "", false).is_empty());
    }

    #[test]
    fn smart_lists_continue_reset_tasks_exit_and_undo() {
        for (input, expected) in [
            ("- item", "- item\n- "),
            ("  - [x] done", "  - [x] done\n  - [ ] "),
            ("9. nine", "9. nine\n10. "),
            ("2) two", "2) two\n3) "),
            ("  - [ ] ", ""),
        ] {
            let mut b = Buffer::new(input);
            b.move_to(input.len(), false);
            newline(&mut b);
            assert_eq!(b.text(), expected);
            b.undo();
            assert_eq!(b.text(), input);
        }
    }
    #[test]
    fn formatting_indentation_and_tasks_are_source_transactions() {
        let mut b = Buffer::new("éclair\nsecond\nthird");
        b.select(0, "éclair\nsecond\n".len());
        indent(&mut b, false);
        assert_eq!(b.text(), "    éclair\n    second\nthird");
        indent(&mut b, true);
        assert_eq!(b.text(), "éclair\nsecond\nthird");
        b.select(0, "éclair".len());
        wrap(&mut b, "**");
        assert!(b.text().starts_with("**éclair**"));
        wrap(&mut b, "**");
        assert_eq!(b.selected(), "éclair");
        toggle_task(&mut b);
        assert!(b.text().starts_with("- [ ] éclair"));
        toggle_task(&mut b);
        assert!(b.text().starts_with("- [x] éclair"));
        b.undo();
        assert!(b.text().starts_with("- [ ] éclair"));
    }
}
