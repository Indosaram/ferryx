use ferryx_lib::cli::{
    api_catalog::{run_api_cli, CLI_GROUPS},
    parse_account_cli, parse_browser_cli, parse_direct_trust_cli, parse_handover_from,
    parse_launch_mode, parse_open_cli, parse_pair_cli, parse_remote_cli, print_browser_cli_error,
    run_account_cli, run_browser_cli, run_daemon_headless, run_daemon_supervisor,
    run_direct_trust_cli, run_open_cli, run_pair_cli, run_remote_cli, AccountCliError, LaunchMode,
};

fn main() {
    #[cfg(unix)]
    if let Some(code) = ferryx_lib::ssh::transport_unix::run_supervisor_mode() {
        std::process::exit(code);
    }
    if let Some(code) = ferryx_lib::ssh::password::run_askpass() {
        std::process::exit(code);
    }
    let args: Vec<String> = std::env::args().collect();
    if args
        .get(1)
        .is_some_and(|arg| matches!(arg.as_str(), "--version" | "-V" | "version"))
    {
        println!("{}", ferryx_lib::cli::cli_version_line());
        return;
    }
    // Only the groups named in CLI_GROUPS dispatch here; everything else falls through to
    // the launch-mode handling below.
    let group = args.get(1).map(String::as_str).filter(|arg| CLI_GROUPS.contains(arg));
    if group == Some("api") {
        std::process::exit(run_api_cli(&args));
    }
    // Before the generic `supervisor` dispatch: `supervisor engine` has its own parser.
    if group == Some("supervisor") && args.get(2).is_some_and(|arg| arg == "engine") {
        std::process::exit(ferryx_lib::cli::supervisor_engine::run_supervisor_engine_cli(&args));
    }
    if group == Some("agents") {
        std::process::exit(ferryx_lib::agent_control::run_agents_cli(&args));
    }
    if group == Some("supervisor") {
        std::process::exit(ferryx_lib::cli::supervisor_goal::run_supervisor_cli(&args));
    }
    if group == Some("hub") {
        std::process::exit(ferryx_lib::hub_runtime::run_hub_cli(&args));
    }
    if group == Some("mcp") {
        std::process::exit(ferryx_lib::hub_runtime::run_mcp());
    }
    if group == Some("open") {
        let outcome = std::env::current_dir()
            .map_err(|e| format!("failed to determine current directory: {e}"))
            .and_then(|cwd| parse_open_cli(&args, &cwd))
            .and_then(run_open_cli);
        match outcome {
            Ok(()) => return,
            Err(error) => {
                print_browser_cli_error("OPEN_CLI_FAILED", error);
                std::process::exit(2);
            }
        }
    }
    if group == Some("up") {
        std::process::exit(ferryx_lib::cli::up_cli_main(&args));
    }
    if group == Some("service") {
        std::process::exit(ferryx_lib::cli::service_cli_main(&args));
    }
    if group == Some("account") {
        let outcome = parse_account_cli(&args)
            .map_err(AccountCliError::usage)
            .and_then(run_account_cli);
        match outcome {
            Ok(()) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(error.exit_code);
            }
        }
    }
    if group == Some("browser") {
        let outcome = match ferryx_lib::cli::parse_browser_controllers_cli(&args) {
            Some(parsed) => parsed.and_then(ferryx_lib::cli::run_browser_controllers_cli),
            None => parse_browser_cli(&args).and_then(run_browser_cli),
        };
        match outcome {
            Ok(()) => return,
            Err(error) => {
                print_browser_cli_error("BROWSER_CLI_INVALID_OR_UNAVAILABLE", error);
                std::process::exit(2);
            }
        }
    }
    if group == Some("pair") {
        match parse_pair_cli(&args).and_then(run_pair_cli) {
            Ok(ferryx_lib::cli::PairCliOutcome::Done) => return,
            Ok(ferryx_lib::cli::PairCliOutcome::AccountLoginRequired) => {
                eprintln!(
                    "ACCOUNT_LOGIN_REQUIRED: PIN issuance was retired. Sign in to the Ferryx account on the \
                     desktop, issue an enrollment code, and run `ferryx-cli account enroll --code <code>` \
                     on this machine."
                );
                std::process::exit(2);
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    if group == Some("direct-trust") {
        match parse_direct_trust_cli(&args).and_then(run_direct_trust_cli) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    if group == Some("remote") {
        match parse_remote_cli(&args).and_then(run_remote_cli) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    match parse_launch_mode(&args) {
        LaunchMode::Daemon => {
            let handover_from = match parse_handover_from(&args) {
                Ok(value) => value,
                Err(e) => {
                    eprintln!("Ferryx daemon error: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = run_daemon_headless(handover_from) {
                eprintln!("Ferryx daemon error: {e}");
                std::process::exit(1);
            }
        }
        LaunchMode::Gui => {
            eprintln!("Ferryx CLI is running in headless mode.\nUsage: ferryx-cli <up [--key-stdin|--key-env] [--email <hint>] [--origin <url>] [--name <name>] [--no-service] [--json]|service <install|status|uninstall> [--json]|agents|supervisor goal <set|show|mode|approve|discard|accept-revision|reject-revision>|supervisor setup --owner <p> --project <name> --repo-identity <id> --host <id> --git-common-dir <dir>|mcp|hub migrate --from-omo <db>|account <enroll|login>|pair|remote|direct-trust|browser|api <list|describe <command>>|--daemon>");
            std::process::exit(1);
        }
        LaunchMode::DaemonSupervisor => std::process::exit(run_daemon_supervisor(&args)),
    }
}
