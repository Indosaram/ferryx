use libloading::Library;
use ferryx_rd_proto::{InputEvent, InputEventType};
use std::{collections::BTreeMap, ffi::c_void, io};

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Report {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

struct Api {
    _library: Library,
    alloc: unsafe extern "C" fn() -> *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
    connect: unsafe extern "C" fn(*mut c_void) -> u32,
    disconnect: unsafe extern "C" fn(*mut c_void),
    target_alloc: unsafe extern "C" fn() -> *mut c_void,
    target_free: unsafe extern "C" fn(*mut c_void),
    add: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    remove: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32,
    update: unsafe extern "C" fn(*mut c_void, *mut c_void, Report) -> u32,
}

fn check(code: u32) -> io::Result<()> {
    if code == 0x20000000 {
        Ok(())
    } else {
        Err(io::Error::other(format!("ViGEm error {code:#x}")))
    }
}

impl Api {
    fn load() -> io::Result<Self> {
        // SAFETY: load only the native system installation, not a DLL from cwd.
        // Symbol signatures match ViGEmClient's C ABI and Library outlives them.
        unsafe {
            let library =
                Library::new("C:\\Windows\\System32\\ViGEmClient.dll").map_err(io::Error::other)?;
            Ok(Self {
                alloc: *library.get(b"vigem_alloc\0").map_err(io::Error::other)?,
                free: *library.get(b"vigem_free\0").map_err(io::Error::other)?,
                connect: *library.get(b"vigem_connect\0").map_err(io::Error::other)?,
                disconnect: *library
                    .get(b"vigem_disconnect\0")
                    .map_err(io::Error::other)?,
                target_alloc: *library
                    .get(b"vigem_target_x360_alloc\0")
                    .map_err(io::Error::other)?,
                target_free: *library
                    .get(b"vigem_target_free\0")
                    .map_err(io::Error::other)?,
                add: *library
                    .get(b"vigem_target_add\0")
                    .map_err(io::Error::other)?,
                remove: *library
                    .get(b"vigem_target_remove\0")
                    .map_err(io::Error::other)?,
                update: *library
                    .get(b"vigem_target_x360_update\0")
                    .map_err(io::Error::other)?,
                _library: library,
            })
        }
    }
}

struct Client {
    api: Api,
    client: *mut c_void,
    targets: BTreeMap<u8, (*mut c_void, Report)>,
}

impl Client {
    fn connect() -> io::Result<Self> {
        let api = Api::load()?;
        // SAFETY: native allocation owns client; failure frees it before return.
        unsafe {
            let client = (api.alloc)();
            if client.is_null() {
                return Err(io::Error::other("ViGEm allocation failed"));
            }
            if let Err(error) = check((api.connect)(client)) {
                (api.free)(client);
                return Err(error);
            }
            Ok(Self {
                api,
                client,
                targets: BTreeMap::new(),
            })
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: each target/client is uniquely owned and released before DLL unload.
        unsafe {
            for (target, _) in self.targets.values() {
                if let Err(error) = check((self.api.remove)(self.client, *target)) {
                    tracing::warn!(%error, "ViGEm target removal failed");
                }
                (self.api.target_free)(*target);
            }
            (self.api.disconnect)(self.client);
            (self.api.free)(self.client);
        }
    }
}

pub(crate) fn available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| match Client::connect() {
        Ok(_) => true,
        Err(error) => {
            tracing::info!(%error, "virtual gamepad unsupported: ViGEmBus/ViGEmClient unavailable");
            false
        }
    })
}

#[derive(Default)]
pub(crate) struct Gamepads(Option<Client>);

impl Gamepads {
    pub(crate) fn reset(&mut self) {
        self.0 = None;
    }
    pub(crate) fn inject(&mut self, event: &InputEvent) -> io::Result<()> {
        let id = (event.key_code >> 8) as u8;
        let control = usize::from(event.key_code & 255);
        if id >= 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "at most four gamepads",
            ));
        }
        if self.0.is_none() {
            self.0 = Some(Client::connect()?);
        }
        let client = self
            .0
            .as_mut()
            .ok_or_else(|| io::Error::other("ViGEm missing"))?;
        // SAFETY: handles come from this live client/DLL and reports have C layout.
        unsafe {
            if let std::collections::btree_map::Entry::Vacant(entry) = client.targets.entry(id) {
                let target = (client.api.target_alloc)();
                if target.is_null() {
                    return Err(io::Error::other("ViGEm target allocation failed"));
                }
                if let Err(error) = check((client.api.add)(client.client, target)) {
                    (client.api.target_free)(target);
                    return Err(error);
                }
                entry.insert((target, Report::default()));
            }
            let (target, report) = client
                .targets
                .get_mut(&id)
                .ok_or_else(|| io::Error::other("ViGEm target missing"))?;
            let invalid =
                || io::Error::new(io::ErrorKind::InvalidInput, "unsupported gamepad control");
            match event.event_type {
                InputEventType::GamepadAxis => {
                    if !event.x.is_finite() || !event.y.is_finite() {
                        return Err(invalid());
                    }
                    let axis = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    match control {
                        0 => {
                            report.lx = axis(event.x);
                            report.ly = axis(-event.y);
                        }
                        1 => {
                            report.rx = axis(event.x);
                            report.ry = axis(-event.y);
                        }
                        2 => {
                            report.left_trigger = (event.x.clamp(0.0, 1.0) * 255.0).round() as u8;
                            report.right_trigger = (event.y.clamp(0.0, 1.0) * 255.0).round() as u8;
                        }
                        _ => return Err(invalid()),
                    }
                }
                InputEventType::GamepadButtonDown | InputEventType::GamepadButtonUp => {
                    const BUTTONS: [u16; 15] = [
                        0x1000, 0x2000, 0x4000, 0x8000, 0x100, 0x200, 0x20, 0x10, 0x400, 0x40,
                        0x80, 1, 2, 4, 8,
                    ];
                    let mask = BUTTONS.get(control).ok_or_else(invalid)?;
                    if event.event_type == InputEventType::GamepadButtonDown {
                        report.buttons |= mask;
                    } else {
                        report.buttons &= !mask;
                    }
                }
                _ => return Err(invalid()),
            }
            check((client.api.update)(client.client, *target, *report))
        }
    }
}
