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
    Unsupported,
}

pub fn spelling(group: &str) -> HostSpelling {
    match group {
        "agents" | "supervisor" | "browser" | "capabilities" => HostSpelling::Flag,
        _ => HostSpelling::Unsupported,
    }
}

/// Removes `--host <id>` from an argument list, returning the id it named.
///
/// A browser command is the one place where the flag has to be taken back out: its parser
/// rejects unknown options, so the id is read here and the request is routed by the caller.
pub fn take_host_flag(args: &mut Vec<String>) -> Option<String> {
    let index = args.iter().position(|arg| arg == "--host")?;
    let value = args.get(index + 1)?.clone();
    args.drain(index..=index + 1);
    Some(value)
}

/// Groups whose host is named inline, inside an argument, rather than by a prefix. A prefix
/// cannot be applied to them: `file cp <src> <dst>` takes `<host>:<path>` on whichever side
/// is remote, so the prefix would have to guess the side. They are refused, and the refusal
/// teaches the inline form instead of silently dropping the host and running here.
fn inline_host_hint(group: &str) -> Option<&'static str> {
    match group {
        "file" => Some("`file` names its host inside the path: `ferryx-cli file cp <host>:<path> <path>`"),
        _ => None,
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
            out.push(host.clone());
            out.extend(argv[2..].iter().cloned());
            Ok(out)
        }
        HostSpelling::Unsupported => {
            let hint = inline_host_hint(group)
                .map(|hint| format!(" {hint}."))
                .unwrap_or_default();
            Err(HostRefusal {
                code: "TARGET_UNSUPPORTED",
                message: format!(
                    "`{group}` cannot run against host `{host}` yet; host-capable groups are: {}.{hint} \
                     Run `{group}` here without a host to act on this machine",
                    host_capable_groups().join(", ")
                ),
            })
        }
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
        let (host, rest) = split_host_prefix(&argv(&[
            "ferryx-cli", "air", "agents", "list", "--json",
        ]))
        .expect("host prefix");
        assert_eq!(host, "air");
        assert_eq!(rest, argv(&["ferryx-cli", "agents", "list", "--json"]));
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
    fn browser_takes_the_host_flag_and_gives_it_back() {
        let out = apply_host_prefix(&argv(&[
            "ferryx-cli",
            "air",
            "browser",
            "snapshot",
            "--browser-id",
            "b1",
        ]))
        .expect("browser reaches a host");
        assert_eq!(
            out,
            argv(&[
                "ferryx-cli",
                "browser",
                "--host",
                "air",
                "snapshot",
                "--browser-id",
                "b1",
            ])
        );
        let mut rest = out;
        assert_eq!(take_host_flag(&mut rest).as_deref(), Some("air"));
        assert_eq!(
            rest,
            argv(&["ferryx-cli", "browser", "snapshot", "--browser-id", "b1"]),
            "the parser sees no --host option"
        );
    }

    #[test]
    fn a_group_that_cannot_reach_a_host_is_refused() {
        let refusal = apply_host_prefix(&argv(&[
            "ferryx-cli", "air", "mcp", "--something",
        ]))
        .expect_err("mcp has no host path");
        assert_eq!(refusal.code, "TARGET_UNSUPPORTED");
        assert!(refusal.message.contains("mcp"));
        assert!(refusal.message.contains("agents"));
    }

    #[test]
    fn file_is_refused_with_its_inline_spelling() {
        let refusal = apply_host_prefix(&argv(&["ferryx-cli", "air", "file", "cp", "a", "b"]))
            .expect_err("a prefix cannot say which side of a copy is remote");
        assert_eq!(refusal.code, "TARGET_UNSUPPORTED");
        assert!(refusal.message.contains("air"), "the refusal names the host it refused");
        assert!(
            refusal.message.contains("<host>:<path>"),
            "the refusal teaches the inline form: {}",
            refusal.message
        );
    }

    #[test]
    fn no_group_silently_drops_a_host() {
        // A prefix that is accepted must put the host somewhere. Anything that cannot is
        // refused, because dropping it would run the command on this machine instead.
        for group in CLI_GROUPS {
            let args = argv(&["ferryx-cli", "air", group, "sub"]);
            match apply_host_prefix(&args) {
                Ok(out) => {
                    // `browser` hands the flag to a parser that rejects unknown options, so the
                    // caller takes it back out; every other accepted group keeps it in argv.
                    let mut out = out;
                    let taken = take_host_flag(&mut out);
                    assert!(
                        taken.as_deref() == Some("air")
                            || out.iter().any(|token| token == "air"),
                        "{group} accepted the prefix but lost the host: {out:?}"
                    );
                }
                Err(refusal) => assert_eq!(refusal.code, "TARGET_UNSUPPORTED"),
            }
        }
    }
}
