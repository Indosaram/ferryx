"""P13 owned source-seam runner. No Cargo, GUI, daemon, or environment mutation."""
from pathlib import Path
import hashlib
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent
DEPS = ROOT / "src-tauri/target/debug/deps"
IPC = ROOT / "src-tauri/src/ipc/browser.rs"
MANAGER = ROOT / "src-tauri/src/browser/manager.rs"
TESTS = ROOT / "src-tauri/src/browser/tests.rs"


def between(text, start, end):
    offset = text.index(start)
    return text[offset:text.index(end, offset)]


def run(name, source, crates=()):
    with tempfile.TemporaryDirectory(prefix="p13-" + name + "-") as directory:
        path = Path(directory) / "seam.rs"
        path.write_text(source)
        binary = Path(directory) / "seam"
        args = ["rustc", "--edition=2021", "--test", str(path), "-o", str(binary)]
        if crates:
            args += ["-L", "dependency=" + str(DEPS)]
        for crate in crates:
            library = DEPS / "libferryx_lib.rlib" if crate == "ferryx_lib" else max(
                DEPS.glob("lib" + crate + "-*.rlib"), key=lambda item: item.stat().st_mtime
            )
            args += ["--extern", crate + "=" + str(library)]
        compiled = subprocess.run(args, capture_output=True, text=True, timeout=90)
        log = "source SHA256 " + hashlib.sha256(source.encode()).hexdigest() + "\n"
        log += "compile argv " + repr(args) + "\n" + compiled.stdout + compiled.stderr
        if compiled.returncode:
            (EVIDENCE / ("p13-" + name + "-compile-blocked.log")).write_text(log)
            raise RuntimeError(log)
        result = subprocess.run([str(binary), "--nocapture"], capture_output=True, text=True, timeout=30)
        log += result.stdout + result.stderr + "\nexit=" + str(result.returncode)
        log += "\nTemporaryDirectory binaries/source automatically removed.\n"
        (EVIDENCE / ("p13-" + name + "-green.log")).write_text(log)
        print(log)
        if result.returncode:
            raise RuntimeError(name + " assertions failed")


ipc = IPC.read_text()
requests = between(ipc, "#[derive(Debug, PartialEq, Eq)]\nenum WindowsOpenRequest", '\n#[cfg(target_os = "windows")]\nfn open_windows_target')
resolver = between(ipc, "fn resolve_file_link(", "\n#[tauri::command]")
seam_tests = between(ipc, "    #[test]\n    fn external_open_preserves", "    #[test]\n    fn keep_or_discard_fresh_webview")
run("opener-home", requests + "\n" + resolver + "\n" + seam_tests)
status = between(ipc, "fn shell_execute_result(", "\nfn open_system_target")
status_test = between(ipc, "    #[test]\n    fn external_open_propagates_failure", "    #[test]\n    fn windows_keypress_returns_typed_unsupported")
run("opener-error", status + "\nmod tests {\n" + status_test + "}\n")

keypress = between(ipc, "fn windows_keypress_capability()", "\npub async fn browser_automation_act")
key_test = between(ipc, "    #[test]\n    fn windows_keypress_returns_typed_unsupported", "    #[test]\n    fn external_open_preserves")
run("key", "extern crate ferryx_lib; pub use ferryx_lib::ipc; use ipc::error::IpcError;\n" + keypress + "\nmod tests {\n" + key_test + "}\n", ("ferryx_lib",))

history_test = between(TESTS.read_text(), "#[test]\nfn native_history_invalidates", "#[test]\nfn test_history_navigation_marks")
run("history", 'extern crate ferryx_lib; mod browser { pub use ferryx_lib::browser::{model,security}; }\n#[path="' + str(MANAGER) + '"] mod manager; use manager::BrowserManager; use ferryx_lib::browser::{CreateBrowserRequest,BrowserAutomationTarget,BrowserError};\n' + history_test, ("ferryx_lib", "parking_lot", "uuid"))
