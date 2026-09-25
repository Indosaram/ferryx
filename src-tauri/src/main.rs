// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub use ferryx_lib::cli::*;

fn main() {
    // Both launch modes need this before anything opens a file: launchd gives an app a soft
    // descriptor limit of 256, and a terminal daemon spends roughly three descriptors per live
    // session, so the inherited default is exhausted after a few dozen sessions and the next
    // spawn fails with EMFILE. Raising it here also propagates to the daemon the GUI spawns.
    let raised_limit = ferryx_lib::raise_file_descriptor_limit();
    if let Some(code) = ferryx_lib::ssh::password::run_askpass() {
        std::process::exit(code);
    }
    let args: Vec<String> = std::env::args().collect();
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
            Ok(PairCliOutcome::Done) => return,
            Ok(PairCliOutcome::AccountLoginRequired) => {
                eprintln!(
                    "ACCOUNT_LOGIN_REQUIRED: PIN issuance was retired. Sign in to the Ferryx account on the \
                     desktop, issue an enrollment code, and run `ferryx account enroll --code <code>` on \
                     the machine that should join."
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
            let _ = raised_limit;
            ferryx_lib::run();
        }
    }
}
