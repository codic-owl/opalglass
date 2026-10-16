//! Background-only syntax coloring; no parser work belongs in a paint callback.
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    ops::Range,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    ThemeSet::load_defaults()
        .themes
        .remove("base16-ocean.dark")
        .unwrap()
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
}

/// Bound background work and token storage for pasted/minified code. Text beyond
/// these limits is still rendered and copied, without syntax decoration.
pub const DOCUMENT_BYTES: usize = 1024 * 1024;
pub const LINE_BYTES: usize = 8192;
const MAX_SPANS: usize = 65_536;
const CACHE_BYTES: usize = 4 * 1024 * 1024;
#[derive(Default)]
struct Cache {
    entries: VecDeque<([u8; 32], Arc<Vec<Span>>)>,
    bytes: usize,
}
impl Cache {
    fn get(&mut self, key: &[u8; 32]) -> Option<Arc<Vec<Span>>> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(index)?;
        let spans = entry.1.clone();
        self.entries.push_back(entry);
        Some(spans)
    }
    fn insert(&mut self, key: [u8; 32], spans: &[Span]) {
        let bytes = std::mem::size_of_val(spans);
        if bytes > CACHE_BYTES || self.entries.iter().any(|(k, _)| *k == key) {
            return;
        }
        while self.bytes + bytes > CACHE_BYTES || self.entries.len() >= 64 {
            let Some((_, old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= std::mem::size_of_val(old.as_slice());
        }
        self.bytes += bytes;
        self.entries.push_back((key, Arc::new(spans.to_vec())));
    }
}
// Store only a content hash and decorations, never a second copy of note text.
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));

pub fn highlight(
    code: &str,
    language: &str,
    cancelled: &AtomicBool,
    remaining: &mut usize,
) -> Option<Vec<Span>> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let language = language.trim().to_ascii_lowercase();
    let token = match language.as_str() {
        "js" | "node" | "nodejs" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "sh" | "bash" | "zsh" | "shell" | "shellscript" => "bash",
        "yml" => "yaml",
        "csharp" => "cs",
        "c++" => "cpp",
        "text" | "txt" | "plaintext" | "plain" | "" => return Some(vec![]),
        token => token,
    };
    if *remaining == 0 {
        return Some(vec![]);
    }
    let key = (code.len() <= *remaining).then(|| {
        let mut hash = Sha256::new();
        hash.update(token.as_bytes());
        hash.update([0]);
        hash.update(code.as_bytes());
        <[u8; 32]>::from(hash.finalize())
    });
    if let Some(key) = &key
        && let Some(spans) = CACHE.lock().unwrap().get(key)
    {
        *remaining -= code.len();
        return Some(spans.as_ref().clone());
    }
    let Some(syntax) = SYNTAXES.find_syntax_by_token(token) else {
        return Some(vec![]);
    };
    let mut highlighter = HighlightLines::new(syntax, &THEME);
    let mut spans: Vec<Span> = Vec::new();
    let mut offset = 0;
    for line in code.split_inclusive('\n') {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if line.len() > LINE_BYTES || line.len() > *remaining || spans.len() >= MAX_SPANS {
            // Do not resume after skipped text: multiline lexical state would be wrong.
            break;
        }
        *remaining -= line.len();
        let Ok(regions) = highlighter.highlight_line(line, &SYNTAXES) else {
            break;
        };
        for (style, text) in regions {
            if text.is_empty() {
                continue;
            }
            let color = style.foreground;
            let style = Style {
                color: (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            };
            let end = offset + text.len();
            if let Some(last) = spans.last_mut()
                && last.style == style
                && last.range.end == offset
            {
                last.range.end = end;
            } else {
                if spans.len() == MAX_SPANS {
                    return Some(spans);
                }
                spans.push(Span {
                    range: offset..end,
                    style,
                });
            }
            offset = end;
        }
    }
    if let Some(key) = key
        && offset == code.len()
    {
        CACHE.lock().unwrap().insert(key, &spans);
    }
    Some(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoration_cache_is_bounded_and_keys_content_and_language() {
        let mut cache = Cache::default();
        let spans = vec![Span {
            range: 0..1,
            style: Style {
                color: 1,
                bold: false,
                italic: false,
            },
        }];
        for n in 0..65u8 {
            cache.insert([n; 32], &spans);
        }
        assert_eq!(cache.entries.len(), 64);
        assert!(cache.get(&[0; 32]).is_none());
        assert!(cache.get(&[64; 32]).is_some());
        assert!(cache.bytes <= CACHE_BYTES);
        let oversized = vec![spans[0].clone(); CACHE_BYTES / std::mem::size_of::<Span>() + 1];
        cache.insert([255; 32], &oversized);
        assert!(cache.get(&[255; 32]).is_none());
        let cancelled = AtomicBool::new(false);
        let first = highlight("const x = 42;", "js", &cancelled, &mut { DOCUMENT_BYTES }).unwrap();
        let cached = highlight("const x = 42;", "js", &cancelled, &mut { DOCUMENT_BYTES }).unwrap();
        let changed =
            highlight("const x = '';", "js", &cancelled, &mut { DOCUMENT_BYTES }).unwrap();
        assert_eq!(first, cached);
        assert_ne!(cached, changed);
        assert!(
            highlight("const x = 42;", "plain", &cancelled, &mut {
                DOCUMENT_BYTES
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn grammars_preserve_multiline_state_aliases_and_unicode() {
        let source =
            "const answer = 42; // é\n/* comment\nstill comment */\nconsole.log(\"你好\");\n";
        let spans = highlight(source, "js", &AtomicBool::new(false), &mut {
            DOCUMENT_BYTES
        })
        .unwrap();
        assert_eq!(spans.first().unwrap().range.start, 0);
        assert_eq!(spans.last().unwrap().range.end, source.len());
        assert!(spans.windows(2).all(|s| s[0].range.end == s[1].range.start));
        assert!(spans.iter().all(|s| source.get(s.range.clone()).is_some()));
        let at = |text: &str| {
            spans
                .iter()
                .find(|s| s.range.contains(&source.find(text).unwrap()))
                .unwrap()
                .style
        };
        assert_ne!(at("const"), at("42"));
        assert_ne!(at("42"), at("你好"));
        assert_eq!(at("comment\n"), at("still comment"));
        assert_eq!(
            spans,
            highlight(source, "javascript", &AtomicBool::new(false), &mut {
                DOCUMENT_BYTES
            })
            .unwrap()
        );
    }
    #[test]
    fn unknown_languages_limits_and_cancellation_preserve_plain_fallback() {
        assert!(
            highlight("x", "opalglass-unknown", &AtomicBool::new(false), &mut {
                DOCUMENT_BYTES
            })
            .unwrap()
            .is_empty()
        );
        assert!(highlight("x", "rust", &AtomicBool::new(true), &mut { DOCUMENT_BYTES }).is_none());
        let source = format!("let x = 1;\n{}\nlet y = 2;", "a".repeat(LINE_BYTES + 1));
        let spans = highlight(&source, "rust", &AtomicBool::new(false), &mut {
            DOCUMENT_BYTES
        })
        .unwrap();
        assert_eq!(spans.last().unwrap().range.end, "let x = 1;\n".len());
        let mut budget = 1;
        assert!(
            highlight("let x = 1;", "rust", &AtomicBool::new(false), &mut budget)
                .unwrap()
                .is_empty()
        );
        let dense = "a=1;\n".repeat(MAX_SPANS);
        let spans = highlight(&dense, "js", &AtomicBool::new(false), &mut {
            DOCUMENT_BYTES
        })
        .unwrap();
        assert!(spans.len() <= MAX_SPANS);
        assert!(spans.last().unwrap().range.end < dense.len());
    }
}
