//! Daemon-owned window layouts. All methods doing disk I/O run on a blocking worker.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Content {
    Terminal { backend_session_id: String },
    File { path: String, line: Option<u64>, col: Option<u64> },
    Browser { url: String },
    Dag,
    Changes,
    RemoteDesktop { host_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction { Horizontal, Vertical }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Node {
    Leaf { node_id: u64, content: Content },
    Split { dir: Direction, ratio: f64, a: Box<Node>, b: Box<Node> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub tab_id: String,
    pub label: String,
    pub pinned: bool,
    pub pane_tree: Node,
    pub active_node_id: u64,
    pub expanded_node_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TabGroupLayout {
    Group { group_id: String, tab_ids: Vec<String>, active_tab_id: Option<String> },
    Split { dir: Direction, ratio: f64, a: Box<TabGroupLayout>, b: Box<TabGroupLayout> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub revision: u64,
    pub tabs: Vec<Tab>,
    pub tab_group_layout: TabGroupLayout,
    pub active_tab_id: Option<String>,
    pub focused_group_id: String,
    next_node_id: u64,
}

impl Default for Layout {
    fn default() -> Self {
        Self { revision: 0, tabs: vec![], tab_group_layout: TabGroupLayout::Group {
            group_id: "main".into(), tab_ids: vec![], active_tab_id: None,
        }, active_tab_id: None, focused_group_id: "main".into(), next_node_id: 1 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum Intent {
    AddTabWithSession { tab_id: String, label: String, content: Content, activate: bool },
    CloseTab { tab_id: String },
    ReorderTab { tab_id: String, target_index: usize },
    RenameTab { tab_id: String, label: String },
    SetTabPinned { tab_id: String, pinned: bool },
    SplitPane { tab_id: String, target_node_id: u64, dir: Direction, first: bool, content: Content },
    MoveTabToGroup { source_tab_id: String, target_group_id: String, target_index: usize },
    MoveTabToSplit { source_tab_id: String, target_group_id: String, dir: Direction, first: bool },
    MoveTabToPaneSplit { source_tab_id: String, target_tab_id: String, target_node_id: u64, dir: Direction, first: bool },
    DetachPaneToTab { source_tab_id: String, node_id: u64, tab_id: String, label: String, target_group_id: String },
    ClosePane { tab_id: String, node_id: u64 },
    ActivateTab { tab_id: String },
    FocusPane { tab_id: String, node_id: u64 },
    SetPaneRatio { tab_id: String, path: String, ratio: f64 },
    SetTabGroupRatio { path: String, ratio: f64 },
    EqualizeTabGroupRun { path: String },
    EqualizePaneRun { tab_id: String, path: String },
    EqualizePanes { tab_id: String },
    SwapPanes { tab_id: String, source_node_id: u64, target_node_id: u64 },
    TogglePaneExpanded { tab_id: String, node_id: u64 },
    UpdateFileTab { tab_id: String, node_id: u64, path: String, line: Option<u64>, col: Option<u64> },
    UpdateBrowserTab { tab_id: String, node_id: u64, url: String },
}

pub type Snapshot = BTreeMap<String, Layout>;
pub type Registry = std::sync::Arc<parking_lot::Mutex<Store>>;

pub struct Store {
    root: PathBuf,
    pub layouts: Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub revision: u64,
    pub layout: Layout,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub migration_map: BTreeMap<String, u64>,
}

#[derive(Debug)]
pub enum ApplyError { Conflict(Layout), Invalid(String) }

fn ratio(value: f64) -> f64 { if value.is_finite() { value.clamp(0.1, 0.9) } else { 0.5 } }

impl Node {
    fn first(&self) -> u64 { match self { Self::Leaf { node_id, .. } => *node_id, Self::Split { a, .. } => a.first() } }
    fn find_mut(&mut self, id: u64) -> Option<&mut Node> {
        if matches!(self, Self::Leaf { node_id, .. } if *node_id == id) { return Some(self); }
        match self {
            Self::Leaf { .. } => None,
            Self::Split { a, b, .. } => a.find_mut(id).or_else(|| b.find_mut(id)),
        }
    }
    fn contains(&self, id: u64) -> bool {
        match self { Self::Leaf { node_id, .. } => *node_id == id, Self::Split { a, b, .. } => a.contains(id) || b.contains(id) }
    }
    fn remove(self, id: u64) -> Option<Node> {
        match self {
            Self::Leaf { node_id, .. } if node_id == id => None,
            Self::Split { dir, ratio, a, b } => match (a.remove(id), b.remove(id)) {
                (Some(a), Some(b)) => Some(Self::Split { dir, ratio, a: Box::new(a), b: Box::new(b) }),
                (a, b) => a.or(b),
            },
            node => Some(node),
        }
    }
    fn at(&mut self, path: &str) -> Option<&mut Node> {
        if path.is_empty() { return Some(self); }
        let (head, tail) = path.split_once('.').unwrap_or((path, ""));
        match self { Self::Split { a, b, .. } => match head {
            "first" | "a" => a.at(tail), "second" | "b" => b.at(tail), _ => None,
        }, _ => None }
    }
    fn equalize(&mut self) {
        if let Self::Split { ratio, a, b, .. } = self { *ratio = 0.5; a.equalize(); b.equalize(); }
    }
    fn equalize_run(&mut self, path: &str) -> Result<(), String> {
        let dir = match self.at(path) { Some(Self::Split { dir, .. }) => *dir, _ => return Err("Unknown split".into()) };
        let segments: Vec<_> = path.split('.').filter(|part| !part.is_empty()).collect();
        let mut scope = 0;
        let mut current = &*self;
        for (index, segment) in segments.iter().enumerate() {
            if let Self::Split { dir: ancestor, a, b, .. } = current {
                if *ancestor != dir { scope = index + 1; }
                current = if *segment == "first" || *segment == "a" { a } else { b };
            }
        }
        fn count(node: &Node, dir: Direction) -> usize {
            match node { Node::Split { dir: own, a, b, .. } if *own == dir => count(a, dir) + count(b, dir), _ => 1 }
        }
        fn apply(node: &mut Node, dir: Direction) {
            if let Node::Split { dir: own, ratio: value, a, b } = node {
                if *own == dir {
                    *value = ratio(count(a, dir) as f64 / (count(a, dir) + count(b, dir)) as f64);
                    apply(a, dir); apply(b, dir);
                }
            }
        }
        apply(self.at(&segments[..scope].join(".")).ok_or("Unknown split")?, dir);
        Ok(())
    }
    fn split(&mut self, id: u64, subtree: Node, dir: Direction, first: bool) -> Result<(), String> {
        let node = self.find_mut(id).ok_or("Unknown pane")?;
        let old = node.clone();
        let (a, b) = if first { (subtree, old) } else { (old, subtree) };
        *node = Self::Split { dir, ratio: 0.5, a: Box::new(a), b: Box::new(b) };
        Ok(())
    }
}

impl TabGroupLayout {
    fn prune(self) -> Option<Self> {
        match self {
            Self::Group { ref tab_ids, .. } if tab_ids.is_empty() => None,
            Self::Split { dir, ratio, a, b } => match (a.prune(), b.prune()) {
                (Some(a), Some(b)) => Some(Self::Split { dir, ratio, a: Box::new(a), b: Box::new(b) }),
                (a, b) => a.or(b),
            },
            node => Some(node),
        }
    }
    fn group_mut(&mut self, id: &str) -> Option<&mut Self> {
        if matches!(self, Self::Group { group_id, .. } if group_id == id) { return Some(self); }
        match self {
            Self::Group { .. } => None,
            Self::Split { a, b, .. } => a.group_mut(id).or_else(|| b.group_mut(id)),
        }
    }
    fn remove_tab(&mut self, id: &str) {
        match self {
            Self::Group { tab_ids, active_tab_id, .. } => {
                tab_ids.retain(|tab| tab != id);
                if active_tab_id.as_deref() == Some(id) { *active_tab_id = tab_ids.first().cloned(); }
            },
            Self::Split { a, b, .. } => { a.remove_tab(id); b.remove_tab(id); }
        }
    }
    fn first_group(&self) -> String { match self { Self::Group { group_id, .. } => group_id.clone(), Self::Split { a, .. } => a.first_group() } }
    fn insert(&mut self, group: &str, tab: String, index: usize) -> Result<(), String> {
        match self.group_mut(group).ok_or("Unknown group")? {
            Self::Group { tab_ids, active_tab_id, .. } => {
                tab_ids.insert(index.min(tab_ids.len()), tab.clone()); *active_tab_id = Some(tab); Ok(())
            }, _ => unreachable!(),
        }
    }
    fn at(&mut self, path: &str) -> Option<&mut Self> {
        if path.is_empty() { return Some(self); }
        let (head, tail) = path.split_once('.').unwrap_or((path, ""));
        match self { Self::Split { a, b, .. } => match head {
            "first" | "a" => a.at(tail), "second" | "b" => b.at(tail), _ => None,
        }, _ => None }
    }
    fn equalize_run(&mut self, path: &str) -> Result<(), String> {
        fn to_node(group: &TabGroupLayout) -> Node {
            match group {
                TabGroupLayout::Group { .. } => Node::Leaf { node_id: 0, content: Content::Dag },
                TabGroupLayout::Split { dir, ratio, a, b } => Node::Split { dir: *dir, ratio: *ratio, a: Box::new(to_node(a)), b: Box::new(to_node(b)) },
            }
        }
        fn copy(group: &mut TabGroupLayout, node: &Node) {
            if let (TabGroupLayout::Split { ratio, a, b, .. }, Node::Split { ratio: value, a: first, b: second, .. }) = (group, node) {
                *ratio = *value; copy(a, first); copy(b, second);
            }
        }
        let mut node = to_node(self); node.equalize_run(path)?; copy(self, &node); Ok(())
    }
}

impl Layout {
    pub(super) fn mint(&mut self, content: Content) -> Node {
        let node_id = self.next_node_id; self.next_node_id += 1; Node::Leaf { node_id, content }
    }
    fn tab(&mut self, id: &str) -> Result<&mut Tab, String> {
        self.tabs.iter_mut().find(|tab| tab.tab_id == id).ok_or_else(|| "Unknown tab".into())
    }
    fn close(&mut self, id: &str) -> Result<Tab, String> {
        let index = self.tabs.iter().position(|tab| tab.tab_id == id).ok_or("Unknown tab")?;
        let tab = self.tabs.remove(index); self.tab_group_layout.remove_tab(id);
        if self.active_tab_id.as_deref() == Some(id) { self.active_tab_id = self.tabs.first().map(|tab| tab.tab_id.clone()); }
        Ok(tab)
    }
    fn add(&mut self, tab: Tab, group: &str, activate: bool) -> Result<(), String> {
        if self.tabs.iter().any(|other| other.tab_id == tab.tab_id) { return Err("Duplicate tab".into()); }
        self.tab_group_layout.insert(group, tab.tab_id.clone(), usize::MAX)?;
        if activate { self.active_tab_id = Some(tab.tab_id.clone()); self.focused_group_id = group.into(); }
        self.tabs.push(tab); Ok(())
    }
    fn remove_pane(&mut self, tab_id: &str, id: u64) -> Result<Node, String> {
        let tab = self.tab(tab_id)?;
        let removed = tab.pane_tree.find_mut(id).ok_or("Unknown pane")?.clone();
        if let Some(root) = tab.pane_tree.clone().remove(id) {
            tab.pane_tree = root;
            if tab.active_node_id == id { tab.active_node_id = tab.pane_tree.first(); }
            if tab.expanded_node_id == Some(id) { tab.expanded_node_id = None; }
        } else { self.close(tab_id)?; }
        Ok(removed)
    }

    fn apply(&mut self, intent: Intent) -> Result<(), String> {
        use Intent::*;
        match intent {
            AddTabWithSession { tab_id, label, content, activate } => {
                let pane_tree = self.mint(content); let active_node_id = pane_tree.first();
                let group = self.focused_group_id.clone();
                self.add(Tab { tab_id, label, pinned: false, pane_tree, active_node_id, expanded_node_id: None }, &group, activate)?;
            },
            CloseTab { tab_id } => { self.close(&tab_id)?; },
            RenameTab { tab_id, label } => self.tab(&tab_id)?.label = label,
            SetTabPinned { tab_id, pinned } => self.tab(&tab_id)?.pinned = pinned,
            ReorderTab { tab_id, target_index } => {
                let index = self.tabs.iter().position(|tab| tab.tab_id == tab_id).ok_or("Unknown tab")?;
                let tab = self.tabs.remove(index); self.tabs.insert(target_index.min(self.tabs.len()), tab);
                fn reorder(group: &mut TabGroupLayout, id: &str, index: usize) {
                    match group {
                        TabGroupLayout::Group { tab_ids, .. } => if let Some(old) = tab_ids.iter().position(|tab| tab == id) {
                            let tab = tab_ids.remove(old); tab_ids.insert(index.min(tab_ids.len()), tab);
                        },
                        TabGroupLayout::Split { a, b, .. } => { reorder(a, id, index); reorder(b, id, index); }
                    }
                }
                reorder(&mut self.tab_group_layout, &tab_id, target_index);
            },
            SplitPane { tab_id, target_node_id, dir, first, content } => {
                let node = self.mint(content); let id = node.first();
                let tab = self.tab(&tab_id)?; tab.pane_tree.split(target_node_id, node, dir, first)?; tab.active_node_id = id;
            },
            MoveTabToGroup { source_tab_id, target_group_id, target_index } => {
                self.tab(&source_tab_id)?;
                self.tab_group_layout.remove_tab(&source_tab_id);
                self.tab_group_layout.insert(&target_group_id, source_tab_id.clone(), target_index)?;
                self.active_tab_id = Some(source_tab_id); self.focused_group_id = target_group_id;
            },
            MoveTabToSplit { source_tab_id, target_group_id, dir, first } => {
                self.tab(&source_tab_id)?;
                self.tab_group_layout.remove_tab(&source_tab_id);
                let group_id = format!("group-{}", self.next_node_id); self.next_node_id += 1;
                let target = self.tab_group_layout.group_mut(&target_group_id).ok_or("Unknown group")?;
                let new = TabGroupLayout::Group { group_id: group_id.clone(), tab_ids: vec![source_tab_id.clone()], active_tab_id: Some(source_tab_id.clone()) };
                let old = target.clone(); let (a, b) = if first { (new, old) } else { (old, new) };
                *target = TabGroupLayout::Split { dir, ratio: 0.5, a: Box::new(a), b: Box::new(b) };
                self.active_tab_id = Some(source_tab_id); self.focused_group_id = group_id;
            },
            MoveTabToPaneSplit { source_tab_id, target_tab_id, target_node_id, dir, first } => {
                if source_tab_id == target_tab_id { return Err("Cannot move tab into itself".into()); }
                let source = self.close(&source_tab_id)?;
                self.tab(&target_tab_id)?.pane_tree.split(target_node_id, source.pane_tree, dir, first)?;
                self.active_tab_id = Some(target_tab_id);
            },
            DetachPaneToTab { source_tab_id, node_id, tab_id, label, target_group_id } => {
                let pane_tree = self.remove_pane(&source_tab_id, node_id)?;
                self.add(Tab { tab_id, label, pinned: false, pane_tree, active_node_id: node_id, expanded_node_id: None }, &target_group_id, true)?;
            },
            ClosePane { tab_id, node_id } => { self.remove_pane(&tab_id, node_id)?; },
            ActivateTab { tab_id } => {
                self.tab(&tab_id)?;
                fn activate(group: &mut TabGroupLayout, id: &str) -> Option<String> {
                    match group {
                        TabGroupLayout::Group { group_id, tab_ids, active_tab_id } if tab_ids.iter().any(|tab| tab == id) => {
                            *active_tab_id = Some(id.into()); Some(group_id.clone())
                        },
                        TabGroupLayout::Split { a, b, .. } => activate(a, id).or_else(|| activate(b, id)), _ => None,
                    }
                }
                self.focused_group_id = activate(&mut self.tab_group_layout, &tab_id).ok_or("Tab has no group")?;
                self.active_tab_id = Some(tab_id);
            },
            FocusPane { tab_id, node_id } => {
                let tab = self.tab(&tab_id)?;
                if !tab.pane_tree.contains(node_id) { return Err("Unknown pane".into()); }
                tab.active_node_id = node_id;
            },
            TogglePaneExpanded { tab_id, node_id } => {
                let tab = self.tab(&tab_id)?;
                if !tab.pane_tree.contains(node_id) { return Err("Unknown pane".into()); }
                tab.expanded_node_id = if tab.expanded_node_id == Some(node_id) { None } else { Some(node_id) };
            },
            SetPaneRatio { tab_id, path, ratio: value } => {
                if let Node::Split { ratio, .. } = self.tab(&tab_id)?.pane_tree.at(&path).ok_or("Unknown split")? { *ratio = ratio_value(value); }
                else { return Err("Not a split".into()); }
            },
            SetTabGroupRatio { path, ratio: value } => {
                if let TabGroupLayout::Split { ratio, .. } = self.tab_group_layout.at(&path).ok_or("Unknown split")? { *ratio = ratio_value(value); }
                else { return Err("Not a split".into()); }
            },
            EqualizeTabGroupRun { path } => self.tab_group_layout.equalize_run(&path)?,
            EqualizePaneRun { tab_id, path } => self.tab(&tab_id)?.pane_tree.equalize_run(&path)?,
            EqualizePanes { tab_id } => self.tab(&tab_id)?.pane_tree.equalize(),
            SwapPanes { tab_id, source_node_id, target_node_id } => {
                let root = &mut self.tab(&tab_id)?.pane_tree;
                let source = root.find_mut(source_node_id).ok_or("Unknown source")?.clone();
                let target = root.find_mut(target_node_id).ok_or("Unknown target")?.clone();
                fn swap(root: &mut Node, source_id: u64, target_id: u64, source: &Node, target: &Node) {
                    match root {
                        Node::Leaf { node_id, .. } if *node_id == source_id => *root = target.clone(),
                        Node::Leaf { node_id, .. } if *node_id == target_id => *root = source.clone(),
                        Node::Split { a, b, .. } => { swap(a, source_id, target_id, source, target); swap(b, source_id, target_id, source, target); }, _ => (),
                    }
                }
                swap(root, source_node_id, target_node_id, &source, &target);
            },
            UpdateFileTab { tab_id, node_id, path, line, col } => {
                match self.tab(&tab_id)?.pane_tree.find_mut(node_id).ok_or("Unknown pane")? {
                    Node::Leaf { content: content @ Content::File { .. }, .. } => *content = Content::File { path, line, col },
                    _ => return Err("Not a file pane".into()),
                }
            },
            UpdateBrowserTab { tab_id, node_id, url } => {
                match self.tab(&tab_id)?.pane_tree.find_mut(node_id).ok_or("Unknown pane")? {
                    Node::Leaf { content: content @ Content::Browser { .. }, .. } => *content = Content::Browser { url },
                    _ => return Err("Not a browser pane".into()),
                }
            },
        }
        Ok(())
    }
}

fn ratio_value(value: f64) -> f64 { ratio(value) }

pub(super) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    decoder.disable_recursion_limit();
    let value = T::deserialize(&mut decoder)?;
    decoder.end()?;
    Ok(value)
}

impl Store {
    pub fn new(root: PathBuf) -> Self { Self { root, layouts: BTreeMap::new() } }
    fn path(&self, window: &str) -> Result<PathBuf, String> {
        crate::session::session_path_for_window(&self.root.join("session_state.json"), window).map_err(|error| error.to_string())?;
        Ok(self.root.join("layout").join(format!("{window}.json")))
    }
    pub fn get(&mut self, window: &str) -> Result<View, String> {
        let path = self.path(window)?;
        let mut migration_map = BTreeMap::new();
        if !self.layouts.contains_key(window) {
            if self.layouts.len() >= 32 { return Err("Window limit exceeded".into()); }
            let saved = match std::fs::read(&path) {
                Ok(bytes) => match decode::<Layout>(&bytes) {
                    Ok(layout) => Some(layout),
                    Err(error) => { tracing::warn!(%error, %window, "Corrupt daemon layout; recovering saved session"); None },
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            };
            let layout = match saved {
                Some(layout) => layout,
                None => {
                    let (layout, mapping) = super::layout_migration::migrate(&self.root, window)?;
                    migration_map = mapping; persist(&path, &layout)?; layout
                },
            };
            self.layouts.insert(window.into(), layout);
        }
        let layout = self.layouts.get(window).ok_or("Layout missing")?.clone();
        Ok(View { revision: layout.revision, layout, migration_map })
    }
    pub fn apply(&mut self, window: &str, base: u64, intent: Intent) -> Result<View, ApplyError> {
        let current = self.get(window).map_err(ApplyError::Invalid)?.layout;
        // Focus is independent of structural edits provided its target still exists.
        // Other stale mutations must be retried explicitly against the daemon's tree.
        let commutes = match &intent {
            Intent::FocusPane { tab_id, node_id } => current.tabs.iter().any(|tab| tab.tab_id == *tab_id && tab.pane_tree.contains(*node_id)),
            Intent::ActivateTab { tab_id } => current.tabs.iter().any(|tab| tab.tab_id == *tab_id),
            _ => false,
        };
        if base != current.revision && (base > current.revision || !commutes) { return Err(ApplyError::Conflict(current)); }
        let mut next = current;
        next.apply(intent).map_err(ApplyError::Invalid)?;
        next.tab_group_layout = next.tab_group_layout.clone().prune().unwrap_or_else(|| Layout::default().tab_group_layout);
        if next.tab_group_layout.group_mut(&next.focused_group_id).is_none() {
            next.focused_group_id = next.tab_group_layout.first_group();
        }
        fn ordered(group: &TabGroupLayout, ids: &mut Vec<String>) {
            match group {
                TabGroupLayout::Group { tab_ids, .. } => ids.extend(tab_ids.iter().cloned()),
                TabGroupLayout::Split { a, b, .. } => { ordered(a, ids); ordered(b, ids); }
            }
        }
        let mut ids = Vec::new(); ordered(&next.tab_group_layout, &mut ids);
        next.tabs.sort_by_key(|tab| ids.iter().position(|id| id == &tab.tab_id).unwrap_or(usize::MAX));
        next.revision = next.revision.checked_add(1).ok_or_else(|| ApplyError::Invalid("Revision exhausted".into()))?;
        if self.layouts.len() > 32 { return Err(ApplyError::Invalid("Window limit exceeded".into())); }
        let mut count = 0;
        fn count_nodes(node: &Node, count: &mut usize) {
            match node { Node::Leaf { .. } => *count += 1, Node::Split { a, b, .. } => { count_nodes(a, count); count_nodes(b, count); } }
        }
        for tab in &next.tabs { count_nodes(&tab.pane_tree, &mut count); }
        if count > 256 { return Err(ApplyError::Invalid("Pane limit exceeded".into())); }
        persist(&self.path(window).map_err(ApplyError::Invalid)?, &next).map_err(ApplyError::Invalid)?;
        self.layouts.insert(window.into(), next.clone());
        Ok(View { revision: next.revision, layout: next, migration_map: BTreeMap::new() })
    }
    pub fn adopt(&mut self, snapshot: Snapshot) -> Result<(), String> {
        for (window, layout) in &snapshot { persist(&self.path(window)?, layout)?; }
        self.layouts = snapshot; Ok(())
    }
}

pub(super) fn persist(path: &Path, value: &Layout) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() >= 1024 * 1024 { return Err("Layout exceeds 1 MiB".into()); }
    let parent = path.parent().ok_or("Layout path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&bytes).and_then(|_| file.as_file().sync_all()).map_err(|error| error.to_string())?;
    file.persist(path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    std::fs::File::open(parent).and_then(|dir| dir.sync_all()).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_reducer_golden_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let output = std::process::Command::new("bun").arg("ui/scripts/gen-layout-fixtures.ts")
            .current_dir(root).output().expect("Bun is required to generate UI reducer fixtures");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let cases: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
        for case in cases {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("session_state.json"), serde_json::to_vec(&case["session"]).unwrap()).unwrap();
            let mut store = Store::new(dir.path().into());
            let intent: Intent = serde_json::from_value(case["intent"].clone()).unwrap();
            let view = store.apply("main", 0, intent).unwrap();
            fn project(node: &Node) -> serde_json::Value {
                match node {
                    Node::Leaf { content, .. } => serde_json::json!({"type":"leaf","content":content}),
                    Node::Split { dir, ratio, a, b } => serde_json::json!({"type":"split","dir":dir,"ratio":ratio,"a":project(a),"b":project(b)}),
                }
            }
            let actual: Vec<_> = view.layout.tabs.iter().map(|tab| serde_json::json!({
                "tabId":tab.tab_id,"label":tab.label,"pinned":tab.pinned,"paneTree":project(&tab.pane_tree),
            })).collect();
            assert_eq!(serde_json::json!(actual), case["expected"], "intent: {}", case["intent"]);
        }
    }
    fn add(id: &str) -> Intent {
        Intent::AddTabWithSession { tab_id: id.into(), label: id.into(), content: Content::Terminal { backend_session_id: format!("pty-{id}") }, activate: true }
    }
    #[test]
    fn conflict_rebase_and_idle_restart() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new(root.path().into());
        let first = store.apply("main", 0, add("one")).unwrap();
        assert_eq!(first.revision, 1);
        assert!(matches!(store.apply("main", 0, Intent::CloseTab { tab_id: "one".into() }), Err(ApplyError::Conflict(layout)) if layout == first.layout));
        let focused = store.apply("main", 0, Intent::FocusPane { tab_id: "one".into(), node_id: 1 }).unwrap();
        assert_eq!(focused.revision, 2);
        let mut restarted = Store::new(root.path().into());
        assert_eq!(restarted.get("main").unwrap().layout, focused.layout);
        assert!(restarted.get("../escape").is_err());
        assert!(restarted.apply("main", 99, Intent::ActivateTab { tab_id: "one".into() }).is_err());
    }
    #[test]
    fn split_move_close_preserves_backend_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new(root.path().into());
        store.apply("main", 0, add("one")).unwrap();
        store.apply("main", 1, Intent::SplitPane { tab_id: "one".into(), target_node_id: 1, dir: Direction::Horizontal, first: false, content: Content::Browser { url: "https://example.org".into() } }).unwrap();
        store.apply("main", 2, Intent::DetachPaneToTab { source_tab_id: "one".into(), node_id: 2, tab_id: "two".into(), label: "two".into(), target_group_id: "main".into() }).unwrap();
        let view = store.apply("main", 3, Intent::CloseTab { tab_id: "two".into() }).unwrap();
        assert_eq!(view.revision, 4);
        assert_eq!(view.layout.tabs[0].pane_tree, Node::Leaf { node_id: 1, content: Content::Terminal { backend_session_id: "pty-one".into() } });
    }
    #[test]
    fn invalid_intent_is_transactional() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new(root.path().into());
        let before = store.apply("main", 0, add("one")).unwrap();
        assert!(store.apply("main", 1, Intent::MoveTabToPaneSplit { source_tab_id: "one".into(), target_tab_id: "missing".into(), target_node_id: 9, dir: Direction::Vertical, first: true }).is_err());
        assert_eq!(store.get("main").unwrap().layout, before.layout);
        assert_eq!(Store::new(root.path().into()).get("main").unwrap().layout, before.layout);
    }
    #[test]
    fn size_bound() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new(root.path().into());
        for index in 0..32 {
            let window = format!("ws-{index}");
            store.apply(&window, 0, add("one")).unwrap();
            for revision in 1..256 {
                store.apply(&window, revision, Intent::SplitPane { tab_id: "one".into(), target_node_id: revision, dir: Direction::Horizontal, first: false, content: Content::Terminal { backend_session_id: format!("pty-{index}-{revision}") } }).unwrap();
            }
            assert!(std::fs::metadata(root.path().join("layout").join(format!("{window}.json"))).unwrap().len() < 1024 * 1024);
            assert_eq!(Store::new(root.path().into()).get(&window).unwrap().revision, 256);
        }
        assert!(store.get("ws-overflow").is_err());
    }
    #[test]
    fn content_updates_tab_transplant_and_expansion() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::new(root.path().into());
        store.apply("main", 0, add("one")).unwrap();
        store.apply("main", 1, Intent::AddTabWithSession { tab_id: "file".into(), label: "file".into(),
            content: Content::File { path: "old".into(), line: None, col: None }, activate: true }).unwrap();
        store.apply("main", 2, Intent::UpdateFileTab { tab_id: "file".into(), node_id: 2, path: "new".into(), line: Some(10), col: Some(3) }).unwrap();
        store.apply("main", 3, Intent::MoveTabToPaneSplit { source_tab_id: "file".into(), target_tab_id: "one".into(), target_node_id: 1, dir: Direction::Vertical, first: false }).unwrap();
        store.apply("main", 4, Intent::SplitPane { tab_id: "one".into(), target_node_id: 1, dir: Direction::Horizontal, first: true,
            content: Content::Browser { url: "old".into() } }).unwrap();
        store.apply("main", 5, Intent::UpdateBrowserTab { tab_id: "one".into(), node_id: 3, url: "new".into() }).unwrap();
        let expanded = store.apply("main", 6, Intent::TogglePaneExpanded { tab_id: "one".into(), node_id: 3 }).unwrap();
        assert_eq!(expanded.layout.tabs[0].expanded_node_id, Some(3));
        let collapsed = store.apply("main", 7, Intent::TogglePaneExpanded { tab_id: "one".into(), node_id: 3 }).unwrap();
        let tab = &collapsed.layout.tabs[0];
        assert_eq!(tab.expanded_node_id, None);
        assert!(matches!(tab.pane_tree.clone().find_mut(2), Some(Node::Leaf { content: Content::File { path, line: Some(10), col: Some(3) }, .. }) if path == "new"));
        assert!(matches!(tab.pane_tree.clone().find_mut(3), Some(Node::Leaf { content: Content::Browser { url }, .. }) if url == "new"));
    }
}
