//! Integrator may register this file as the ferryx-remote-helper Cargo bin path.
#[path = "../../dag/journal.rs"]
pub mod dag_journal;
#[path = "../../worktree/naming.rs"]
pub mod worktree_naming;
#[path = "../../scoped_contracts.rs"]
pub mod scoped_contracts;
#[path = "mod.rs"]
pub mod ssh;
fn main() {
    if let Err(error) = ssh::process::run(std::env::args().skip(1)) { eprintln!("{error}"); std::process::exit(1); }
}
