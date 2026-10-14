#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub fn detect(text: &str) -> Self {
        match text.find(['\r', '\n']) {
            Some(index) if text[index..].starts_with("\r\n") => Self::CrLf,
            Some(index) if text.as_bytes()[index] == b'\r' => Self::Cr,
            _ => Self::Lf,
        }
    }
    pub fn encode(self, normalized: &str) -> String {
        match self {
            Self::Lf => normalized.into(),
            Self::CrLf => normalized.replace('\n', "\r\n"),
            Self::Cr => normalized.replace('\n', "\r"),
        }
    }
}

pub fn normalize(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Bounded display chunks, without splitting a grapheme or losing source bytes.
pub fn chunks(text: &str, limit: usize) -> Vec<std::ops::Range<usize>> {
    use unicode_segmentation::UnicodeSegmentation;
    if text.len() <= limit {
        return std::iter::once(0..text.len()).collect();
    }
    let mut result = Vec::new();
    let mut start = 0;
    for (index, _) in text.grapheme_indices(true) {
        if index - start >= limit {
            result.push(start..index);
            start = index;
        }
    }
    result.push(start..text.len());
    result
}

pub fn source_rows(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut result = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        if line.len() <= 256 {
            result.push(start..start + line.len());
        } else {
            result.extend(
                chunks(line, 256)
                    .into_iter()
                    .map(|r| start + r.start..start + r.end),
            );
        }
        start += line.len() + 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_chunks_preserve_graphemes_empty_lines_and_offsets() {
        let text = format!("{}\n\nend\n", "👩‍💻e\u{301}".repeat(1000));
        let rows = source_rows(&text);
        assert!(rows.len() > 10);
        for row in &rows {
            assert!(text.get(row.clone()).is_some());
            assert!(row.len() < 280);
        }
        assert_eq!(rows.last(), Some(&(text.len()..text.len())));
        assert!(rows.iter().any(|r| r.is_empty() && r.start < text.len()));
        assert_eq!(
            chunks(&text, 1024)
                .into_iter()
                .map(|r| &text[r])
                .collect::<String>(),
            text
        );
    }
    #[test]
    fn platform_line_endings_round_trip_and_extend_consistently() {
        for original in ["one\ntwo\n", "one\r\ntwo\r\n", "one\rtwo\r"] {
            let ending = LineEnding::detect(original);
            assert_eq!(ending.encode(&normalize(original)), original);
            let edited = ending.encode(&(normalize(original) + "new\n"));
            assert!(edited.starts_with(original));
            assert_eq!(normalize(&edited), "one\ntwo\nnew\n");
        }
    }
}
