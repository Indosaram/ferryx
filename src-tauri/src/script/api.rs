//! Versioned owner API. Host effects are supplied by the daemon, never by Luau libraries.
use mlua::{Function, Lua, LuaSerdeExt, Table, Value};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub const CAPABILITY: &str = "luauApiV1";
pub type Host = Rc<dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, String>>;
pub struct Api {
    pub bindings: RefCell<BTreeMap<String, (bool, Function)>>,
    events: RefCell<BTreeMap<String, Vec<Function>>>,
    pub session: Rc<RefCell<Option<String>>>,
}
fn bad() -> mlua::Error { mlua::Error::runtime("BAD_ARGUMENT") }
fn text(value: Value) -> mlua::Result<String> {
    match value { Value::String(s) => Ok(s.to_str()?.to_owned()), _ => Err(bad()) }
}
pub fn chord(keys: &str) -> Result<String, String> {
    let keys = keys.to_ascii_lowercase();
    let mut parts: Vec<_> = keys.split('+').collect();
    let key = parts.pop().filter(|p| !p.is_empty()).ok_or("BAD_ARGUMENT")?;
    if parts.iter().any(|p| !["cmd", "ctrl", "alt", "shift"].contains(p)) {
        return Err("BAD_ARGUMENT".into());
    }
    parts.sort_unstable();
    parts.dedup();
    parts.push(key);
    Ok(parts.join("+"))
}
impl Api {
    pub fn install(lua: &Lua, host: Host) -> mlua::Result<Rc<Self>> {
        let api = Rc::new(Self { bindings: RefCell::new(BTreeMap::new()), events: RefCell::new(BTreeMap::new()), session: Rc::new(RefCell::new(None)) });
        let ferryx: Table = lua.globals().get("ferryx")?;
        ferryx.set("version", CAPABILITY)?;
        let bindings = Rc::downgrade(&api);
        ferryx.set("bind", lua.create_function(move |_, (keys, callback, options): (String, Function, Option<Table>)| {
            let keys = chord(&keys).map_err(mlua::Error::runtime)?;
            let overrides = options.map(|o| o.get::<Option<bool>>("override")).transpose()?.flatten().unwrap_or(false);
            bindings.upgrade().ok_or_else(|| mlua::Error::runtime("Script unloaded"))?.bindings.borrow_mut().insert(keys, (overrides, callback));
            Ok(())
        })?)?;
        let events = Rc::downgrade(&api);
        ferryx.set("on", lua.create_function(move |_, (event, callback): (String, Function)| {
            if !["agent.state", "agent.done", "session.exit", "layout.changed"].contains(&event.as_str()) { return Err(bad()); }
            events.upgrade().ok_or_else(|| mlua::Error::runtime("Script unloaded"))?.events.borrow_mut().entry(event).or_default().push(callback);
            Ok(())
        })?)?;
        for name in ["split", "send", "read", "notify", "exec", "layout"] {
            let host = host.clone();
            let session = api.session.clone();
            ferryx.set(name, lua.create_function(move |lua, args: mlua::MultiValue| {
                let mut args = validate(lua, name, args)?;
                if name == "exec" {
                    args["session"] = session.borrow().clone().ok_or_else(|| mlua::Error::runtime("NO_SESSION_CONTEXT"))?.into();
                }
                let result = host(name, args).map_err(mlua::Error::runtime)?;
                lua.to_value(&result)
            })?)?;
        }
        Ok(api)
    }
    pub fn dispatch(&self, lua: &Lua, event: &str, value: &serde_json::Value) -> mlua::Result<()> {
        *self.session.borrow_mut() = value["sessionId"].as_str().map(str::to_owned);
        let callbacks = self.events.borrow().get(event).cloned().unwrap_or_default();
        for callback in callbacks { callback.call::<()>(lua.to_value(value)?)?; }
        Ok(())
    }
}
fn validate(lua: &Lua, name: &str, args: mlua::MultiValue) -> mlua::Result<serde_json::Value> {
    let mut args = args.into_iter();
    let mut next = || args.next().ok_or_else(bad);
    let value = match name {
        "split" => {
            let session = text(next()?)?;
            let direction = text(next()?)?;
            if !["right", "left", "up", "down"].contains(&direction.as_str()) { return Err(bad()); }
            serde_json::json!({"session":session,"direction":direction})
        }
        "send" => serde_json::json!({"session":text(next()?)?,"text":text(next()?)?}),
        "notify" => serde_json::json!({"title":text(next()?)?,"body":text(next()?)?}),
        "read" => {
            let session = text(next()?)?;
            let Value::Integer(lines) = next()? else { return Err(bad()); };
            if !(1..=10_000).contains(&lines) { return Err(bad()); }
            serde_json::json!({"session":session,"lines":lines})
        }
        "exec" => {
            let Value::Table(table) = next()? else { return Err(bad()); };
            let mut argv = Vec::new();
            for entry in table.clone().sequence_values::<Value>() { argv.push(text(entry?)?); }
            if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|s| s.contains('\0')) { return Err(bad()); }
            if table.pairs::<Value, Value>().count() != argv.len() { return Err(bad()); }
            serde_json::json!({"argv":argv})
        }
        "layout" => {
            let window = text(next()?)?;
            let Value::Integer(revision) = next()? else { return Err(bad()); };
            if revision < 0 { return Err(bad()); }
            let Value::Table(intent) = next()? else { return Err(bad()); };
            let intent: crate::daemon::layout::Intent = lua.from_value(Value::Table(intent)).map_err(|_| bad())?;
            serde_json::json!({"window":window,"revision":revision,"intent":intent})
        }
        _ => return Err(bad()),
    };
    if args.next().is_some() { return Err(bad()); }
    Ok(value)
}
