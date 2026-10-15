Standalone Tantivy search prototype (format 2)

The app now uses the production implementation in crates/search, derived from
this prototype. This package remains an independent benchmark reference.

This package does not change Opalglass's search or dependencies. It reads a vault
and creates a disposable index outside it. Dependencies are pinned by its own
Cargo.lock. Rust 1.90 is used with portable CPU targeting, release optimization
and ThinLTO. No target-cpu=native flags.

From the repository root:

cargo build --release --locked --manifest-path prototypes/tantivy-search/Cargo.toml --target-dir target/tantivy-prototype
mkdir -p target/search-audit/tantivy
target/tantivy-prototype/release/opalglass-tantivy-prototype build /path/to/vault target/search-audit/tantivy/index-v2
target/tantivy-prototype/release/opalglass-tantivy-prototype bench-v2 target/search-audit/tantivy/index-v2

The build command refuses an existing destination. Old format-1 indexes require
rebuilding into a NEW directory. bench-v2 opens the index in a separate process.
Both commands print JSON; redirect output to a file to keep the measurements.
The original bench command is retained for the old literal-phrase search baseline,
but must also use a format-2 index because the schema has new metadata fields.

Try actual results and snippets (JSON lines, titles emitted before excerpts):

target/tantivy-prototype/release/opalglass-tantivy-prototype search target/search-audit/tantivy/index-v2 'glossry'
target/tantivy-prototype/release/opalglass-tantivy-prototype search target/search-audit/tantivy/index-v2 'divnie light'
target/tantivy-prototype/release/opalglass-tantivy-prototype search target/search-audit/tantivy/index-v2 '"divine light"'
target/tantivy-prototype/release/opalglass-tantivy-prototype search target/search-audit/tantivy/index-v2 'gloss*'

Validation:
cargo test --release --locked --offline --manifest-path prototypes/tantivy-search/Cargo.toml --target-dir target/tantivy-prototype
cargo clippy --locked --offline --manifest-path prototypes/tantivy-search/Cargo.toml --target-dir target/tantivy-prototype --all-targets -- -D warnings

Storage and indexing:
- Note bodies: indexed tokens, frequencies and positions, without STORED. Every
  stored document is checked after indexing to ensure no body field is present.
- Stored metadata: titles, paths, source length/mtime and a compact checkpoint map.
  Each checkpoint holds a token position (u32) and original UTF-8 byte offset (u64).
  Checkpoints occur every 128 token positions or when the byte gap exceeds 4 KiB.
  These are offsets, not copied text. Checkpoints use the same tokenizer as the index.
- Source length and modification time are checked around indexing reads.
- One worker and one merge thread; 32 MiB writer arena; commits after approximately
  16 MiB of input to drain the document queue. These are not a total RAM cap.
- Build timings include discovery, reads, checkpoint generation, indexing, commits
  and background merges. Body-storage verification follows and has separate memory.

Retrieval and typo tolerance (search.rs):
- Exact lowercase titles first, then exact token matches, then fuzzy title matches,
  then fuzzy body matches. Exact results cannot be displaced by fuzzy results.
- Ordinary words use AND across title/body, with title BM25 weight 10. An entire
  quoted query is an exact token phrase, including tokenizer position gaps, and
  never triggers fuzzy fallback. Mixed quoted clauses/operators are not supported.
- Fuzzy fallback runs only when fewer than five results exist. Words below four
  characters remain exact. Others allow one edit, including adjacent transposition.
  Two edits are attempted only for words of nine or more characters and only if
  the first pass returned no results. A two-edit correction must produce a title
  hit before it can retrieve related body hits. Results are capped at twenty.
- Known words in multiword queries remain exact. Title corrections are reused for
  body retrieval, avoiding repeated broad dictionary searches. Only unresolved
  spellings need a body-dictionary expansion (one edit maximum).
- Uses the same Levenshtein DFA + term-dictionary FST approach as FuzzyTermQuery.
  Explicit expansion provides BM25 scoring, bounded alternatives, cancellation
  checkpoints, and the real corrected terms needed for snippet highlighting.
- Up to 256 matching dictionary terms inspected per field/segment/word; at most
  sixteen alternatives retained, preferring smaller edit distance then frequency.
  Exact spelling is always retained. Truncation is reported in results; these caps
  intentionally trade exhaustive fuzzy recall for predictable query work.
- An explicit trailing * completes the last word by prefix, separately from typo
  correction. Prefix length must be >=2; the same expansion caps apply. Automatic
  per-keystroke completion is left to the future UI.
- Queries are limited to 512 UTF-8 bytes and eight indexed words in this prototype.
  Default Tantivy tokenization/lowercasing is used, including its long-token filter.
  No stemming, accent folding, aliases, fuzzy path subsequences, or language tuning.

Bounded snippets (snippets.rs):
- Get actual matched-word positions from postings, choose a nearby cluster with
  good query-term coverage, and use checkpoints to seek into the ORIGINAL file.
- Read at most 16 KiB per visible result. Tokenize only through the selected match
  plus 128 positions. There is no hidden whole-file fallback. A pathological window
  that cannot verify its match is reported as window_miss, not given a false highlight.
- Phrase selection starts with the rarest query term and checks the full position
  lists for phrase order. Ordinary queries sample candidate anchors (up to 256 per
  group), but use full lists to find nearby terms. This finds common words beside
  a rare term late in a file without scanning the entire file or all position events.
- Tantivy still decodes the full position list for a term in one document. Very
  frequent words can therefore cost more; snippets are not constant-time in all
  circumstances. Quoted queries inspect up to 4096 rare-term candidates; exhaustion
  is reported as positions_limit. Candidate sampling is reported in snippet JSON.
- About 240 characters of original Markdown per excerpt, extending to include a
  longer whole phrase. UTF-8 byte highlight ranges and absolute source byte offset
  are returned. Corrected words are highlighted, not the misspelled query spelling.
- Title-only matches show a beginning preview without spurious body highlighting.
- A bounded LRU cache keeps <=128 snippets with an approximate 1 MiB payload budget.
  Cached snippets require no file-content read, but still validate file metadata.
- Source size/mtime are checked before cache use and around reads. Changed/deleted
  files return stale/missing status; a rebuild is required. Size+mtime is not a
  cryptographic version ID and cannot detect deliberate preservation of both.
- Each request has a generation token. New requests invalidate earlier work;
  checks between stages, dictionary terms, hits, reads and tokenization suppress
  obsolete output. A running Tantivy search call or OS read is not interruptible.
- The CLI renders only the first five snippets and emits results first. Actual
  viewport scheduling/background execution belongs to app integration.

Benchmark method:
- Search: first call plus median/p95 of twenty warm calls, excluding snippets,
  stdout serialization and UI. No result cache is used for retrieval.
- Snippets: first call, twenty warm-filesystem calls with the snippet cache cleared,
  then twenty cached calls. These include metadata validation and JSON construction.
- Separate side-by-side comparison runs the old whole-file generator and new bounded
  generator on IDENTICAL top-five literal-phrase hits. One timing sample per path.
  Passage selection differs: the old generator scores individual words throughout
  a document; the new generator uses positional clusters and verifies whole phrases.
- Bounded-path memory is recorded BEFORE running the old whole-file baseline.
  Linux /proc reports RSS/HWM; elsewhere these metrics are null. This is a standalone
  process, not the full GUI. More diverse queries can touch more mapped index pages.
- Filesystem caches are not cleared; first call does not mean cold disk.
- The older Opalglass scan benchmark returns every arbitrary-substring match. New
  unquoted search is token AND, and quoted search is a token phrase. Counts/ranking
  are not feature-equivalent, and backend latency is not end-to-end UI latency.
- Benchmark JSON includes paths and highlight counts, not copied excerpt contents.

Integration remains separate:
No app UI changes, watcher, incremental reconciliation, dirty-buffer overlay or
automatic rebuild. Those need to be implemented before replacing the app's search.
The current note-file format, source root and tokenizer must stay aligned with
checkpoints. Body-free storage saves a separate retrievable body copy; derived
terms/positions still disclose content and are not a confidentiality mechanism.

References:
https://docs.rs/tantivy/0.26.2/tantivy/
https://docs.rs/tantivy/0.26.2/tantivy/query/struct.FuzzyTermQuery.html
