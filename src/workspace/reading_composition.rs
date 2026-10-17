//! Per-tab instance state. Documents remain owned by the bounded loading cache.
use super::*;
use embedded_documents::{Input, SourceToken};
use reading_projection::Projection;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Weak,
};

#[derive(Clone)]
pub(super) struct Instance {
    pub(super) id: u64,
    pub(super) parent: u64,
    pub(super) host: usize,
    pub(super) origin: PathBuf,
    pub(super) label: Arc<str>,
    pub(super) target: Option<opalglass::embeds::Target>,
    pub(super) input: Option<Input>,
    pub(super) selected_source: Option<std::ops::Range<usize>>,
    pub(super) projection: opalglass::callouts::Projection,
    pub(super) rows: std::ops::Range<usize>,
    pub(super) depth: usize,
    pub(super) error: Option<Arc<str>>,
}

struct Demand {
    used: u64,
    lease: Weak<()>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) projection: Projection,
    pub(super) instances: HashMap<u64, Instance>,
    pub(super) folds: HashMap<u64, BTreeMap<usize, bool>>,
    pub(super) key: Option<(usize, u64, u64, u64, u64, u64)>,
    pub(super) generation: u64,
    demands: HashMap<u64, Demand>,
    clock: u64,
}

impl State {
    pub(super) fn new(count: usize) -> Self {
        Self {
            projection: Projection::identity(count),
            ..Self::default()
        }
    }

    pub(super) fn enabled(&self, id: u64) -> bool {
        self.demands.contains_key(&id)
    }

    /// Only recently demanded instances expand. Repeated occurrences of one
    /// cached note cannot retain an unbounded expanded tree offscreen.
    pub(super) fn demand(&mut self, id: u64) -> Option<(Arc<()>, bool)> {
        self.clock = self.clock.wrapping_add(1);
        let fresh = !self.demands.contains_key(&id);
        if fresh && self.demands.len() >= 64 {
            let oldest = self
                .demands
                .iter()
                .filter(|(_, demand)| demand.lease.strong_count() == 0)
                .min_by_key(|(_, demand)| demand.used)
                .map(|(&id, _)| id)?;
            self.demands.remove(&oldest);
            self.folds.remove(&oldest);
        }
        let demand = self.demands.entry(id).or_insert(Demand {
            used: 0,
            lease: Weak::new(),
        });
        demand.used = self.clock;
        let lease = demand.lease.upgrade().unwrap_or_else(|| Arc::new(()));
        demand.lease = Arc::downgrade(&lease);
        if fresh {
            self.generation = self.generation.wrapping_add(1);
        }
        Some((lease, fresh))
    }

    pub(super) fn token(&self, id: u64) -> Option<SourceToken> {
        self.instances
            .get(&id)
            .and_then(|instance| instance.input.as_ref())
            .map(|input| input.token)
    }
}
