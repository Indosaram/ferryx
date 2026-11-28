//! `ferryx <host> <command> ...` — the paired host as the first token.
//!
//! This module owns one contract: which groups can reach another machine, and how each
//! spells the host once the prefix is removed. The daemon resolves the token to a paired
//! host; nothing here does.
use crate::cli::api_catalog::CLI_GROUPS;

pub const EXIT_REFUSED: i32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRefusal {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSpelling {
    /// `--host <id>`, inserted right after the group name.
    Flag,
    /// Named inside the group's own arguments (`file cp <host>:<path>`).
    Inline,
    Unsupported,
}

pub fn spelling(group: &str) -> HostSpelling {
    match group {
        "agents" => HostSpelling::Flag,
        "supervisor" => HostSpelling::Flag,
        "file" => HostSpelling::Inline,
        _ => HostSpelling::Unsupported,
    }
}

pub fn host_capable_groups() -> Vec<&'static str> {
    CLI_GROUPS
        .iter()
        .copied()
        .filter(|group| spelling(group) != HostSpelling::Unsupported)
        .collect()
}

/// A prefix is recognised only when the first token is not a known group and the second is.
/// That rule is what keeps every existing command working: before it, the first token was
/// always a group or a launch mode.
pub fn split_host_prefix(args: &[String]) -> Option<(String, Vec<String>)> {
    let host = args.get(1)?;
    let group = args.get(2)?;
    if host.is_empty() || host.starts_with('-') {
        return None;
    }
    if CLI_GROUPS.contains(&host.as_str()) || !CLI_GROUPS.contains(&group.as_str()) {
        return None;
    }
    let mut argv = Vec::with_capacity(args.len() - 1);
    argv.push(args[0].clone());
    argv.extend(args[2..].iter().cloned());
    Some((host.clone(), argv))
}

/// Returns the argv unchanged when there is no host prefix, so callers can run it
/// unconditionally. A group that cannot reach a host is refused rather than being allowed
/// to act on this machine under a host name.
pub fn apply_host_prefix(args: &[String]) -> Result<Vec<String>, HostRefusal> {
    let Some((host, argv)) = split_host_prefix(args) else {
        return Ok(args.to_vec());
    };
    let group = argv[1].as_str();
    match spelling(group) {
        HostSpelling::Flag => {
            let mut out = Vec::with_capacity(argv.len() + 2);
            out.push(argv[0].clone());
            out.push(argv[1].clone());
            out.push("--host".to_string());
            out.push(host);
            out.extend(argv[2..].iter().cloned());
            Ok(out)
        }
        HostSpelling::Inline => Ok(argv),
        HostSpelling::Unsupported => Err(HostRefusal {
            code: "TARGET_UNSUPPORTED",
            message: format!(
                "`{group}` cannot run against another host yet; host-capable groups are: {}. \
                 Run `{group}` here without a host, or use `ferryx-cli file cp {host}:<path> <path>` \
                 to move a file",
                host_capable_groups().join(", ")
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn a_known_group_stays_a_group() {
        let args = argv(&["ferryx-cli", "browser", "snapshot", "--browser-id", "b1"]);
        assert_eq!(split_host_prefix(&args), None);
        assert_eq!(apply_host_prefix(&args).unwrap(), args);
    }

    #[test]
    fn a_host_before_a_group_is_the_host() {
        let (host, argv) = split_host_prefix(&argv(&[
            "ferryx-cli", "air", "agents", "list", "--json",
        ]))
        .expect("host prefix");
        assert_eq!(host, "air");
        assert_eq!(argv, argv(&["ferryx-cli", "agents", "list", "--json"]));
    }

    #[test]
    fn a_group_name_is_never_read_as_a_host() {
        assert_eq!(
            split_host_prefix(&argv(&["ferryx-cli", "browser", "browser"])),
            None
        );
    }

    #[test]
    fn a_host_without_a_group_is_not_a_prefix() {
        assert_eq!(split_host_prefix(&argv(&["ferryx-cli", "air"])), None);
        assert_eq!(
            split_host_prefix(&argv(&["ferryx-cli", "air", "not-a-group"])),
            None
        );
    }

    #[test]
    fn a_flag_first_is_not_a_host() {
        assert_eq!(
            split_host_prefix(&argv(&["ferryx-cli", "--version", "agents"])),
            None
        );
    }

    #[test]
    fn the_host_flag_is_inserted_before_the_trailing_text() {
        let out = apply_host_prefix(&argv(&[
            "ferryx-cli", "air", "agents", "send", "s-1", "hello", "world",
        ]))
        .unwrap();
        assert_eq!(
            out,
            argv(&[
                "ferryx-cli", "agents", "--host", "air", "send", "s-1", "hello", "world",
            ])
        );
    }

    #[test]
    fn a_group_that_cannot_reach_a_host_is_refused() {
        let refusal = apply_host_prefix(&argv(&[
            "ferryx-cli", "air", "browser", "snapshot", "--browser-id", "b1",
        ]))
        .expect_err("browser has no host path yet");
        assert_eq!(refusal.code, "TARGET_UNSUPPORTED");
        assert!(refusal.message.contains("browser"));
        assert!(refusal.message.contains("agents"));
    }

    #[test]
    fn file_keeps_its_own_host_spelling() {
        let args = argv(&["ferryx-cli", "air", "file", "cp", "a", "b"]);
        assert_eq!(apply_host_prefix(&args).unwrap(), args);
    }
}
