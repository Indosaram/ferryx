use super::{store::{self, Shared, Store}, vm::Vm};
use std::{cell::{Cell, RefCell}, rc::Rc, time::{Duration, Instant}};

fn vm() -> Vm { Vm::new("test".into(), Rc::new(|| 0), |_, _| Ok(())).unwrap() }

#[test]
fn sandbox_denies_host_libraries_and_dynamic_loading() {
    for source in ["io.open('secret')", "os.execute('true')", "package.loadlib('x')",
        "debug.info(1)", "load('return 1')", "loadstring('return 1')", "require('x')"] {
        assert!(vm().load(source).is_err(), "unexpectedly accepted {source}");
    }
}

#[test]
fn infinite_loop_interrupts_under_two_hundred_milliseconds() {
    let vm = vm();
    let start = Instant::now();
    let error = vm.load("while true do end").unwrap_err();
    assert!(error.to_string().contains("SCRIPT_INTERRUPTED"));
    assert!(start.elapsed() < Duration::from_millis(200));
}

#[test]
fn memory_limit_is_enforced() {
    let error = vm().load("local x = string.rep('x', 70 * 1024 * 1024)").unwrap_err();
    assert!(matches!(error, mlua::Error::MemoryError(_)), "{error}");
}

#[test]
fn sixty_fifth_timer_is_refused_and_failed_initialization_clears_timers() {
    let vm = vm();
    vm.load("for i = 1, 64 do ferryx.after(100, function() end) end").unwrap();
    assert_eq!(vm.timers.borrow().len(), 64);
    assert!(vm.load("ferryx.after(100, function() end)").unwrap_err().to_string().contains("SCRIPT_TIMER_LIMIT"));
    assert!(vm.timers.borrow().is_empty());
}

#[test]
fn disabled_setting_does_not_read_or_load_script() {
    let dir = tempfile::tempdir().unwrap();
    let init = dir.path().join("init.luau");
    std::fs::write(&init, "while true do end").unwrap();
    assert!(!store::enabled(dir.path()).unwrap());
    assert!(store::source(false, &init).unwrap().is_none());
    store::write(&dir.path().join("settings.json"), &serde_json::json!({"scripting":{"enabled":true}})).unwrap();
    assert!(store::enabled(dir.path()).unwrap());
    assert!(store::source(true, &init).unwrap().is_some());
    assert!(store::source(true, &dir.path().join("missing")).unwrap().is_none());
}

#[test]
fn handover_reload_and_windows_restart_deduplicate_with_fake_clock() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Shared::load(dir.path()).unwrap();
    let clock = Rc::new(Cell::new(0_u64));
    let logs = Rc::new(RefCell::new(Vec::new()));
    let make_vm = || {
        let clock = clock.clone();
        let logs = logs.clone();
        Vm::new("same-script".into(), Rc::new(move || clock.get()), move |message, _| {
            logs.borrow_mut().push(message); Ok(())
        }).unwrap()
    };
    let first = make_vm();
    first.load("ferryx.after(100, function() ferryx.log('tick') end)").unwrap();
    // Candidate loading overlaps the predecessor, but execution waits until ownership.
    clock.set(50);
    let successor = make_vm();
    successor.load("ferryx.after(100, function() ferryx.log('tick') end)").unwrap();
    let host = tokio::runtime::Runtime::new().unwrap();
    clock.set(100);
    first.fire(&registry, host.handle()).unwrap();
    let ledger = registry.freeze();
    let adopted = Shared::load(dir.path()).unwrap();
    adopted.store.lock().adopt(ledger).unwrap();
    clock.set(150);
    successor.fire(&adopted, host.handle()).unwrap();
    assert_eq!(&*logs.borrow(), &["tick"]);
    let mut restarted = Store::load(dir.path()).unwrap();
    assert!(!restarted.claim("same-script/0", 100, 199).unwrap());
    assert!(restarted.claim("same-script/0", 100, 200).unwrap());
}

#[cfg(unix)]
#[test]
fn committed_decision_carries_ledger_without_killing_predecessor() {
    use crate::daemon::handover::HandoverManager;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("legacy.sock");
    let manager = HandoverManager::new(root.path().join("daemon.sock"));
    manager.force_prepared_for_test(legacy.clone());
    let retired = Arc::new(AtomicBool::new(false));
    let signal = retired.clone();
    manager.set_retirement_action(move || { signal.store(true, Ordering::SeqCst); });
    let registry = Shared::load(root.path()).unwrap();
    assert!(registry.store.lock().claim("timer", 100, 1000).unwrap());
    manager.set_script(registry.clone());
    let terminals = crate::terminal::service::TerminalService::new();
    manager.commit_handover_v5(&terminals).unwrap();
    let candidate = HandoverManager::new(root.path().join("next.sock"));
    let other = tempfile::tempdir().unwrap();
    let adopted = Shared::load(other.path()).unwrap();
    candidate.set_script(adopted.clone());
    candidate.adopt_recorded_script(&legacy).unwrap();
    assert!(!adopted.store.lock().claim("timer", 100, 1050).unwrap());
    assert!(registry.store.lock().paused);
    assert!(!retired.load(Ordering::SeqCst), "handover must not kill predecessor");
}
