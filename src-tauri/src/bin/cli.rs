use ferryx_lib::cli::{
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
    if args.get(1).is_some_and(|arg| arg == "agents") {
        std::process::exit(ferryx_lib::agent_control::run_agents_cli(&args));
    }
    if args.get(1).is_some_and(|arg| arg == "supervisor") {
        std::process::exit(ferryx_lib::cli::supervisor_goal::run_supervisor_cli(&args));
    }
    if args.get(1).is_some_and(|arg| arg == "hub") {
        std::process::exit(ferryx_lib::hub_runtime::run_hub_cli(&args));
    }
    if args.get(1).is_some_and(|arg| arg == "mcp") {
        std::process::exit(ferryx_lib::hub_runtime::run_mcp());
    }
    if args.get(1).is_some_and(|arg| arg == "open") {
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
    if args.get(1).is_some_and(|arg| arg == "up") {
        std::process::exit(ferryx_lib::cli::up_cli_main(&args));
    }
    if args.get(1).is_some_and(|arg| arg == "service") {
        std::process::exit(ferryx_lib::cli::service_cli_main(&args));
    }
    if args.get(1).is_some_and(|arg| arg == "account") {
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
    if args.get(1).is_some_and(|arg| arg == "browser") {
        match parse_browser_cli(&args).and_then(run_browser_cli) {
            Ok(()) => return,
            Err(error) => {
                print_browser_cli_error("BROWSER_CLI_INVALID_OR_UNAVAILABLE", error);
                std::process::exit(2);
            }
        }
    }
    if args.get(1).is_some_and(|arg| arg == "pair") {
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
    if args.get(1).is_some_and(|arg| arg == "direct-trust") {
        match parse_direct_trust_cli(&args).and_then(run_direct_trust_cli) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    if args.get(1).is_some_and(|arg| arg == "remote") {
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
            eprintln!("Ferryx CLI is running in headless mode.\nUsage: ferryx-cli <up [--key-stdin|--key-env] [--email <hint>] [--origin <url>] [--name <name>] [--no-service] [--json]|service <install|status|uninstall> [--json]|agents|supervisor goal <set|show|mode|approve|discard|accept-revision|reject-revision>|mcp|hub migrate --from-omo <db>|account <enroll|login>|pair|remote|direct-trust|browser|--daemon>");
            std::process::exit(1);
        }
        LaunchMode::DaemonSupervisor => std::process::exit(run_daemon_supervisor(&args)),
    }
}
