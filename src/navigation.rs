//! Cached navigation and per-vault UI state. Expansion never touches the disk.
use crate::vault::Vault;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    #[default]
    NameAscending,
    NameDescending,
    ModifiedNewest,
    ModifiedOldest,
    CreatedNewest,
    CreatedOldest,
}
impl SortOrder {
    pub const ALL: [Self; 6] = [
        Self::NameAscending,
        Self::NameDescending,
        Self::ModifiedNewest,
        Self::ModifiedOldest,
        Self::CreatedNewest,
        Self::CreatedOldest,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NameAscending => "File name (A to Z)",
            Self::NameDescending => "File name (Z to A)",
            Self::ModifiedNewest => "Modified time (new to old)",
            Self::ModifiedOldest => "Modified time (old to new)",
            Self::CreatedNewest => "Created time (new to old)",
            Self::CreatedOldest => "Created time (old to new)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub folder: bool,
    pub depth: usize,
}
impl Entry {
    pub fn name(&self) -> String {
        let name = if self.folder {
            self.path.file_name()
        } else {
            self.path.file_stem()
        };
        name.unwrap_or_default().to_string_lossy().into_owned()
    }
}
#[derive(Default)]
pub struct Tree {
    children: HashMap<PathBuf, Vec<Entry>>,
    pub visible: Vec<Entry>,
}
impl Tree {
    pub fn rebuild(&mut self, vault: &Vault, expanded: &HashSet<PathBuf>, order: SortOrder) {
        self.children.clear();
        for (path, folder) in vault
            .folders
            .iter()
            .map(|p| (p, true))
            .chain(vault.notes.iter().map(|n| (&n.path, false)))
        {
            self.children
                .entry(path.parent().unwrap_or(Path::new("")).into())
                .or_default()
                .push(Entry {
                    path: path.clone(),
                    folder,
                    depth: path.components().count() - 1,
                });
        }
        // Metadata comes from the background vault scan; sorting never stats
        // files on the UI thread. Unknown timestamps sort last in either direction.
        let dates: HashMap<_, _> = vault
            .notes
            .iter()
            .map(|n| (&n.path, (n.modified(), n.created())))
            .collect();
        for children in self.children.values_mut() {
            let names: HashMap<_, _> = children
                .iter()
                .map(|e| (e.path.clone(), e.name().to_lowercase()))
                .collect();
            children.sort_by(|a, b| {
                let alphabetical = || {
                    names[&a.path]
                        .cmp(&names[&b.path])
                        .then(a.path.cmp(&b.path))
                };
                (!a.folder).cmp(&!b.folder).then_with(|| match order {
                    SortOrder::NameAscending => alphabetical(),
                    SortOrder::NameDescending => alphabetical().reverse(),
                    _ if a.folder => alphabetical(),
                    _ => {
                        let date = |entry: &Entry| {
                            dates.get(&entry.path).and_then(|&(modified, created)| {
                                if matches!(
                                    order,
                                    SortOrder::ModifiedNewest | SortOrder::ModifiedOldest
                                ) {
                                    modified
                                } else {
                                    created
                                }
                            })
                        };
                        let (a_date, b_date) = (date(a), date(b));
                        a_date
                            .is_none()
                            .cmp(&b_date.is_none())
                            .then_with(|| {
                                if matches!(
                                    order,
                                    SortOrder::ModifiedNewest | SortOrder::CreatedNewest
                                ) {
                                    b_date.cmp(&a_date)
                                } else {
                                    a_date.cmp(&b_date)
                                }
                            })
                            .then_with(alphabetical)
                    }
                })
            });
        }
        self.flatten(expanded);
    }
    pub fn flatten(&mut self, expanded: &HashSet<PathBuf>) {
        self.visible.clear();
        let mut stack = vec![(Path::new(""), 0usize)];
        while let Some((parent, index)) = stack.pop() {
            let Some(entry) = self.children.get(parent).and_then(|c| c.get(index)) else {
                continue;
            };
            stack.push((parent, index + 1));
            self.visible.push(entry.clone());
            if entry.folder && expanded.contains(&entry.path) {
                stack.push((&entry.path, 0));
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    pub expanded: HashSet<PathBuf>,
    pub selected: Option<PathBuf>,
    pub scroll_y: f32,
    pub width: f32,
    pub hidden: bool,
    pub sort: SortOrder,
    pub auto_reveal: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            expanded: HashSet::new(),
            selected: None,
            scroll_y: 0.,
            width: 270.,
            hidden: false,
            sort: SortOrder::default(),
            auto_reveal: false,
        }
    }
}
impl State {
    pub fn load(directory: &Path, root: &Path) -> Self {
        let mut state: Self = std::fs::read(state_file(directory, root))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        state.width = if state.width.is_finite() {
            state.width.clamp(180., 600.)
        } else {
            270.
        };
        state.scroll_y = if state.scroll_y.is_finite() {
            state.scroll_y.min(0.)
        } else {
            0.
        };
        state
    }
    pub fn save(&self, directory: &Path, root: &Path) -> io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let mut temp = tempfile::NamedTempFile::new_in(directory)?;
        temp.write_all(&serde_json::to_vec(self)?)?;
        temp.persist(state_file(directory, root))
            .map_err(|e| e.error)?;
        Ok(())
    }
    pub fn reveal(&mut self, path: &Path) {
        let mut parent = path.parent();
        while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
            self.expanded.insert(p.into());
            parent = p.parent();
        }
        self.selected = Some(path.into());
    }
}
fn state_file(directory: &Path, root: &Path) -> PathBuf {
    // Stable across process launches, unlike randomized HashMap hashing.
    let hash = root
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
    directory.join(format!("{hash:016x}.json"))
}
pub fn state_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
        .map(|p| p.join("opalglass/navigation"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sort_orders_keep_folders_first_and_use_cached_dates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Folder")).unwrap();
        for (name, seconds) in [("Alpha.md", 300), ("Beta.md", 100), ("Gamma.md", 200)] {
            let file = std::fs::File::create(dir.path().join(name)).unwrap();
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
                .unwrap();
        }
        let vault = Vault::open(dir.path()).unwrap();
        let mut tree = Tree::default();
        for (order, expected) in [
            (
                SortOrder::NameAscending,
                ["Folder", "Alpha", "Beta", "Gamma"],
            ),
            (
                SortOrder::NameDescending,
                ["Folder", "Gamma", "Beta", "Alpha"],
            ),
            (
                SortOrder::ModifiedNewest,
                ["Folder", "Alpha", "Gamma", "Beta"],
            ),
            (
                SortOrder::ModifiedOldest,
                ["Folder", "Beta", "Gamma", "Alpha"],
            ),
        ] {
            tree.rebuild(&vault, &HashSet::new(), order);
            assert_eq!(
                tree.visible.iter().map(Entry::name).collect::<Vec<_>>(),
                expected
            );
        }
        // File creation times are optional on supported filesystems. The cache
        // must faithfully expose that metadata, never substitute modified time.
        for note in vault.notes.iter() {
            assert_eq!(
                note.created(),
                std::fs::metadata(dir.path().join(&note.path))
                    .unwrap()
                    .created()
                    .ok()
            );
        }
        let discovery =
            Vault::discover(dir.path(), &std::sync::atomic::AtomicBool::new(false)).unwrap();
        for order in [SortOrder::CreatedNewest, SortOrder::CreatedOldest] {
            tree.rebuild(&discovery, &HashSet::new(), order);
            assert_eq!(
                tree.visible.iter().map(Entry::name).collect::<Vec<_>>(),
                ["Folder", "Alpha", "Beta", "Gamma"]
            );
        }
        // Older navigation files remain loadable with the new defaults.
        let old: State = serde_json::from_str("{\"width\":320,\"hidden\":true}").unwrap();
        assert_eq!(old.sort, SortOrder::NameAscending);
        assert!(!old.auto_reveal);
        assert_eq!(old.width, 320.);
    }
    #[test]
    fn folders_first_expansion_and_independent_persistence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Zebra/Empty")).unwrap();
        std::fs::write(dir.path().join("apple.md"), "").unwrap();
        std::fs::write(dir.path().join("Zebra/note.md"), "").unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let mut state = State::default();
        let mut tree = Tree::default();
        tree.rebuild(&vault, &state.expanded, state.sort);
        assert_eq!(
            tree.visible.iter().map(Entry::name).collect::<Vec<_>>(),
            ["Zebra", "apple"]
        );
        state.reveal(Path::new("Zebra/note.md"));
        tree.flatten(&state.expanded);
        assert_eq!(
            tree.visible.iter().map(Entry::name).collect::<Vec<_>>(),
            ["Zebra", "Empty", "note", "apple"]
        );
        state.scroll_y = -120.;
        state.width = 350.;
        state.hidden = true;
        state.sort = SortOrder::ModifiedNewest;
        state.auto_reveal = true;
        let store = tempfile::tempdir().unwrap();
        state.save(store.path(), &vault.root).unwrap();
        assert_eq!(State::load(store.path(), &vault.root), state);
        assert_eq!(
            State::load(store.path(), Path::new("/different")),
            State::default()
        );
    }
}
