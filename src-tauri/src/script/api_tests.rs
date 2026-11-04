use super::{api, vm::Vm};
use std::{cell::RefCell, rc::Rc};

fn sandbox() -> (Vm, Rc<RefCell<Vec<String>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let effects = calls.clone();
    let vm = Vm::with_host("api-test".into(), Rc::new(|| 0), |_, _| Ok(()), Rc::new(move |name, value| {
        effects.borrow_mut().push(format!("{name}:{value}"));
        Ok(serde_json::json!("result"))
    })).unwrap();
    (vm, calls)
}

#[test]
fn script_api_each_host_function_accepts_valid_arguments() {
    let (vm, calls) = sandbox();
    vm.load(r#"
        ferryx.split('session', 'right')
        ferryx.send('session', 'hello\n')
        ferryx.read('session', 20)
        ferryx.notify('title', 'body')
        ferryx.layout('main', 0, {type='ACTIVATE_TAB', tabId='tab'})
        ferryx.bind('cmd+shift+a', function(session)
            ferryx.exec({'echo', 'literal;not-a-shell'})
        end, {override=true})
    "#).unwrap();
    vm.key("shift+cmd+a", "session").unwrap();
    let names: Vec<_> = calls.borrow().iter().map(|s| s.split(':').next().unwrap().to_owned()).collect();
    assert_eq!(names, ["split", "send", "read", "notify", "layout", "exec"]);
    assert!(vm.api.bindings.borrow()["cmd+shift+a"].0);
    assert!(calls.borrow().last().unwrap().contains("literal;not-a-shell"));
}

#[test]
fn script_api_rejects_bad_arguments_before_host_effects() {
    for source in [
        "ferryx.exec('sh -c rm')", "ferryx.exec({})", "ferryx.exec({42})",
        "ferryx.exec({'echo', extra='not argv'})", "ferryx.split('s', 'diagonal')",
        "ferryx.send('s', {})", "ferryx.read('s', 0)", "ferryx.read('s', 10001)",
        "ferryx.notify({}, 'body')", "ferryx.layout('main', -1, {})",
        "ferryx.layout('main', 0, {type='NOT_AN_INTENT'})", "ferryx.on('unknown', function() end)",
        "ferryx.bind('hyper+a', function() end)", "ferryx.every(0, function() end)",
    ] {
        let (vm, calls) = sandbox();
        let error = vm.load(source).unwrap_err();
        assert!(error.to_string().contains("BAD_ARGUMENT"), "{source}: {error}");
        assert!(calls.borrow().is_empty());
    }
}

#[test]
fn script_api_events_deliver_payloads_and_keys_default_to_non_override() {
    let (vm, calls) = sandbox();
    vm.load(r#"
        for _, event in {'agent.state', 'agent.done', 'session.exit', 'layout.changed'} do
            ferryx.on(event, function(payload) ferryx.send(payload.sessionId, event) end)
        end
        ferryx.bind('ctrl+a', function(s) ferryx.send(s, 'key') end)
    "#).unwrap();
    for event in ["agent.state", "agent.done", "session.exit", "layout.changed"] {
        vm.event(event, &serde_json::json!({"sessionId":"session"})).unwrap();
    }
    vm.key("ctrl+a", "session").unwrap();
    assert_eq!(calls.borrow().len(), 5);
    assert!(!vm.api.bindings.borrow()["ctrl+a"].0);
    assert_eq!(api::chord("SHIFT+CMD+a").unwrap(), "cmd+shift+a");
}

#[test]
fn script_api_repeating_timer_uses_durable_claim_without_clock_sleeps() {
    use super::store::Shared;
    use std::cell::Cell;
    let clock = Rc::new(Cell::new(0_u64));
    let timer_clock = clock.clone();
    let calls = Rc::new(Cell::new(0));
    let effects = calls.clone();
    let vm = Vm::new("repeat".into(), Rc::new(move || timer_clock.get()), move |_, _| {
        effects.set(effects.get() + 1); Ok(())
    }).unwrap();
    vm.load("ferryx.every(10, function() ferryx.log('tick') end)").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let registry = Shared::load(dir.path()).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for time in [10, 10, 20] { clock.set(time); vm.fire(&registry, runtime.handle()).unwrap(); }
    assert_eq!(calls.get(), 2);
    assert_eq!(vm.timers.borrow().len(), 1);
    registry.freeze();
    clock.set(30);
    vm.fire(&registry, runtime.handle()).unwrap();
    assert_eq!(calls.get(), 2);
}
