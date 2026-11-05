//! The only guest import is a bounded JSON request. No WASI linker is installed.
use super::{MAX_LINE, MAX_MODULE};
use std::io::{BufRead, Read, Write};
use wasmi::{Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

struct Host { limits: StoreLimits, calls: usize, replies: Option<std::sync::mpsc::Receiver<Vec<u8>>> }

pub fn execute(bytes: &[u8]) -> Result<(), String> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut line = Vec::new();
            match (&mut input).take((MAX_LINE + 1) as u64).read_until(b'\n', &mut line) {
                Ok(_) if line.last() == Some(&b'\n') && line.len() <= MAX_LINE => {
                    if send.send(line).is_err() { return; }
                },
                _ => std::process::exit(0),
            }
        }
    });
    execute_inner(bytes, 3600, Some(receive))
}

#[cfg(test)]
fn execute_ticks(bytes: &[u8], ticks: usize) -> Result<(), String> {
    execute_inner(bytes, ticks, None)
}

fn execute_inner(bytes: &[u8], ticks: usize, replies: Option<std::sync::mpsc::Receiver<Vec<u8>>>) -> Result<(), String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes).map_err(|e| e.to_string())?;
    let mut store = Store::new(&engine, Host {
        limits: StoreLimitsBuilder::new().memory_size(8 * 1024 * 1024)
            .table_elements(1024).instances(1).memories(1).tables(1).build(),
        calls: 0,
        replies,
    });
    store.limiter(|host| &mut host.limits);
    store.set_fuel(5_000_000).map_err(|e| e.to_string())?;
    let mut linker = Linker::new(&engine);
    linker.func_wrap("ferryx", "request", |mut caller: Caller<'_, Host>, ptr: i32, len: i32,
        out: i32, capacity: i32| -> Result<i32, wasmi::Error> {
        if caller.data().calls >= 32 { return Err(wasmi::Error::new("host call limit")); }
        caller.data_mut().calls += 1;
        let range = |ptr: i32, len: i32| -> Result<(usize, usize), wasmi::Error> {
            let ptr = usize::try_from(ptr).map_err(|_| wasmi::Error::new("negative pointer"))?;
            let len = usize::try_from(len).map_err(|_| wasmi::Error::new("negative length"))?;
            if len > MAX_LINE { return Err(wasmi::Error::new("request exceeds limit")); }
            ptr.checked_add(len).ok_or_else(|| wasmi::Error::new("pointer overflow"))?;
            Ok((ptr, len))
        };
        let (ptr, len) = range(ptr, len)?;
        let (out, capacity) = range(out, capacity)?;
        let memory = caller.get_export("memory").and_then(|export| export.into_memory())
            .ok_or_else(|| wasmi::Error::new("exported memory required"))?;
        let mut request = vec![0; len];
        memory.read(&caller, ptr, &mut request).map_err(|_| wasmi::Error::new("invalid input range"))?;
        if request.contains(&b'\n') { return Err(wasmi::Error::new("embedded newline")); }
        // Validate both ranges before producing any host-side effect.
        if out.checked_add(capacity).is_none_or(|end| end > memory.data(&caller).len()) {
            return Err(wasmi::Error::new("invalid output range"));
        }
        let _: serde_json::Value = serde_json::from_slice(&request)
            .map_err(|_| wasmi::Error::new("invalid request JSON"))?;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&request).and_then(|()| stdout.write_all(b"\n"))
            .and_then(|()| stdout.flush()).map_err(|e| wasmi::Error::new(e.to_string()))?;
        drop(stdout);
        let mut response = caller.data().replies.as_ref()
            .ok_or_else(|| wasmi::Error::new("host reply channel unavailable"))?
            .recv_timeout(std::time::Duration::from_secs(6))
            .map_err(|e| wasmi::Error::new(e.to_string()))?;
        if response.last() != Some(&b'\n') || response.len() > MAX_LINE {
            return Err(wasmi::Error::new("invalid host response"));
        }
        response.pop();
        if response.len() > capacity { return Err(wasmi::Error::new("response capacity exceeded")); }
        memory.write(&mut caller, out, &response).map_err(|_| wasmi::Error::new("invalid output range"))?;
        Ok(response.len() as i32)
    }).map_err(|e| e.to_string())?;
    let instance = linker.instantiate(&mut store, &module).and_then(|pre| pre.start(&mut store))
        .map_err(|e| e.to_string())?;
    let tick = instance.get_typed_func::<i64, ()>(&store, "tick").map_err(|e| e.to_string())?;
    for iteration in 0..ticks {
        store.set_fuel(5_000_000).map_err(|e| e.to_string())?;
        store.data_mut().calls = 0;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_secs() as i64;
        tick.call(&mut store, now).map_err(|e| e.to_string())?;
        if iteration + 1 < ticks { std::thread::sleep(std::time::Duration::from_secs(1)); }
    }
    Ok(())
}

pub fn entry() -> Option<i32> {
    if std::env::args_os().nth(1).is_none_or(|arg| arg != "--plugin-wasm-runner") { return None; }
    Some(run_stdio())
}

pub fn run_stdio() -> i32 {
    let result = (|| {
        let mut header = [0; 4];
        std::io::stdin().read_exact(&mut header).map_err(|e| e.to_string())?;
        let len = u32::from_le_bytes(header) as usize;
        if len > MAX_MODULE { return Err("module exceeds limit".into()); }
        let mut bytes = vec![0; len];
        std::io::stdin().read_exact(&mut bytes).map_err(|e| e.to_string())?;
        execute(&bytes)
    })();
    match result { Ok(()) => 0, Err(error) => { eprintln!("plugin runner: {error}"); 1 } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_imports_cannot_instantiate_even_in_start_function() {
        for (module, name) in [("wasi_snapshot_preview1", "sock_open"),
            ("wasi_snapshot_preview1", "path_open"), ("wasi_snapshot_preview1", "environ_get"),
            ("env", "fork"), ("env", "exec"), ("env", "setsid"),
            ("env", "connect"), ("env", "CreateProcessW"), ("env", "OpenFile")]
        {
            let wasm = format!("(module (import \"{module}\" \"{name}\" (func $escape)) (func $start call $escape) (start $start) (func (export \"tick\") (param i64)))");
            assert!(execute_ticks(wasm.as_bytes(), 1).is_err(), "{module}/{name}");
        }
    }

    #[test]
    fn guest_fuel_memory_and_pointer_limits_trap() {
        for wasm in [
            "(module (func (export \"tick\") (param i64) (loop $forever br $forever)))",
            "(module (memory 129) (func (export \"tick\") (param i64)))",
            "(module (import \"ferryx\" \"request\" (func $request (param i32 i32 i32 i32) (result i32))) (memory (export \"memory\") 1) (func (export \"tick\") (param i64) i32.const -1 i32.const 1 i32.const 0 i32.const 0 call $request drop))",
        ] {
            assert!(execute_ticks(wasm.as_bytes(), 1).is_err());
        }
        assert!(execute_ticks(b"(module (func (export \"tick\") (param i64)))", 1).is_ok());
    }

    #[test]
    fn host_call_budget_is_enforced_with_real_replies() {
        let (send, receive) = std::sync::mpsc::channel();
        for _ in 0..33 { send.send(b"{}\n".to_vec()).unwrap(); }
        let module = br#"(module
            (import "ferryx" "request" (func $request (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1) (data (i32.const 0) "{}")
            (func (export "tick") (param i64) (local $count i32)
                (loop $again
                    i32.const 0 i32.const 2 i32.const 100 i32.const 100 call $request drop
                    local.get $count i32.const 1 i32.add local.tee $count
                    i32.const 33 i32.lt_u br_if $again)))"#;
        let result = execute_inner(module, 1, Some(receive));
        assert!(result.unwrap_err().contains("host call limit"));
    }
}

