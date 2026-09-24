use ferryx_lib::cli::{
    parse_account_cli, parse_browser_cli, parse_handover_from, parse_launch_mode, parse_pair_cli,
    parse_remote_cli, print_browser_cli_error, run_account_cli, run_browser_cli,
    run_daemon_headless, run_pair_cli, run_remote_cli, LaunchMode,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "account") {
        match parse_account_cli(&args).and_then(run_account_cli) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
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
            eprintln!("Ferryx CLI is running in headless mode.\nUsage: ferryx-cli <account <enroll|login>|pair|remote|browser|--daemon>");
            std::process::exit(1);
        }
    }
}
