//! Bounded dictionary expansion keeps the actual matched words available for snippets.
use crate::{Fields, Result, text};
use levenshtein_automata::{DFA, Distance, LevenshteinAutomatonBuilder};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use tantivy::{
    DocAddress, Index, Searcher, TantivyDocument, Term,
    collector::{Count, TopDocs},
    query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery},
    schema::{Field, IndexRecordOption},
};
use tantivy_fst::Automaton;

const LIMIT: usize = 20;
const SPARSE: usize = 5;
const MAX_ALTERNATIVES: usize = 16;
const MAX_ENUMERATED: usize = 256;

#[derive(Clone, Default)]
pub struct Requests(Arc<AtomicU64>);
#[derive(Clone)]
pub struct Ticket {
    generation: u64,
    current: Arc<AtomicU64>,
}
impl Requests {
    pub fn begin(&self) -> Ticket {
        Ticket {
            generation: self.0.fetch_add(1, Ordering::Relaxed) + 1,
            current: self.0.clone(),
        }
    }
}
impl Ticket {
    pub fn check(&self) -> Result<()> {
        if self.current.load(Ordering::Relaxed) != self.generation {
            return Err("Search superseded".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub groups: Vec<Vec<String>>,
    pub positions: Vec<u32>,
    pub quoted: bool,
}
#[derive(Clone)]
pub struct Hit {
    pub address: DocAddress,
    pub document: TantivyDocument,
    pub path: String,
    pub title: String,
    pub tier: &'static str,
    pub plan: Plan,
}
pub struct Results {
    pub hits: Vec<Hit>,
    pub exact_count: usize,
    pub expansions_truncated: bool,
    pub stages: Vec<&'static str>,
}
impl Results {
    pub fn json(&self) -> Json {
        json!({"exact_match_count": self.exact_count, "expansions_truncated": self.expansions_truncated,
            "stages": self.stages, "hits": self.hits.iter().map(|h| json!({"path": h.path,
            "title": h.title, "tier": h.tier, "matched_vocabulary": h.plan.groups})).collect::<Vec<_>>()})
    }
}
pub struct Engine {
    pub index: Index,
    pub searcher: Searcher,
    pub fields: Fields,
    pub root: PathBuf,
}
impl Engine {
    pub fn open(directory: &Path) -> Result<Self> {
        let source: Json = serde_json::from_slice(&std::fs::read(directory.join("source.json"))?)?;
        if source["format"] != 2 {
            return Err("Rebuild the prototype index with format 2".into());
        }
        let root =
            PathBuf::from(source["root"].as_str().ok_or("Missing vault root")?).canonicalize()?;
        let index = Index::open_in_dir(directory)?;
        let fields = Fields::from(&index.schema())?;
        let searcher = index.reader()?.searcher();
        Ok(Self {
            index,
            fields,
            searcher,
            root,
        })
    }

    pub fn search(&self, input: &str, ticket: &Ticket) -> Result<Results> {
        ticket.check()?;
        let input = input.trim();
        if input.len() > 512 {
            return Err("Prototype query limit is 512 bytes".into());
        }
        let quoted = input.starts_with('"') && input.ends_with('"') && input.len() >= 2;
        let literal = if quoted {
            &input[1..input.len() - 1]
        } else {
            input
        };
        if literal.contains('"') {
            return Err(
                "Quote the entire phrase; mixed quoted clauses are not supported yet".into(),
            );
        }
        let prefix = !quoted && literal.ends_with('*');
        let literal = if prefix {
            &literal[..literal.len() - 1]
        } else {
            literal
        };
        let mut tokenizer = self.index.tokenizer_for_field(self.fields.body)?;
        let mut stream = tokenizer.token_stream(literal);
        let mut groups = vec![];
        let mut positions = vec![];
        while stream.advance() {
            groups.push(vec![stream.token().text.clone()]);
            positions.push(stream.token().position as u32);
            if groups.len() > 8 {
                return Err("Prototype query limit is 8 indexed words".into());
            }
        }
        let plan = Plan {
            groups,
            positions,
            quoted,
        };
        let mut output = Results {
            hits: vec![],
            exact_count: 0,
            expansions_truncated: false,
            stages: vec![],
        };
        if plan.groups.is_empty() {
            return Ok(output);
        }
        if prefix {
            if plan.groups.last().unwrap()[0].chars().count() < 2 {
                return Err("Prefixes need at least two characters".into());
            }
            let mut expanded = plan.clone();
            let last = expanded.groups.len() - 1;
            expanded.groups[last] = self.prefix_terms(
                &expanded.groups[last][0],
                ticket,
                &mut output.expansions_truncated,
            )?;
            self.append(
                &mut output,
                &expanded,
                &[self.fields.title, self.fields.body],
                "prefix",
                ticket,
            )?;
            return Ok(output);
        }
        let exact_title = TermQuery::new(
            Term::from_field_text(self.fields.exact, &literal.to_lowercase()),
            IndexRecordOption::Basic,
        );
        let titles = self
            .searcher
            .search(&exact_title, &TopDocs::with_limit(LIMIT).order_by_score())?;
        self.add_hits(&mut output, titles, &plan, "exact_title", ticket)?;
        output.exact_count = self.append(
            &mut output,
            &plan,
            &[self.fields.title, self.fields.body],
            "exact",
            ticket,
        )?;
        if quoted || output.hits.len() >= SPARSE {
            return Ok(output);
        }
        // Correct titles first. Reuse their vocabulary for body retrieval instead
        // of searching the much larger body dictionary for the same spelling.
        for distance in [1, 2] {
            if output.hits.len() >= SPARSE || distance == 2 && !output.hits.is_empty() {
                break;
            }
            if distance == 2 && !plan.groups.iter().any(|g| g[0].chars().count() >= 9) {
                break;
            }
            let expanded_title = self.fuzzy_plan(
                &plan,
                self.fields.title,
                distance,
                ticket,
                &mut output.expansions_truncated,
            )?;
            let before_titles = output.hits.len();
            if expanded_title.groups != plan.groups {
                self.append(
                    &mut output,
                    &expanded_title,
                    &[self.fields.title],
                    "fuzzy_title",
                    ticket,
                )?;
            }
            if output.hits.len() >= SPARSE {
                break;
            }
            // Two-edit corrections need evidence from a matching title. Never
            // invent broad two-edit body matches for an otherwise unknown query.
            if distance == 2 && output.hits.len() == before_titles {
                continue;
            }
            let expanded_body = if distance == 1 {
                self.fuzzy_plan(
                    &expanded_title,
                    self.fields.body,
                    1,
                    ticket,
                    &mut output.expansions_truncated,
                )?
            } else {
                expanded_title
            };
            if expanded_body.groups != plan.groups {
                self.append(
                    &mut output,
                    &expanded_body,
                    &[self.fields.title, self.fields.body],
                    "fuzzy_body",
                    ticket,
                )?;
            }
        }
        ticket.check()?;
        Ok(output)
    }

    fn append(
        &self,
        output: &mut Results,
        plan: &Plan,
        fields: &[Field],
        tier: &'static str,
        ticket: &Ticket,
    ) -> Result<usize> {
        ticket.check()?;
        let query = self.query(plan, fields)?;
        let (hits, count) = self.searcher.search(
            &query,
            &(TopDocs::with_limit(LIMIT).order_by_score(), Count),
        )?;
        output.stages.push(tier);
        self.add_hits(output, hits, plan, tier, ticket)?;
        Ok(count)
    }
    fn add_hits(
        &self,
        output: &mut Results,
        hits: Vec<(f32, DocAddress)>,
        plan: &Plan,
        tier: &'static str,
        ticket: &Ticket,
    ) -> Result<()> {
        for (_, address) in hits {
            ticket.check()?;
            if output.hits.len() == LIMIT {
                break;
            }
            if output.hits.iter().any(|h| h.address == address) {
                continue;
            }
            let document = self.searcher.doc::<TantivyDocument>(address)?;
            output.hits.push(Hit {
                address,
                path: text(&document, self.fields.path),
                title: text(&document, self.fields.title),
                document,
                tier,
                plan: plan.clone(),
            });
        }
        Ok(())
    }
    fn query(&self, plan: &Plan, fields: &[Field]) -> Result<Box<dyn Query>> {
        if plan.quoted {
            // Preserve tokenizer position gaps by using PhraseQuery directly.
            let fields = fields
                .iter()
                .map(|&field| {
                    let terms: Vec<_> = plan
                        .groups
                        .iter()
                        .zip(&plan.positions)
                        .map(|(g, p)| (*p as usize, Term::from_field_text(field, &g[0])))
                        .collect();
                    let q: Box<dyn Query> = if terms.len() == 1 {
                        Box::new(TermQuery::new(
                            terms[0].1.clone(),
                            IndexRecordOption::WithFreqs,
                        ))
                    } else {
                        Box::new(tantivy::query::PhraseQuery::new_with_offset(terms))
                    };
                    let boost = if field == self.fields.title {
                        10.0
                    } else {
                        1.0
                    };
                    (
                        Occur::Should,
                        Box::new(BoostQuery::new(q, boost)) as Box<dyn Query>,
                    )
                })
                .collect();
            return Ok(Box::new(BooleanQuery::new(fields)));
        }
        let clauses = plan
            .groups
            .iter()
            .map(|group| {
                let choices = fields
                    .iter()
                    .flat_map(|&field| {
                        group.iter().enumerate().map(move |(i, word)| {
                            let term = TermQuery::new(
                                Term::from_field_text(field, word),
                                IndexRecordOption::WithFreqs,
                            );
                            let boost = (if field == self.fields.title {
                                10.0
                            } else {
                                1.0
                            }) * (if i == 0 { 1.0 } else { 0.5 });
                            (
                                Occur::Should,
                                Box::new(BoostQuery::new(Box::new(term), boost)) as Box<dyn Query>,
                            )
                        })
                    })
                    .collect();
                (
                    Occur::Must,
                    Box::new(BooleanQuery::new(choices)) as Box<dyn Query>,
                )
            })
            .collect();
        Ok(Box::new(BooleanQuery::new(clauses)))
    }
    fn fuzzy_plan(
        &self,
        plan: &Plan,
        field: Field,
        max_distance: u8,
        ticket: &Ticket,
        truncated: &mut bool,
    ) -> Result<Plan> {
        let mut result = plan.clone();
        for group in &mut result.groups {
            ticket.check()?;
            let word = group[0].clone();
            if group.len() > 1 {
                continue;
            } // Reuse corrections found in the title dictionary.
            // Preserve known words in multiword queries. Broadening "missing term"
            // into unrelated vocabulary hurts both precision and latency.
            if plan.groups.len() > 1
                && (self
                    .searcher
                    .doc_freq(&Term::from_field_text(self.fields.title, &word))?
                    > 0
                    || self
                        .searcher
                        .doc_freq(&Term::from_field_text(self.fields.body, &word))?
                        > 0)
            {
                continue;
            }
            let length = word.chars().count();
            let distance = if length < 4 {
                0
            } else if length < 9 {
                1
            } else {
                max_distance
            };
            if distance == 0 {
                continue;
            }
            let dfa = automaton(&word, distance);
            let mut candidates: BTreeMap<String, (u8, u64)> = BTreeMap::new();
            for segment in self.searcher.segment_readers() {
                let inverted = segment.inverted_index(field)?;
                let mut stream = inverted.terms().search(Dfa(&dfa)).into_stream()?;
                let mut seen = 0;
                while let Some((key, info)) = stream.next() {
                    ticket.check()?;
                    seen += 1;
                    if seen > MAX_ENUMERATED {
                        *truncated = true;
                        break;
                    }
                    let term = std::str::from_utf8(key)?.to_owned();
                    let Distance::Exact(distance) = dfa.eval(key) else {
                        continue;
                    };
                    let entry = candidates.entry(term).or_insert((distance, 0));
                    entry.1 += info.doc_freq as u64;
                }
            }
            let mut candidates: Vec<_> =
                candidates.into_iter().filter(|(s, _)| s != &word).collect();
            candidates.sort_by(|a, b| {
                a.1.0
                    .cmp(&b.1.0)
                    .then(b.1.1.cmp(&a.1.1))
                    .then(a.0.cmp(&b.0))
            });
            *truncated |= candidates.len() >= MAX_ALTERNATIVES;
            group.extend(
                candidates
                    .into_iter()
                    .take(MAX_ALTERNATIVES - 1)
                    .map(|(word, _)| word),
            );
        }
        Ok(result)
    }
    fn prefix_terms(
        &self,
        prefix: &str,
        ticket: &Ticket,
        truncated: &mut bool,
    ) -> Result<Vec<String>> {
        let mut candidates = BTreeMap::<String, u64>::new();
        for field in [self.fields.title, self.fields.body] {
            for segment in self.searcher.segment_readers() {
                let inverted = segment.inverted_index(field)?;
                let mut stream = inverted
                    .terms()
                    .range()
                    .ge(prefix.as_bytes())
                    .into_stream()?;
                let mut seen = 0;
                while let Some((key, info)) = stream.next() {
                    ticket.check()?;
                    if !key.starts_with(prefix.as_bytes()) {
                        break;
                    }
                    seen += 1;
                    if seen > MAX_ENUMERATED {
                        *truncated = true;
                        break;
                    }
                    *candidates
                        .entry(std::str::from_utf8(key)?.to_owned())
                        .or_default() +=
                        info.doc_freq as u64 * if field == self.fields.title { 10 } else { 1 };
                }
            }
        }
        let mut candidates: Vec<_> = candidates.into_iter().collect();
        candidates.sort_by(|a, b| {
            (b.0 == prefix)
                .cmp(&(a.0 == prefix))
                .then(b.1.cmp(&a.1))
                .then(a.0.cmp(&b.0))
        });
        *truncated |= candidates.len() > MAX_ALTERNATIVES;
        Ok(candidates
            .into_iter()
            .take(MAX_ALTERNATIVES)
            .map(|(s, _)| s)
            .collect())
    }
}
fn automaton(word: &str, distance: u8) -> DFA {
    static ONE: OnceLock<LevenshteinAutomatonBuilder> = OnceLock::new();
    static TWO: OnceLock<LevenshteinAutomatonBuilder> = OnceLock::new();
    let builder = if distance == 1 { &ONE } else { &TWO };
    builder
        .get_or_init(|| LevenshteinAutomatonBuilder::new(distance, true))
        .build_dfa(word)
}
// Tantivy's FuzzyTermQuery uses this same DFA/FST approach. Expanding ourselves
// allows a vocabulary cap, normal BM25 scoring, and corrected-word highlighting.
struct Dfa<'a>(&'a DFA);
impl Automaton for Dfa<'_> {
    type State = u32;
    fn start(&self) -> u32 {
        self.0.initial_state()
    }
    fn is_match(&self, state: &u32) -> bool {
        matches!(self.0.distance(*state), Distance::Exact(_))
    }
    fn can_match(&self, state: &u32) -> bool {
        *state != levenshtein_automata::SINK_STATE
    }
    fn accept(&self, state: &u32, byte: u8) -> u32 {
        self.0.transition(*state, byte)
    }
}
