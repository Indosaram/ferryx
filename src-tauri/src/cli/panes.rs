//! Detached session inventory and daemon-owned layout operations.
use crate::daemon::layout::{Direction, PaneAction};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};

pub fn parse(args: &[String]) -> Result<PaneAction, String> {
    let mut positional = Vec::new();
    let mut window = None;
    let mut split = None;
    let mut rest = args.iter().skip(3);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => {},
            "--window" => window = Some(rest.next().ok_or("--window requires a value")?.clone()),
            "--split" => split = Some(match rest.next().map(String::as_str) {
                Some("right") => Direction::Horizontal,
                Some("down") => Direction::Vertical,
                _ => return Err("--split expects right or down".into()),
            }),
            value if value.starts_with('-') => return Err(format!("Unknown flag {value}")),
            _ => positional.push(arg.clone()),
        }
    }
    let verb = args.get(2).map(String::as_str);
    if verb != Some("attach") && (window.is_some() || split.is_some()) { return Err("Target flags require attach".into()); }
    match (verb, positional.as_slice()) {
        (Some("list"), []) => Ok(PaneAction::List),
        (Some("detach"), [session]) => Ok(PaneAction::Detach { session: session.clone() }),
        (Some("attach"), [session]) => Ok(PaneAction::Attach { session: session.clone(), window, split }),
        (Some("zoom"), [session]) => Ok(PaneAction::Zoom { session: session.clone() }),
        (Some("swap"), [a, b]) => Ok(PaneAction::Swap { a: a.clone(), b: b.clone() }),
        _ => Err("usage: ferryx-cli panes list|detach <session>|attach <session> [--window W] [--split right|down]|zoom <session>|swap <a> <b> [--json]".into()),
    }
}

pub fn run(args: &[String]) -> i32 {
    let (request, capability, mutates) = if args.iter().any(|arg| arg == "--plugin") {
        match parse_plugin(args) {
            Ok(request) => (request, "blockPluginsV1", true),
            Err(error) => { eprintln!("{error}"); return 2; },
        }
    } else {
        let action = match parse(args) { Ok(action) => action, Err(error) => { eprintln!("{error}"); return 2; } };
        let changes_layout = action.mutates();
        (DaemonRequest::Panes { request: action }, "detachedPanesV1", changes_layout)
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => { eprintln!("{error}"); return 1; },
    };
    let result = runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), super::supervisor_goal::exchange_once(
            capability, request, mutates,
        )).await
    });
    match result {
        Ok(Ok(DaemonResponse::PanesOk { detached })) => { println!("{}", serde_json::json!(detached)); 0 },
        Ok(Ok(DaemonResponse::LayoutOk { view })) => { println!("{}", serde_json::json!(view)); 0 },
        Ok(Ok(DaemonResponse::Error { message, code, .. })) => { eprintln!("{}: {message}", code.unwrap_or_else(|| "INTERNAL".into())); 1 },
        Ok(Ok(response)) => { eprintln!("Unexpected response: {response:?}"); 1 },
        Ok(Err((value, exit))) => { println!("{value}"); exit },
        Err(error) => { eprintln!("TIMEOUT: {error}"); 1 },
    }
}

fn parse_plugin(args: &[String]) -> Result<DaemonRequest, String> {
    if args.get(2).map(String::as_str) != Some("attach") { return Err("--plugin requires attach".into()); }
    let mut plugin = None;
    let mut window = None;
    let mut rest = args.iter().skip(3);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--plugin" if plugin.is_none() => plugin = Some(rest.next().ok_or("--plugin requires a name")?.clone()),
            "--window" if window.is_none() => window = Some(rest.next().ok_or("--window requires a label")?.clone()),
            "--json" => {},
            _ => return Err(format!("Unexpected plugin attach argument: {arg}")),
        }
    }
    let plugin = plugin.ok_or("--plugin requires a name")?;
    if plugin.is_empty() || !plugin.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err("Invalid plugin name".into());
    }
    Ok(DaemonRequest::PluginAttach { plugin, window })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn argv(tail: &[&str]) -> Vec<String> { ["ferryx-cli", "panes"].into_iter().chain(tail.iter().copied()).map(String::from).collect() }
    #[test]
    fn plugin_attach_arguments_are_strict() {
        assert!(matches!(parse_plugin(&argv(&["attach", "--plugin", "clock"])),
            Ok(DaemonRequest::PluginAttach { plugin, window: None }) if plugin == "clock"));
        for tail in [vec!["attach", "--plugin"], vec!["list", "--plugin", "clock"],
            vec!["attach", "s", "--plugin", "clock"], vec!["attach", "--plugin", "../clock"]] {
            assert!(parse_plugin(&argv(&tail)).is_err());
        }
    }
    #[test]
    fn detached_cli_verbs_and_strict_arguments() {
        for tail in [vec!["list", "--json"], vec!["detach", "a"], vec!["zoom", "a"], vec!["swap", "a", "b"], vec!["attach", "a", "--window", "second", "--split", "down"]] {
            assert!(parse(&argv(&tail)).is_ok());
        }
        for tail in [vec!["attach"], vec!["list", "extra"], vec!["attach", "a", "--split", "left"], vec!["zoom", "a", "--window", "main"]] {
            assert!(parse(&argv(&tail)).is_err());
        }
    }
}
