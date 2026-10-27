//! The supervisor pane is the view. The headless engine keeps running when the pane is closed.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPanePreset {
    pub title: String,
    pub cwd: PathBuf,
    pub command: Vec<String>,
}

impl SupervisorPanePreset {
    pub fn from_parts(cwd: PathBuf, command: Vec<String>) -> Self {
        Self {
            title: "supervisor".into(),
            cwd,
            command: if command.is_empty() {
                vec![super::supervisor::mhc_bin()]
            } else {
                command
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preset_is_named_supervisor() {
        let preset = SupervisorPanePreset::from_parts(
            PathBuf::from("/tmp"),
            vec!["mhc".into()],
        );
        assert_eq!(preset.title, "supervisor");
        assert_eq!(preset.command[0], "mhc");
    }
}
