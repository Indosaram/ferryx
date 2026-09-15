#[path = "../../../src-tauri/src/ipc/windows_process_cwd.rs"]
mod windows_process_cwd;

fn main() {
    assert_eq!(windows_process_cwd::process_cwd(std::process::id()), std::env::current_dir().ok());
}
