//! Reviewable edits built from saved note snapshots, with conflict checks at apply time.
use crate::{comparison, editing, markdown, vault::Vault};
use sha2::{Digest, Sha256};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug)]
pub struct Edit {
    pub range: Range<usize>,
    pub replacement: String,
}
#[derive(Clone, Debug)]
pub struct NoteChanges {
    pub path: PathBuf,
    pub fingerprint: [u8; 32],
    pub edits: Vec<Edit>,
    pub preview: Vec<comparison::Row>,
}
fn fingerprint(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}
fn edited(source: &str, edits: &[Edit]) -> Result<String, String> {
    let mut output = source.to_owned();
    let mut previous = source.len();
    for edit in edits.iter().rev() {
        if edit.range.end > previous || source.get(edit.range.clone()).is_none() {
            return Err("Edits overlap or no longer match the note.".into());
        }
        output.replace_range(edit.range.clone(), &edit.replacement);
        previous = edit.range.start;
    }
    Ok(output)
}
impl NoteChanges {
    pub fn validate(&self, vault: &Vault) -> Result<(), String> {
        let source = vault.read(&self.path).map_err(|e| e.to_string())?;
        if fingerprint(&source) != self.fingerprint {
            return Err(format!(
                "{} changed since the preview. Refresh before applying.",
                self.path.display()
            ));
        }
        Ok(())
    }
    pub fn new(path: PathBuf, source: &str, edits: Vec<Edit>) -> Result<Self, String> {
        let next = edited(source, &edits)?;
        Ok(Self {
            path,
            fingerprint: fingerprint(source),
            edits,
            preview: comparison::compare(source, &next),
        })
    }
    pub fn apply(&self, vault: &mut Vault) -> Result<(), String> {
        let source = vault.read(&self.path).map_err(|e| e.to_string())?;
        if fingerprint(&source) != self.fingerprint {
            return Err("The note changed since this preview. Refresh before applying.".into());
        }
        let next = edited(&source, &self.edits)?;
        vault
            .save(&self.path, &source, &next)
            .map_err(|e| e.to_string())
    }
}

pub fn replace_preview(
    vault: &Vault,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    cancelled: &AtomicBool,
) -> Result<Vec<NoteChanges>, String> {
    if query.is_empty() {
        return Err("Enter text to find before previewing replacements.".into());
    }
    let mut results = vec![];
    for note in vault.notes.iter() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Search cancelled".into());
        }
        let source = vault
            .read(&note.path)
            .map_err(|e| format!("{}: {e}", note.path.display()))?;
        let replacement = crate::document::LineEnding::detect(&source)
            .encode(&crate::document::normalize(replacement));
        let edits: Vec<_> = editing::find(&source, query, case_sensitive)
            .into_iter()
            .filter(|range| source[range.clone()] != replacement)
            .map(|range| Edit {
                range,
                replacement: replacement.clone(),
            })
            .collect();
        if !edits.is_empty() {
            results.push(NoteChanges::new(note.path.clone(), &source, edits)?);
        }
    }
    Ok(results)
}

#[derive(Clone, Debug)]
pub struct Mention {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub label: String,
    pub context: String,
    pub highlight: Range<usize>,
    pub line: usize,
    pub fingerprint: [u8; 32],
}
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
pub fn mention_ranges(source: &str, title: &str) -> Vec<Range<usize>> {
    let mut ranges = vec![];
    for (block, range) in markdown::blocks_with_ranges(source) {
        if matches!(block, markdown::Block::Code(_) | markdown::Block::Rule) {
            continue;
        }
        let raw = &source[range.clone()];
        let (display, spans, mapping) = markdown::inline_mapped(raw);
        for found in editing::find(&display, title, false) {
            if display[..found.start].chars().next_back().is_some_and(word)
                || display[found.end..].chars().next().is_some_and(word)
                || spans.iter().any(|span| {
                    span.range.start < found.end
                        && found.start < span.range.end
                        && (span.style.target.is_some() || span.style.code)
                })
            {
                continue;
            }
            let from = mapping[found.start];
            // The next display byte may map after a closing Markdown delimiter.
            let end = from + display[found.clone()].len();
            if raw.get(from..end) == Some(&display[found]) {
                ranges.push(range.start + from..range.start + end);
            }
        }
    }
    ranges
}
pub fn unlinked_mentions(
    vault: &Vault,
    target: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<Mention>, String> {
    let title = target
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Missing note title")?;
    let mut results = vec![];
    for note in vault.notes.iter().filter(|n| n.path != target) {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Mention search cancelled".into());
        }
        let source = vault
            .read(&note.path)
            .map_err(|e| format!("{}: {e}", note.path.display()))?;
        let hash = fingerprint(&source);
        let newlines: Vec<_> = source.match_indices('\n').map(|(i, _)| i).collect();
        for range in mention_ranges(&source, title) {
            let start = source[..range.start]
                .char_indices()
                .rev()
                .nth(70)
                .map_or(0, |(i, _)| i);
            let end = source[range.end..]
                .char_indices()
                .nth(100)
                .map_or(source.len(), |(i, _)| range.end + i);
            results.push(Mention {
                path: note.path.clone(),
                label: source[range.clone()].into(),
                context: source[start..end].replace(['\r', '\n'], " "),
                highlight: range.start - start..range.end - start,
                line: newlines.partition_point(|i| *i < range.start) + 1,
                range,
                fingerprint: hash,
            });
        }
    }
    Ok(results)
}
pub fn link_mention(vault: &mut Vault, mention: &Mention, target: &Path) -> Result<(), String> {
    if !vault.contains_path(target) {
        return Err("The target note no longer exists.".into());
    }
    let source = vault.read(&mention.path).map_err(|e| e.to_string())?;
    if fingerprint(&source) != mention.fingerprint
        || source.get(mention.range.clone()) != Some(&mention.label)
    {
        return Err("The mentioning note changed. Refresh mentions before linking.".into());
    }
    let target_text = format!(
        "/{}",
        target
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/")
    );
    if target_text.contains(['[', ']', '|', '#', '\n', '\r'])
        || mention.label.contains(['[', ']', '|', '\n', '\r'])
    {
        return Err("This name cannot be represented safely as a wiki link.".into());
    }
    let edit = Edit {
        range: mention.range.clone(),
        replacement: format!("[[{target_text}|{}]]", mention.label),
    };
    let next = edited(&source, &[edit])?;
    vault
        .save(&mention.path, &source, &next)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mentions_exclude_links_code_yaml_and_partial_words_and_keep_unicode_offsets() {
        let source = "---\ntitle: Ilia\n---\nIlia and ILIAD. **ilia** [Ilia](other.md) [[Ilia]] `Ilia` #Ilia\n```\nIlia\n```\n";
        assert_eq!(
            mention_ranges(source, "Ilia")
                .into_iter()
                .map(|r| &source[r])
                .collect::<Vec<_>>(),
            ["Ilia", "ilia"]
        );
        let source = "élan ÉLAN élans";
        assert_eq!(
            mention_ranges(source, "élan")
                .into_iter()
                .map(|r| &source[r])
                .collect::<Vec<_>>(),
            ["élan", "ÉLAN"]
        );
    }
    #[test]
    fn preview_does_not_write_and_stale_replacements_or_mentions_never_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(root.path()).unwrap();
        vault.create("A", "Ilia and apple\r\nAPPLE\r\n").unwrap();
        vault.create("Ilia", "target").unwrap();
        let cancelled = AtomicBool::new(false);
        let plans = replace_preview(&vault, "apple", "pear", false, &cancelled).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].edits.len(), 2);
        assert_eq!(
            vault.read(Path::new("A.md")).unwrap(),
            "Ilia and apple\r\nAPPLE\r\n"
        );
        plans[0].apply(&mut vault).unwrap();
        assert_eq!(
            vault.read(Path::new("A.md")).unwrap(),
            "Ilia and pear\r\npear\r\n"
        );
        assert!(plans[0].apply(&mut vault).is_err());
        let mentions = unlinked_mentions(&vault, Path::new("Ilia.md"), &cancelled).unwrap();
        assert_eq!(mentions.len(), 1);
        link_mention(&mut vault, &mentions[0], Path::new("Ilia.md")).unwrap();
        assert!(
            vault
                .read(Path::new("A.md"))
                .unwrap()
                .starts_with("[[/Ilia|Ilia]]")
        );
        assert!(link_mention(&mut vault, &mentions[0], Path::new("Ilia.md")).is_err());
        assert!(
            unlinked_mentions(&vault, Path::new("Ilia.md"), &cancelled)
                .unwrap()
                .is_empty()
        );
        assert!(replace_preview(&vault, "", "x", false, &cancelled).is_err());
        assert!(replace_preview(&vault, "a", "x", false, &AtomicBool::new(true)).is_err());
    }
}
