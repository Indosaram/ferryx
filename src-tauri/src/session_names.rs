//! Readable default names for terminal sessions: `<adjective>-<noun>`.
//!
//! A name is derived from the session id (FNV-1a 32-bit over its UTF-8 bytes), so the same
//! session keeps the same name across reads and the UI can derive the identical name on its own
//! (`ui/src/lib/sessionNames.ts` mirrors this exactly). The registry only resolves collisions
//! inside one workspace: the first session to claim a name keeps it bare, later ones get `-2`,
//! `-3`, ... A name, once given, never changes while the session lives; a user rename lives in
//! the UI (`customLabel`) and always wins over this default.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

const ADJECTIVES_TXT: &str = include_str!("../resources/names/adjectives.txt");
const NOUNS_TXT: &str = include_str!("../resources/names/nouns.txt");

fn words(text: &'static str) -> Vec<&'static str> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).collect()
}

fn lists() -> &'static (Vec<&'static str>, Vec<&'static str>) {
    static LISTS: OnceLock<(Vec<&'static str>, Vec<&'static str>)> = OnceLock::new();
    LISTS.get_or_init(|| (words(ADJECTIVES_TXT), words(NOUNS_TXT)))
}

fn fnv1a(text: &str) -> u32 {
    text.bytes()
        .fold(0x811c_9dc5_u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}

/// The bare two-word name for `session_id`, before any collision suffix.
pub fn base_name(session_id: &str) -> String {
    let (adjectives, nouns) = lists();
    let hash = fnv1a(session_id) as usize;
    let adjective = adjectives[hash % adjectives.len()];
    let noun = nouns[(hash / adjectives.len()) % nouns.len()];
    format!("{adjective}-{noun}")
}

#[derive(Default)]
struct Registry {
    /// workspace -> session id -> given name
    by_workspace: HashMap<String, HashMap<String, String>>,
}

impl Registry {
    fn name_for(&mut self, workspace: &str, session_id: &str) -> String {
        let names = self.by_workspace.entry(workspace.to_owned()).or_default();
        if let Some(name) = names.get(session_id) {
            return name.clone();
        }
        let taken: HashSet<&str> = names.values().map(String::as_str).collect();
        let base = base_name(session_id);
        let name = std::iter::once(base.clone())
            .chain((2..).map(|n| format!("{base}-{n}")))
            .find(|candidate| !taken.contains(candidate.as_str()))
            .expect("an unbounded suffix sequence always has a free name");
        names.insert(session_id.to_owned(), name.clone());
        name
    }

    fn retain(&mut self, live: &HashSet<&str>) {
        for names in self.by_workspace.values_mut() {
            names.retain(|id, _| live.contains(id.as_str()));
        }
        self.by_workspace.retain(|_, names| !names.is_empty());
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// The display name of `session_id`, unique within `workspace` (`None` = no workspace).
pub fn name_for(workspace: Option<&str>, session_id: &str) -> String {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .name_for(workspace.unwrap_or_default(), session_id)
}

/// Forget names of sessions that no longer exist, so their names can be given again.
pub fn retain_live<'a>(live: impl IntoIterator<Item = &'a str>) {
    let live: HashSet<&str> = live.into_iter().collect();
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(&live);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_two_words(name: &str) -> bool {
        let mut parts = name.split('-');
        let ok = |part: Option<&str>| {
            part.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()))
        };
        ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
    }

    #[test]
    fn session_names_word_lists_are_lowercase_unique_and_about_256_each() {
        let (adjectives, nouns) = lists();
        for list in [adjectives, nouns] {
            assert!((240..=272).contains(&list.len()), "list has {} words", list.len());
            assert!(list
                .iter()
                .all(|word| !word.is_empty() && word.bytes().all(|b| b.is_ascii_lowercase())));
            assert_eq!(list.iter().collect::<HashSet<_>>().len(), list.len(), "duplicate word");
        }
        assert!(!ADJECTIVES_TXT.contains('\r') && !NOUNS_TXT.contains('\r'), "word lists must be LF");
    }

    #[test]
    fn session_names_base_name_is_two_words_and_stable() {
        let name = base_name("session-1");
        assert!(is_two_words(&name), "{name}");
        assert_eq!(name, base_name("session-1"));
        // Pinned so the UI mirror (ui/src/lib/sessionNames.ts) is held to the same derivation.
        assert_eq!(fnv1a(""), 0x811c_9dc5);
        assert_eq!(fnv1a("a"), 0xe40c_292c);
    }

    #[test]
    fn session_names_a_session_keeps_its_name() {
        let mut registry = Registry::default();
        let first = registry.name_for("ws", "s1");
        registry.name_for("ws", "s2");
        assert_eq!(registry.name_for("ws", "s1"), first);
    }

    #[test]
    fn session_names_three_sessions_get_three_distinct_two_word_names() {
        let mut registry = Registry::default();
        let names: HashSet<String> =
            ["a", "b", "c"].iter().map(|id| registry.name_for("ws", id)).collect();
        assert_eq!(names.len(), 3);
        assert!(names.iter().all(|name| is_two_words(name)), "{names:?}");
    }

    #[test]
    fn session_names_three_hundred_sessions_in_one_workspace_are_unique() {
        let mut registry = Registry::default();
        let names: Vec<String> =
            (0..300).map(|i| registry.name_for("ws", &format!("session-{i}"))).collect();
        let unique: HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), 300);
        for name in &names {
            let bare = name
                .rsplit_once('-')
                .filter(|(_, n)| n.parse::<u32>().is_ok_and(|n| n >= 2))
                .map(|(bare, _)| bare);
            assert!(is_two_words(bare.unwrap_or(name)), "{name}");
        }
    }

    #[test]
    fn session_names_collision_gets_suffix_two_and_workspaces_are_independent() {
        let mut seen: HashMap<String, String> = HashMap::new();
        let (first, second) = (0..)
            .map(|i| format!("id-{i}"))
            .find_map(|id| seen.insert(base_name(&id), id.clone()).map(|previous| (previous, id)))
            .expect("a collision exists in a finite name space");
        let mut registry = Registry::default();
        let bare = registry.name_for("ws", &first);
        assert_eq!(registry.name_for("ws", &second), format!("{bare}-2"));
        assert_eq!(registry.name_for("other", &second), bare);
    }

    #[test]
    fn session_names_retain_frees_names_of_gone_sessions_only() {
        let mut registry = Registry::default();
        let kept = registry.name_for("ws", "keep");
        registry.name_for("ws", "gone");
        registry.retain(&HashSet::from(["keep"]));
        assert_eq!(registry.by_workspace["ws"].len(), 1);
        assert_eq!(registry.name_for("ws", "keep"), kept);
    }
}
