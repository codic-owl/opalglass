//! Cached navigation and per-vault UI state. Expansion never touches the disk.
use crate::vault::Vault;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
    path::{Path, PathBuf},
};

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
    pub fn rebuild(&mut self, vault: &Vault, expanded: &HashSet<PathBuf>) {
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
        for children in self.children.values_mut() {
            children.sort_by_cached_key(|e| (!e.folder, e.name().to_lowercase(), e.path.clone()));
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
}
impl Default for State {
    fn default() -> Self {
        Self {
            expanded: HashSet::new(),
            selected: None,
            scroll_y: 0.,
            width: 270.,
            hidden: false,
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
    fn folders_first_expansion_and_independent_persistence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Zebra/Empty")).unwrap();
        std::fs::write(dir.path().join("apple.md"), "").unwrap();
        std::fs::write(dir.path().join("Zebra/note.md"), "").unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let mut state = State::default();
        let mut tree = Tree::default();
        tree.rebuild(&vault, &state.expanded);
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
        let store = tempfile::tempdir().unwrap();
        state.save(store.path(), &vault.root).unwrap();
        assert_eq!(State::load(store.path(), &vault.root), state);
        assert_eq!(
            State::load(store.path(), Path::new("/different")),
            State::default()
        );
    }
}
