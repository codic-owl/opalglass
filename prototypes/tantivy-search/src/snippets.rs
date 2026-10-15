use crate::{
    Result,
    search::{Engine, Hit, Ticket},
};
use serde_json::{Value as Json, json};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    ops::Range,
    path::Path,
    time::UNIX_EPOCH,
};
use tantivy::{
    DocSet, Term,
    postings::Postings,
    schema::{IndexRecordOption, Value},
    tokenizer::TextAnalyzer,
};

pub const WINDOW_BYTES: usize = 16 * 1024;
const CACHE_BYTES: usize = 1024 * 1024;
const CACHE_ENTRIES: usize = 128;
const MAX_POSITIONS_PER_GROUP: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileVersion {
    pub len: u64,
    pub modified: String,
}
impl FileVersion {
    pub fn read(path: &Path) -> Result<Self> {
        Self::metadata(&fs::metadata(path)?)
    }
    fn metadata(meta: &fs::Metadata) -> Result<Self> {
        Ok(Self {
            len: meta.len(),
            modified: meta
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_nanos()
                .to_string(),
        })
    }
    fn indexed(engine: &Engine, hit: &Hit) -> Result<Self> {
        Ok(Self {
            len: hit
                .document
                .get_first(engine.fields.source_len)
                .and_then(|v| v.as_u64())
                .ok_or("Missing indexed file size")?,
            modified: crate::text(&hit.document, engine.fields.source_mtime),
        })
    }
}

// Each record is a token ordinal (u32 LE) and original UTF-8 byte offset (u64 LE).
// Byte-gap checkpoints keep skipped long tokens from forcing unbounded reads.
pub fn checkpoints(tokenizer: &mut TextAnalyzer, body: &str) -> Vec<u8> {
    let mut stream = tokenizer.token_stream(body);
    let mut output = vec![];
    let mut previous_position = 0;
    let mut previous_byte = 0;
    while stream.advance() {
        let token = stream.token();
        if output.is_empty()
            || token.position - previous_position >= 128
            || token.offset_from - previous_byte >= 4096
        {
            output.extend_from_slice(&(token.position as u32).to_le_bytes());
            output.extend_from_slice(&(token.offset_from as u64).to_le_bytes());
            previous_position = token.position;
            previous_byte = token.offset_from;
        }
    }
    output
}
fn checkpoint(bytes: &[u8], position: u32) -> Result<(u32, u64)> {
    if !bytes.len().is_multiple_of(12) {
        return Err("Invalid checkpoint map".into());
    }
    let count = bytes.len() / 12;
    let entry = |i: usize| {
        (
            u32::from_le_bytes(bytes[i * 12..i * 12 + 4].try_into().unwrap()),
            u64::from_le_bytes(bytes[i * 12 + 4..i * 12 + 12].try_into().unwrap()),
        )
    };
    let (mut low, mut high) = (0, count);
    while low < high {
        let mid = (low + high) / 2;
        if entry(mid).0 <= position {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Ok(if low == 0 { (0, 0) } else { entry(low - 1) })
}

#[derive(Clone, Debug)]
pub struct Snippet {
    pub status: &'static str,
    pub fragment: String,
    pub highlights: Vec<Range<usize>>,
    pub source_offset: u64,
    pub bytes_read: usize,
    pub cached: bool,
    pub positions_sampled: bool,
}
impl Snippet {
    fn empty(status: &'static str) -> Self {
        Self {
            status,
            fragment: String::new(),
            highlights: vec![],
            source_offset: 0,
            bytes_read: 0,
            cached: false,
            positions_sampled: false,
        }
    }
    pub fn json(&self, include_text: bool) -> Json {
        let mut value = json!({"status": self.status, "fragment_bytes": self.fragment.len(), "highlight_count": self.highlights.len(),
            "source_offset": self.source_offset, "source_bytes_read": self.bytes_read, "cached": self.cached, "positions_sampled": self.positions_sampled});
        if include_text {
            value["fragment"] = json!(self.fragment);
            value["highlight_byte_ranges"] = json!(
                self.highlights
                    .iter()
                    .map(|r| [r.start, r.end])
                    .collect::<Vec<_>>()
            );
        }
        value
    }
}
struct Entry {
    key: String,
    snippet: Snippet,
    cost: usize,
}
#[derive(Default)]
pub struct Snippets {
    cache: VecDeque<Entry>,
    cache_bytes: usize,
}
impl Snippets {
    pub fn clear(&mut self) {
        self.cache.clear();
        self.cache_bytes = 0;
    }
    pub fn get(&mut self, engine: &Engine, hit: &Hit, ticket: &Ticket) -> Result<Snippet> {
        ticket.check()?;
        let path = match engine.root.join(&hit.path).canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Snippet::empty("missing"));
            }
            Err(error) => return Err(error.into()),
        };
        if !path.starts_with(&engine.root) {
            return Err("Snippet path escaped vault".into());
        }
        let expected = FileVersion::indexed(engine, hit)?;
        if FileVersion::read(&path)? != expected {
            return Ok(Snippet::empty("stale"));
        }
        let key = format!(
            "{}|{:?}|{}|{}|{:?}",
            engine.root.display(),
            hit.address,
            expected.len,
            expected.modified,
            hit.plan
        );
        if let Some(i) = self.cache.iter().position(|e| e.key == key) {
            let entry = self.cache.remove(i).unwrap();
            let mut snippet = entry.snippet.clone();
            snippet.bytes_read = 0;
            snippet.cached = true;
            self.cache.push_back(entry);
            ticket.check()?;
            return Ok(snippet);
        }
        let groups = positions(engine, hit, ticket)?;
        let (anchor, sampled) =
            choose_anchor(&groups, &hit.plan.positions, hit.plan.quoted, ticket)?;
        let mut snippet = Snippet::empty(if anchor.is_some() {
            "matched"
        } else if sampled {
            "positions_limit"
        } else {
            "title_only"
        });
        snippet.positions_sampled = sampled;
        // If a phrase hits only the title, do not highlight isolated body words.
        let table = hit
            .document
            .get_first(engine.fields.checkpoints)
            .and_then(|v| v.as_bytes())
            .ok_or("Missing checkpoint map")?;
        let (base_position, offset) = if let Some(anchor) = anchor {
            let context = checkpoint(table, anchor.saturating_sub(20))?;
            let direct = checkpoint(table, anchor)?;
            if direct.1.saturating_sub(context.1) > (WINDOW_BYTES / 2) as u64 {
                direct
            } else {
                context
            }
        } else {
            (0, 0)
        };
        ticket.check()?;
        let mut file = File::open(&path)?;
        if FileVersion::metadata(&file.metadata()?)? != expected {
            return Ok(Snippet::empty("stale"));
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = Vec::with_capacity(WINDOW_BYTES);
        (&mut file)
            .take(WINDOW_BYTES as u64)
            .read_to_end(&mut buffer)?;
        let bytes_read = buffer.len();
        ticket.check()?;
        if FileVersion::metadata(&file.metadata()?)? != expected
            || FileVersion::read(&path)? != expected
        {
            return Ok(Snippet::empty("stale"));
        }
        let valid_length = match std::str::from_utf8(&buffer) {
            Ok(_) => buffer.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => return Ok(Snippet::empty("stale")),
        };
        let window = std::str::from_utf8(&buffer[..valid_length])?;
        let mut tokenizer = engine.index.tokenizer_for_field(engine.fields.body)?;
        let mut stream = tokenizer.token_stream(window);
        let mut tokens = vec![];
        while stream.advance() {
            ticket.check()?;
            tokens.push(stream.token().clone());
            if base_position + stream.token().position as u32
                > anchor.unwrap_or(0).saturating_add(128)
            {
                break;
            }
        }
        let anchor_index = anchor.and_then(|position| {
            tokens
                .iter()
                .position(|t| base_position + t.position as u32 == position)
        });
        if anchor.is_some() && anchor_index.is_none() {
            snippet.status = "window_miss";
            snippet.bytes_read = bytes_read;
            return Ok(snippet); // No hidden whole-file fallback.
        }
        let mut ranges = vec![];
        if anchor.is_some() {
            for (i, token) in tokens.iter().enumerate() {
                if hit.plan.quoted {
                    if token.text != hit.plan.groups[0][0] {
                        continue;
                    }
                    let start_position = token.position;
                    let mut end = token.offset_to;
                    let mut matched = true;
                    for (group, position) in hit.plan.groups.iter().zip(&hit.plan.positions).skip(1)
                    {
                        let wanted = start_position + (position - hit.plan.positions[0]) as usize;
                        if let Some(t) = tokens[i..]
                            .iter()
                            .take_while(|t| t.position <= wanted)
                            .find(|t| t.position == wanted && t.text == group[0])
                        {
                            end = t.offset_to;
                        } else {
                            matched = false;
                            break;
                        }
                    }
                    if matched {
                        ranges.push(token.offset_from..end);
                    }
                } else if hit.plan.groups.iter().any(|g| g.contains(&token.text)) {
                    ranges.push(token.offset_from..token.offset_to);
                }
            }
        }
        let anchor_byte = anchor_index.map_or(0, |i| tokens[i].offset_from);
        // Token verification protects against mismatched tokenizer/offset metadata.
        if anchor.is_some() && !ranges.iter().any(|r| r.start == anchor_byte) {
            snippet.status = "window_miss";
            snippet.bytes_read = bytes_read;
            return Ok(snippet);
        }
        let start = window[..anchor_byte]
            .char_indices()
            .rev()
            .nth(60)
            .map_or(0, |(i, _)| i);
        let mut end = window[start..]
            .char_indices()
            .nth(240)
            .map_or(window.len(), |(i, _)| start + i);
        if let Some(span) = ranges.iter().find(|r| r.start == anchor_byte) {
            end = end.max(span.end);
        }
        snippet.fragment = window[start..end].to_owned();
        snippet.source_offset = offset + start as u64;
        snippet.highlights = ranges
            .into_iter()
            .filter(|r| r.start >= start && r.end <= end)
            .map(|r| r.start - start..r.end - start)
            .collect();
        snippet.bytes_read = bytes_read;
        ticket.check()?;
        let cost = key.len() + snippet.fragment.len() + snippet.highlights.len() * 16 + 256;
        if cost <= CACHE_BYTES {
            while self.cache.len() >= CACHE_ENTRIES || self.cache_bytes + cost > CACHE_BYTES {
                if let Some(entry) = self.cache.pop_front() {
                    self.cache_bytes -= entry.cost;
                }
            }
            self.cache_bytes += cost;
            self.cache.push_back(Entry {
                key,
                snippet: snippet.clone(),
                cost,
            });
        }
        Ok(snippet)
    }
}

fn positions(engine: &Engine, hit: &Hit, ticket: &Ticket) -> Result<Vec<Vec<u32>>> {
    let inverted = engine
        .searcher
        .segment_reader(hit.address.segment_ord)
        .inverted_index(engine.fields.body)?;
    let mut groups = vec![];
    for alternatives in &hit.plan.groups {
        let mut positions = vec![];
        for term in alternatives {
            ticket.check()?;
            if let Some(mut postings) = inverted.read_postings(
                &Term::from_field_text(engine.fields.body, term),
                IndexRecordOption::WithFreqsAndPositions,
            )? && postings.seek(hit.address.doc_id) == hit.address.doc_id
            {
                let mut current = vec![];
                postings.positions(&mut current);
                // Tantivy decodes a term's positions together. Keep them for
                // binary-search membership; only passage candidates are capped.
                positions.extend(current);
            }
        }
        positions.sort_unstable();
        positions.dedup();
        groups.push(positions);
    }
    Ok(groups)
}
fn choose_anchor(
    groups: &[Vec<u32>],
    offsets: &[u32],
    quoted: bool,
    ticket: &Ticket,
) -> Result<(Option<u32>, bool)> {
    if quoted {
        let Some((rarest, positions)) = groups.iter().enumerate().min_by_key(|(_, g)| g.len())
        else {
            return Ok((None, false));
        };
        for &p in positions.iter().take(MAX_POSITIONS_PER_GROUP) {
            ticket.check()?;
            let Some(start) = p.checked_sub(offsets[rarest] - offsets[0]) else {
                continue;
            };
            if groups
                .iter()
                .zip(offsets)
                .all(|(group, offset)| group.binary_search(&(start + offset - offsets[0])).is_ok())
            {
                return Ok((Some(start), false));
            }
        }
        return Ok((None, positions.len() > MAX_POSITIONS_PER_GROUP));
    }
    // Sample anchors, but binary-search complete posting lists. A rare term late
    // in a huge note can therefore locate nearby common terms without a full sweep.
    let mut best: Option<(usize, u32, u32)> = None;
    let mut sampled = false;
    for positions in groups {
        let step = positions.len().div_ceil(256).max(1);
        sampled |= step > 1;
        for &anchor in positions.iter().step_by(step) {
            ticket.check()?;
            for before in [true, false] {
                let (mut covered, mut first, mut last) = (0, anchor, anchor);
                for group in groups {
                    let position = if before {
                        let i = group.partition_point(|p| *p <= anchor);
                        i.checked_sub(1)
                            .map(|i| group[i])
                            .filter(|p| anchor - p <= 32)
                    } else {
                        group
                            .get(group.partition_point(|p| *p < anchor))
                            .copied()
                            .filter(|p| p - anchor <= 32)
                    };
                    if let Some(position) = position {
                        covered += 1;
                        first = first.min(position);
                        last = last.max(position);
                    }
                }
                let span = last - first;
                if best.is_none_or(|(n, s, p)| {
                    covered > n || covered == n && (span < s || span == s && first < p)
                }) {
                    best = Some((covered, span, first));
                }
            }
        }
    }
    Ok((best.map(|(_, _, position)| position), sampled))
}
