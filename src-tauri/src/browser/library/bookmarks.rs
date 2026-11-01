//! Durable bookmark store (ported from maho-core `bookmark_manager.rs`), kept as one JSON file
//! in the Ferryx data dir. Every save writes a temp file and renames it over the store, and the
//! previous good copy is kept as `bookmarks.json.bak`, so a torn or corrupted file recovers from
//! the backup on the next load. All functions do blocking file I/O: call them through
//! `crate::ipc::run_blocking`.
//!
//! Import lane contract: [`BookmarkStore::import`] takes plain [`ImportedBookmark`] values (title,
//! url, folder path) and creates the folders by name under the chosen root.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The bookmark bar root. Roots are not stored as folders; they are fixed parent ids.
pub const BAR_ROOT: &str = "bar";
/// The "Other bookmarks" root.
pub const OTHER_ROOT: &str = "other";

const STORE_VERSION: u32 = 1;
const FILE_NAME: &str = "bookmarks.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkFolder {
    pub id: String,
    pub name: String,
    pub parent_id: String,
    pub position: u32,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub url: String,
    pub parent_id: String,
    pub position: u32,
    pub created_at: i64,
}

/// The whole store as the UI reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkTree {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub folders: Vec<BookmarkFolder>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

/// One bookmark handed over by an importer: `folder_path` names nested folders under the root
/// (empty = directly in the root).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedBookmark {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub folder_path: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BookmarkError {
    #[error("bookmark store io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bookmark store encode: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("bookmark or folder {0} not found")]
    NotFound(String),
    #[error("invalid bookmark request: {0}")]
    Invalid(String),
}

/// How a load went, so a recovery is visible to the caller instead of silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    Fresh,
    Loaded,
    RecoveredFromBackup,
    /// Both the store and its backup were unreadable; they were moved aside and the store
    /// starts empty.
    ResetCorrupt,
}

pub struct BookmarkStore {
    path: PathBuf,
    tree: BookmarkTree,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn is_root(id: &str) -> bool {
    id == BAR_ROOT || id == OTHER_ROOT
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

fn read_tree(path: &Path) -> Option<BookmarkTree> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<BookmarkTree>(&bytes).ok()
}

fn validate_url(url: &str) -> Result<String, BookmarkError> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if trimmed.is_empty()
        || !(lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://"))
    {
        return Err(BookmarkError::Invalid(format!("unsupported bookmark url: {trimmed}")));
    }
    Ok(trimmed.to_string())
}

impl BookmarkStore {
    /// `<data>/browser/bookmarks.json`.
    pub fn default_path(data_dir: &Path) -> PathBuf {
        data_dir.join("browser").join(FILE_NAME)
    }

    /// Opens the store, recovering from the backup when the main file is unreadable.
    pub fn open(path: impl Into<PathBuf>) -> (Self, LoadOutcome) {
        let path = path.into();
        let backup = backup_path(&path);
        let (tree, outcome) = if !path.exists() && !backup.exists() {
            (BookmarkTree::default(), LoadOutcome::Fresh)
        } else if let Some(tree) = read_tree(&path) {
            (tree, LoadOutcome::Loaded)
        } else if let Some(tree) = read_tree(&backup) {
            (tree, LoadOutcome::RecoveredFromBackup)
        } else {
            // Keep the unreadable bytes for a human instead of overwriting them on the next save.
            let stamp = now_ms();
            for candidate in [&path, &backup] {
                if candidate.exists() {
                    let mut name = candidate.file_name().unwrap_or_default().to_os_string();
                    name.push(format!(".corrupt-{stamp}"));
                    let _ = std::fs::rename(candidate, candidate.with_file_name(name));
                }
            }
            (BookmarkTree::default(), LoadOutcome::ResetCorrupt)
        };
        (Self { path, tree }, outcome)
    }

    pub fn tree(&self) -> &BookmarkTree {
        &self.tree
    }

    /// Atomic save: temp file + rename, with the previous good file kept as the backup.
    pub fn save(&mut self) -> Result<(), BookmarkError> {
        self.tree.version = STORE_VERSION;
        let parent = self.path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&parent)?;
        let bytes = serde_json::to_vec_pretty(&self.tree)?;
        let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
        std::io::Write::write_all(&mut temp, &bytes)?;
        temp.as_file().sync_all()?;
        if read_tree(&self.path).is_some() {
            std::fs::copy(&self.path, backup_path(&self.path))?;
        }
        temp.persist(&self.path).map_err(|error| BookmarkError::Io(error.error))?;
        Ok(())
    }

    fn parent_exists(&self, parent_id: &str) -> bool {
        is_root(parent_id) || self.tree.folders.iter().any(|folder| folder.id == parent_id)
    }

    fn next_position(&self, parent_id: &str) -> u32 {
        let folders = self.tree.folders.iter().filter(|f| f.parent_id == parent_id).map(|f| f.position);
        let marks = self.tree.bookmarks.iter().filter(|b| b.parent_id == parent_id).map(|b| b.position);
        folders.chain(marks).max().map_or(0, |max| max + 1)
    }

    /// Children (folders and bookmarks) of `parent_id` in display order, as `(id, is_folder)`.
    pub fn children(&self, parent_id: &str) -> Vec<(String, bool)> {
        let mut items: Vec<(u32, String, bool)> = self
            .tree
            .folders
            .iter()
            .filter(|f| f.parent_id == parent_id)
            .map(|f| (f.position, f.id.clone(), true))
            .chain(
                self.tree
                    .bookmarks
                    .iter()
                    .filter(|b| b.parent_id == parent_id)
                    .map(|b| (b.position, b.id.clone(), false)),
            )
            .collect();
        items.sort_by_key(|(position, ..)| *position);
        items.into_iter().map(|(_, id, folder)| (id, folder)).collect()
    }

    fn renumber(&mut self, parent_id: &str, order: &[String]) {
        for (index, id) in order.iter().enumerate() {
            let position = u32::try_from(index).unwrap_or(u32::MAX);
            if let Some(folder) = self.tree.folders.iter_mut().find(|f| &f.id == id) {
                folder.parent_id = parent_id.to_string();
                folder.position = position;
            } else if let Some(mark) = self.tree.bookmarks.iter_mut().find(|b| &b.id == id) {
                mark.parent_id = parent_id.to_string();
                mark.position = position;
            }
        }
    }

    pub fn add_bookmark(&mut self, title: &str, url: &str, parent_id: &str) -> Result<Bookmark, BookmarkError> {
        let url = validate_url(url)?;
        if !self.parent_exists(parent_id) {
            return Err(BookmarkError::NotFound(parent_id.to_string()));
        }
        let title = title.trim();
        let bookmark = Bookmark {
            id: uuid::Uuid::new_v4().to_string(),
            title: if title.is_empty() { url.clone() } else { title.to_string() },
            url,
            parent_id: parent_id.to_string(),
            position: self.next_position(parent_id),
            created_at: now_ms(),
        };
        self.tree.bookmarks.push(bookmark.clone());
        Ok(bookmark)
    }

    pub fn create_folder(&mut self, name: &str, parent_id: &str) -> Result<BookmarkFolder, BookmarkError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(BookmarkError::Invalid("folder name is empty".into()));
        }
        if !self.parent_exists(parent_id) {
            return Err(BookmarkError::NotFound(parent_id.to_string()));
        }
        let folder = BookmarkFolder {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            parent_id: parent_id.to_string(),
            position: self.next_position(parent_id),
            created_at: now_ms(),
        };
        self.tree.folders.push(folder.clone());
        Ok(folder)
    }

    /// Renames a folder or edits a bookmark's title and/or url.
    pub fn update(&mut self, id: &str, title: Option<&str>, url: Option<&str>) -> Result<(), BookmarkError> {
        if let Some(folder) = self.tree.folders.iter_mut().find(|f| f.id == id) {
            if let Some(name) = title.map(str::trim).filter(|name| !name.is_empty()) {
                folder.name = name.to_string();
            }
            return Ok(());
        }
        let url = url.map(validate_url).transpose()?;
        let mark = self
            .tree
            .bookmarks
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or_else(|| BookmarkError::NotFound(id.to_string()))?;
        if let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) {
            mark.title = title.to_string();
        }
        if let Some(url) = url {
            mark.url = url;
        }
        Ok(())
    }

    fn is_descendant_or_self(&self, candidate: &str, ancestor: &str) -> bool {
        let mut current = Some(candidate.to_string());
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.tree.folders.iter().find(|f| f.id == id).map(|f| f.parent_id.clone());
        }
        false
    }

    /// Moves a folder or bookmark under `parent_id` at `index` (clamped; None = end).
    pub fn move_item(&mut self, id: &str, parent_id: &str, index: Option<usize>) -> Result<(), BookmarkError> {
        let is_folder = self.tree.folders.iter().any(|f| f.id == id);
        if !is_folder && !self.tree.bookmarks.iter().any(|b| b.id == id) {
            return Err(BookmarkError::NotFound(id.to_string()));
        }
        if !self.parent_exists(parent_id) {
            return Err(BookmarkError::NotFound(parent_id.to_string()));
        }
        if is_folder && self.is_descendant_or_self(parent_id, id) {
            return Err(BookmarkError::Invalid("a folder cannot move into itself".into()));
        }
        let old_parent = self
            .tree
            .folders
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.parent_id.clone())
            .or_else(|| self.tree.bookmarks.iter().find(|b| b.id == id).map(|b| b.parent_id.clone()))
            .unwrap_or_default();
        let mut order: Vec<String> =
            self.children(parent_id).into_iter().map(|(child, _)| child).filter(|child| child != id).collect();
        let at = index.unwrap_or(order.len()).min(order.len());
        order.insert(at, id.to_string());
        self.renumber(parent_id, &order);
        if old_parent != parent_id {
            let rest: Vec<String> = self.children(&old_parent).into_iter().map(|(child, _)| child).collect();
            self.renumber(&old_parent, &rest);
        }
        Ok(())
    }

    /// Deletes a bookmark, or a folder with everything inside it.
    pub fn delete(&mut self, id: &str) -> Result<(), BookmarkError> {
        if let Some(index) = self.tree.bookmarks.iter().position(|b| b.id == id) {
            self.tree.bookmarks.remove(index);
            return Ok(());
        }
        if !self.tree.folders.iter().any(|f| f.id == id) {
            return Err(BookmarkError::NotFound(id.to_string()));
        }
        let doomed: Vec<String> = self
            .tree
            .folders
            .iter()
            .filter(|f| self.is_descendant_or_self(&f.id, id))
            .map(|f| f.id.clone())
            .collect();
        self.tree.folders.retain(|f| !doomed.contains(&f.id));
        self.tree.bookmarks.retain(|b| !doomed.contains(&b.parent_id));
        Ok(())
    }

    /// Bookmarks whose title or url contains every whitespace-separated term (case-insensitive).
    pub fn search(&self, query: &str, limit: usize) -> Vec<Bookmark> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut hits: Vec<Bookmark> = self
            .tree
            .bookmarks
            .iter()
            .filter(|b| {
                let hay = format!("{} {}", b.title.to_lowercase(), b.url.to_lowercase());
                terms.iter().all(|term| hay.contains(term))
            })
            .cloned()
            .collect();
        hits.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        hits.truncate(limit);
        hits
    }

    /// Bulk insert for importers: folders along each `folder_path` are reused by name under the
    /// same parent, and a url already present in its target folder is skipped. Returns how many
    /// bookmarks were added.
    pub fn import(&mut self, root: &str, entries: &[ImportedBookmark]) -> Result<usize, BookmarkError> {
        if !is_root(root) {
            return Err(BookmarkError::Invalid(format!("unknown bookmark root: {root}")));
        }
        let mut added = 0;
        for entry in entries {
            let mut parent = root.to_string();
            for name in entry.folder_path.iter().map(|name| name.trim()).filter(|name| !name.is_empty()) {
                let existing =
                    self.tree.folders.iter().find(|f| f.parent_id == parent && f.name == name).map(|f| f.id.clone());
                parent = match existing {
                    Some(id) => id,
                    None => self.create_folder(name, &parent)?.id,
                };
            }
            let Ok(url) = validate_url(&entry.url) else { continue };
            if self.tree.bookmarks.iter().any(|b| b.parent_id == parent && b.url == url) {
                continue;
            }
            self.add_bookmark(&entry.title, &url, &parent)?;
            added += 1;
        }
        Ok(added)
    }
}
