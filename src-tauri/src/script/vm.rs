use super::store::Registry;
use mlua::{Function, Lua, VmState};
use std::{cell::{Cell, RefCell}, rc::Rc, time::Duration};

pub struct Timer {
    pub id: String,
    pub period: u64,
    pub due: u64,
    pub callback: Function,
}

pub struct Vm {
    lua: Lua,
    pub timers: Rc<RefCell<Vec<Timer>>>,
    clock: Rc<dyn Fn() -> u64>,
    startup: Rc<Cell<bool>>,
    budget: Rc<RefCell<cpu_time::ThreadTime>>,
}

impl Vm {
    pub fn new(
        identity: String,
        clock: Rc<dyn Fn() -> u64>,
        log: impl Fn(String, bool) -> Result<(), String> + 'static,
    ) -> mlua::Result<Self> {
        let lua = Lua::new();
        lua.set_memory_limit(64 * 1024 * 1024)?;
        for name in ["io", "os", "package", "debug", "load", "loadfile", "loadstring", "dofile", "require", "pcall", "xpcall"] {
            lua.globals().set(name, mlua::Nil)?;
        }
        let budget = Rc::new(RefCell::new(cpu_time::ThreadTime::now()));
        let interrupt_budget = budget.clone();
        lua.set_interrupt(move |_| {
            if interrupt_budget.borrow().elapsed() >= Duration::from_millis(50) {
                return Err(mlua::Error::runtime("SCRIPT_INTERRUPTED"));
            }
            Ok(VmState::Continue)
        });
        let timers = Rc::new(RefCell::new(Vec::new()));
        let pending = timers.clone();
        let sequence = Rc::new(Cell::new(0_u64));
        let timer_clock = clock.clone();
        let ferryx = lua.create_table()?;
        ferryx.set("after", lua.create_function(move |_, (period, callback): (u64, Function)| {
            let mut timers = pending.borrow_mut();
            if timers.len() >= 64 { return Err(mlua::Error::runtime("SCRIPT_TIMER_LIMIT")); }
            if period == 0 { return Err(mlua::Error::runtime("Timer period must be positive")); }
            let number = sequence.get();
            sequence.set(number + 1);
            let id = format!("{identity}/{number}");
            timers.push(Timer { id: id.clone(), period, due: timer_clock().saturating_add(period), callback });
            Ok(id)
        })?)?;
        let startup = Rc::new(Cell::new(true));
        let in_startup = startup.clone();
        ferryx.set("log", lua.create_function(move |_, message: String| {
            log(message, in_startup.get()).map_err(mlua::Error::runtime)
        })?)?;
        lua.globals().set("ferryx", ferryx)?;
        lua.sandbox(true)?;
        Ok(Self { lua, timers, clock, startup, budget })
    }

    pub fn load(&self, source: &str) -> mlua::Result<()> {
        *self.budget.borrow_mut() = cpu_time::ThreadTime::now();
        let result = self.lua.load(source).set_name("init.luau").exec();
        self.startup.set(false);
        if result.is_err() { self.timers.borrow_mut().clear(); }
        result
    }

    pub fn next_wait(&self) -> Duration {
        self.timers.borrow().iter().map(|timer| timer.due.saturating_sub((self.clock)()))
            .min().map(Duration::from_millis).unwrap_or(Duration::from_secs(3600))
    }

    pub fn fire(&self, registry: &Registry, host: &tokio::runtime::Handle) -> Result<(), String> {
        let now = (self.clock)();
        let mut due = Vec::new();
        {
            let mut timers = self.timers.borrow_mut();
            let mut index = 0;
            while index < timers.len() {
                if timers[index].due <= now { due.push(timers.remove(index)); } else { index += 1; }
            }
        }
        for timer in due {
            // Freeze waits for the entire callback, not merely the durable claim. No effect can
            // escape the decision snapshot while the successor is taking ownership.
            let _execution = registry.execution.lock();
            let store = registry.clone();
            let claimed = super::blocking(host, move || store.store.lock().claim(&timer.id, timer.period, now))?;
            if claimed {
                *self.budget.borrow_mut() = cpu_time::ThreadTime::now();
                timer.callback.call::<()>(()).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

}
