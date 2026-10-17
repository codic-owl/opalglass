//! Compact runs compose note transclusions into one virtualized Reading list.
//! Runs hold indices and instance IDs, never document bodies or prepared rows.
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Document {
        instance: u64,
        index: usize,
    },
    Fragment {
        instance: u64,
        index: usize,
        range: Range<usize>,
        part: usize,
    },
    Placeholder {
        instance: u64,
    },
    Header {
        instance: u64,
    },
    Footer {
        instance: u64,
    },
}

impl Row {
    pub(super) fn instance(&self) -> u64 {
        match *self {
            Self::Document { instance, .. }
            | Self::Fragment { instance, .. }
            | Self::Placeholder { instance }
            | Self::Header { instance }
            | Self::Footer { instance } => instance,
        }
    }
}

#[derive(Clone, Debug)]
enum Kind {
    Rows { instance: u64, first: usize },
    Single(Row),
}

#[derive(Clone, Debug)]
struct Run {
    list: Range<usize>,
    /// Host projected rows. Embedded runs retain their embedding row as anchor.
    host: Range<usize>,
    kind: Kind,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Projection {
    runs: Vec<Run>,
    count: usize,
}

#[derive(Clone, Debug)]
pub(super) struct Anchor {
    pub(super) row: Row,
    pub(super) host: usize,
    pub(super) offset: f32,
}

impl Projection {
    pub(super) fn len(&self) -> usize {
        self.count
    }

    pub(super) fn identity(count: usize) -> Self {
        let mut result = Self::default();
        result.rows(0, 0..count, 0);
        result
    }

    pub(super) fn rows(&mut self, instance: u64, range: Range<usize>, host: usize) {
        if range.is_empty() {
            return;
        }
        let host_range = host..host + if instance == 0 { range.len() } else { 1 };
        if let Some(previous) = self.runs.last_mut()
            && let Kind::Rows {
                instance: old,
                first,
            } = previous.kind
            && old == instance
            && first + previous.list.len() == range.start
            && (if instance == 0 {
                previous.host.end == host
            } else {
                previous.host == host_range
            })
        {
            previous.list.end += range.len();
            if instance == 0 {
                previous.host.end = host_range.end;
            }
        } else {
            self.runs.push(Run {
                list: self.count..self.count + range.len(),
                host: host_range,
                kind: Kind::Rows {
                    instance,
                    first: range.start,
                },
            });
        }
        self.count += range.len();
    }

    pub(super) fn push(&mut self, row: Row, host: usize) {
        self.runs.push(Run {
            list: self.count..self.count + 1,
            host: host..host + 1,
            kind: Kind::Single(row),
        });
        self.count += 1;
    }

    /// An embed label can span multiple host rows. All of those source rows
    /// navigate to the replacement, without allocating a reverse map per row.
    pub(super) fn cover_host_rows(&mut self, first: usize, host: Range<usize>) {
        let at = self.runs.partition_point(|run| run.list.end <= first);
        for run in &mut self.runs[at..] {
            debug_assert_eq!(run.host.start, host.start);
            run.host.end = host.end;
        }
    }

    pub(super) fn row(&self, index: usize) -> Option<Row> {
        let run = self.run(index)?;
        Some(match &run.kind {
            Kind::Rows { instance, first } => Row::Document {
                instance: *instance,
                index: first + index - run.list.start,
            },
            Kind::Single(row) => row.clone(),
        })
    }

    fn run(&self, index: usize) -> Option<&Run> {
        let at = self.runs.partition_point(|run| run.list.end <= index);
        self.runs.get(at).filter(|run| run.list.contains(&index))
    }

    pub(super) fn host_row(&self, index: usize) -> usize {
        self.run(index).map_or(0, |run| {
            if matches!(run.kind, Kind::Rows { instance: 0, .. }) {
                run.host.start + index - run.list.start
            } else {
                run.host.start
            }
        })
    }

    pub(super) fn list_index(&self, host: usize) -> usize {
        let at = self.runs.partition_point(|run| run.host.end <= host);
        self.runs
            .get(at)
            .map_or(self.count.saturating_sub(1), |run| {
                run.list.start
                    + if matches!(run.kind, Kind::Rows { instance: 0, .. }) {
                        host.saturating_sub(run.host.start).min(run.list.len() - 1)
                    } else {
                        0
                    }
            })
    }

    pub(super) fn anchor(&self, index: usize, offset: f32) -> Option<Anchor> {
        Some(Anchor {
            row: self.row(index)?,
            host: self.host_row(index),
            offset,
        })
    }

    pub(super) fn restore(&self, anchor: &Anchor) -> usize {
        self.find(&anchor.row)
            .unwrap_or_else(|| self.list_index(anchor.host))
    }

    pub(super) fn find_document_row(&self, instance: u64, index: usize) -> Option<usize> {
        self.find(&Row::Document { instance, index })
    }

    fn find(&self, row: &Row) -> Option<usize> {
        self.runs.iter().find_map(|run| match (&run.kind, row) {
            (
                Kind::Rows { instance, first },
                Row::Document {
                    instance: id,
                    index,
                },
            ) if instance == id && *index >= *first && *index - first < run.list.len() => {
                Some(run.list.start + index - first)
            }
            (Kind::Single(candidate), _) if candidate == row => Some(run.list.start),
            _ => None,
        })
    }
}

#[cfg(test)]
#[path = "reading_projection/tests.rs"]
mod tests;
