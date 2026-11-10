//! Windows: files dragged from Explorer onto a tab's page.
//!
//! Chromium registers its OLE drop target on the tab's top-level `Chrome_WidgetWin_1` window,
//! and the engine makes that window a child of the Ferryx window (`platform::embed`). Embedded
//! that way, a file drag over the page reaches the window but never the page (Windows CI run
//! 42883999543: no dragenter, no drop), while the same files dropped through CDP
//! `Input.dispatchDragEvent` arrive as real `File` objects. So the window's drop target is
//! wrapped: a drag that carries files (`CF_HDROP`) is handed to the page through
//! `Input.dispatchDragEvent` at the pointer's page point; any other drag (one Chromium itself
//! started, text, links) goes to Chromium's own target unchanged.

use super::{TABS, platform};
use cef::*;
use ferryx_browser_engine_protocol::json_string;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::{E_NOINTERFACE, HWND, POINTL, S_OK};
use windows_sys::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL};
use windows_sys::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT_COPY, DROPEFFECT_NONE, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows_sys::Win32::UI::Shell::DragQueryFileW;
use windows_sys::Win32::UI::WindowsAndMessaging::GetPropW;
use windows_sys::core::{GUID, HRESULT, IID_IUnknown};

const IID_IDROPTARGET: GUID = GUID::from_u128(0x00000122_0000_0000_c000_000000000046);

/// DevTools message ids of these drops: far above Ferryx's (below 1 000 000) and the vault's
/// (counting up from 1 000 000), so no observer takes their answers for its own.
const FIRST_ID: i32 = 2_000_000_000;

type Hr = HRESULT;

#[repr(C)]
struct DropTargetVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> Hr,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    drag_enter: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINTL, *mut u32) -> Hr,
    drag_over: unsafe extern "system" fn(*mut c_void, u32, POINTL, *mut u32) -> Hr,
    drag_leave: unsafe extern "system" fn(*mut c_void) -> Hr,
    drop: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINTL, *mut u32) -> Hr,
}

/// The leading `IDataObject` methods, up to `GetData`.
#[repr(C)]
struct DataObjectVtbl {
    _query_interface: usize,
    _add_ref: usize,
    _release: usize,
    get_data: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> Hr,
}

/// A COM object: its first field is the vtable pointer.
unsafe fn vtbl<T>(object: *mut c_void) -> &'static T {
    unsafe { &**(object as *const *const T) }
}

/// Where the current drag goes.
#[derive(Clone, PartialEq)]
enum Drag {
    None,
    /// Chromium's own target handles it.
    Chromium,
    /// Files for the page; `entered` once the page got its dragEnter.
    Files { tab: String, paths: Vec<String>, entered: bool, last: Option<(i32, i32)> },
}

#[repr(C)]
struct Target {
    vtbl: &'static DropTargetVtbl,
    refs: AtomicU32,
    hwnd: HWND,
    /// Chromium's own drop target for the window (one reference held), or null.
    inner: *mut c_void,
    drag: RefCell<Drag>,
}

static VTBL: DropTargetVtbl = DropTargetVtbl {
    query_interface,
    add_ref,
    release,
    drag_enter,
    drag_over,
    drag_leave,
    drop: on_drop,
};

thread_local! {
    static NEXT_ID: Cell<i32> = const { Cell::new(FIRST_ID) };
}

/// Wraps the drop target of a tab's window (UI thread, once the window exists).
pub(super) fn install(window: &Window) {
    let hwnd = platform::handle(window) as usize as HWND;
    // OLE keeps the registered target in this window property; it is the only way to reach the
    // target Chromium registered. One reference is taken before revoking drops OLE's.
    let inner = unsafe { GetPropW(hwnd, windows_sys::w!("OleDropTargetInterface")) };
    if !inner.is_null() {
        unsafe {
            (vtbl::<DropTargetVtbl>(inner).add_ref)(inner);
            RevokeDragDrop(hwnd);
        }
    }
    let target = Box::into_raw(Box::new(Target {
        vtbl: &VTBL,
        refs: AtomicU32::new(1),
        hwnd,
        inner,
        drag: RefCell::new(Drag::None),
    })) as *mut c_void;
    let result = unsafe { RegisterDragDrop(hwnd, target) };
    if result != S_OK {
        eprintln!("ferryx-engine: file drop: RegisterDragDrop failed ({result:#x}); Chromium's target restored");
        if !inner.is_null() {
            unsafe { RegisterDragDrop(hwnd, inner) };
        }
    } else {
        eprintln!("ferryx-engine: file drop: target on {hwnd:?} (Chromium's: {})", !inner.is_null());
    }
    // RegisterDragDrop holds its own reference.
    unsafe { release(target) };
}

unsafe fn this<'a>(object: *mut c_void) -> &'a Target {
    unsafe { &*(object as *const Target) }
}

unsafe extern "system" fn query_interface(object: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> Hr {
    let iid = unsafe { &*iid };
    let same = |other: &GUID| {
        iid.data1 == other.data1 && iid.data2 == other.data2 && iid.data3 == other.data3 && iid.data4 == other.data4
    };
    if same(&IID_IUnknown) || same(&IID_IDROPTARGET) {
        unsafe {
            add_ref(object);
            *out = object;
        }
        S_OK
    } else {
        unsafe { *out = std::ptr::null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref(object: *mut c_void) -> u32 {
    unsafe { this(object) }.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn release(object: *mut c_void) -> u32 {
    let left = unsafe { this(object) }.refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if left == 0 {
        let target = unsafe { Box::from_raw(object as *mut Target) };
        if !target.inner.is_null() {
            unsafe { (vtbl::<DropTargetVtbl>(target.inner).release)(target.inner) };
        }
    }
    left
}

/// The file paths a drag carries (`CF_HDROP`); empty for any other drag.
fn dropped_paths(data: *mut c_void) -> Vec<String> {
    if data.is_null() {
        return Vec::new();
    }
    let format = FORMATETC {
        cfFormat: CF_HDROP,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL as u32,
    };
    let mut medium = STGMEDIUM::default();
    if unsafe { (vtbl::<DataObjectVtbl>(data).get_data)(data, &format, &mut medium) } != S_OK {
        return Vec::new();
    }
    let drop = unsafe { medium.u.hGlobal };
    let count = unsafe { DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0) };
    let paths = (0..count)
        .filter_map(|index| {
            let length = unsafe { DragQueryFileW(drop, index, std::ptr::null_mut(), 0) };
            let mut buffer = vec![0u16; length as usize + 1];
            let copied = unsafe { DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
            (copied > 0).then(|| String::from_utf16_lossy(&buffer[..copied as usize]))
        })
        .collect();
    unsafe { ReleaseStgMedium(&mut medium) };
    paths
}

/// The Ferryx tab whose window is `hwnd`.
fn tab_of(hwnd: HWND) -> Option<String> {
    TABS.with_borrow(|tabs| {
        tabs.iter()
            .find(|(_, pane)| pane.window.as_ref().is_some_and(|window| platform::handle(window) as usize == hwnd as usize))
            .map(|(tab, _)| tab.clone())
    })
}

/// The page point (CSS pixels in the main frame's viewport) under screen pixel `point`, or
/// None when it is outside the page (over Chrome's toolbar).
fn page_point(tab: &str, point: POINTL) -> Option<(i32, i32)> {
    let (view, browser) = TABS.with_borrow(|tabs| {
        let pane = tabs.get(tab)?;
        Some((pane.view.clone(), pane.view.browser()?))
    })?;
    let mut at = display_convert_screen_point_from_pixels(Some(&Point { x: point.x, y: point.y }));
    if view.convert_point_from_screen(Some(&mut at)) == 0 {
        return None;
    }
    let bounds = view.bounds();
    if at.x < 0 || at.y < 0 || at.x >= bounds.width || at.y >= bounds.height {
        return None;
    }
    // CEF zoom levels are steps of 20% (system.rs `zoom`).
    let zoom = browser.host().map_or(0.0, |host| host.zoom_level());
    let factor = 1.2_f64.powf(zoom);
    Some(((f64::from(at.x) / factor).round() as i32, (f64::from(at.y) / factor).round() as i32))
}

/// Sends one `Input.dispatchDragEvent` of `kind` to the tab's page.
fn dispatch(tab: &str, kind: &str, (x, y): (i32, i32), paths: &[String]) -> bool {
    let Some(host) = TABS.with_borrow(|tabs| tabs.get(tab).and_then(|pane| pane.view.browser())).and_then(|browser| browser.host())
    else {
        return false;
    };
    let id = NEXT_ID.get();
    NEXT_ID.set(id.checked_add(1).unwrap_or(FIRST_ID));
    let files = paths.iter().map(|path| json_string(path)).collect::<Vec<_>>().join(",");
    let message = format!(
        "{{\"id\":{id},\"method\":\"Input.dispatchDragEvent\",\"params\":{{\"type\":\"{kind}\",\"x\":{x},\"y\":{y},\
         \"data\":{{\"items\":[],\"files\":[{files}],\"dragOperationsMask\":1}}}}}}"
    );
    host.send_dev_tools_message(Some(message.as_bytes())) != 0
}

/// Moves a files drag to `point`: dragEnter when the pointer first reaches the page, dragOver
/// while it moves on it, dragCancel when it leaves for the toolbar. Returns the drop effect.
fn track(drag: &mut Drag, point: POINTL) -> u32 {
    let Drag::Files { tab, paths, entered, last } = drag else { return DROPEFFECT_NONE };
    let Some(at) = page_point(tab, point) else {
        if *entered {
            dispatch(tab, "dragCancel", last.unwrap_or_default(), paths);
            *entered = false;
        }
        return DROPEFFECT_NONE;
    };
    if !*entered {
        *entered = dispatch(tab, "dragEnter", at, paths);
        *last = Some(at);
    } else if *last != Some(at) {
        dispatch(tab, "dragOver", at, paths);
        *last = Some(at);
    }
    if *entered { DROPEFFECT_COPY } else { DROPEFFECT_NONE }
}

unsafe extern "system" fn drag_enter(object: *mut c_void, data: *mut c_void, keys: u32, point: POINTL, effect: *mut u32) -> Hr {
    let target = unsafe { this(object) };
    let paths = dropped_paths(data);
    let tab = (!paths.is_empty()).then(|| tab_of(target.hwnd)).flatten();
    let Some(tab) = tab else {
        target.drag.replace(Drag::Chromium);
        if target.inner.is_null() {
            unsafe { *effect = DROPEFFECT_NONE };
            return S_OK;
        }
        return unsafe { (vtbl::<DropTargetVtbl>(target.inner).drag_enter)(target.inner, data, keys, point, effect) };
    };
    eprintln!("ferryx-engine: file drop {tab}: {} file(s) entered at ({}, {})", paths.len(), point.x, point.y);
    let mut drag = Drag::Files { tab, paths, entered: false, last: None };
    let allowed = track(&mut drag, point);
    target.drag.replace(drag);
    unsafe { *effect &= allowed };
    S_OK
}

unsafe extern "system" fn drag_over(object: *mut c_void, keys: u32, point: POINTL, effect: *mut u32) -> Hr {
    let target = unsafe { this(object) };
    // No borrow is held while Chromium's target runs: it can pump messages.
    let files = matches!(*target.drag.borrow(), Drag::Files { .. });
    if files {
        let allowed = track(&mut target.drag.borrow_mut(), point);
        unsafe { *effect &= allowed };
        return S_OK;
    }
    if target.inner.is_null() {
        unsafe { *effect = DROPEFFECT_NONE };
        return S_OK;
    }
    unsafe { (vtbl::<DropTargetVtbl>(target.inner).drag_over)(target.inner, keys, point, effect) }
}

unsafe extern "system" fn drag_leave(object: *mut c_void) -> Hr {
    let target = unsafe { this(object) };
    let drag = target.drag.replace(Drag::None);
    match drag {
        Drag::Files { tab, paths, entered, last } => {
            if entered {
                dispatch(&tab, "dragCancel", last.unwrap_or_default(), &paths);
            }
            S_OK
        }
        Drag::Chromium if !target.inner.is_null() => unsafe { (vtbl::<DropTargetVtbl>(target.inner).drag_leave)(target.inner) },
        _ => S_OK,
    }
}

unsafe extern "system" fn on_drop(object: *mut c_void, data: *mut c_void, keys: u32, point: POINTL, effect: *mut u32) -> Hr {
    let target = unsafe { this(object) };
    let mut drag = target.drag.replace(Drag::None);
    if !matches!(drag, Drag::Files { .. }) {
        if target.inner.is_null() {
            unsafe { *effect = DROPEFFECT_NONE };
            return S_OK;
        }
        return unsafe { (vtbl::<DropTargetVtbl>(target.inner).drop)(target.inner, data, keys, point, effect) };
    }
    let allowed = track(&mut drag, point);
    let Drag::Files { tab, paths, entered, last } = &drag else { return S_OK };
    // A dragOver right before the drop, as a page sees from a real drag: Blink drops only on a
    // target whose last dragover accepted it.
    let at = last.unwrap_or_default();
    let sent = *entered && dispatch(tab, "dragOver", at, paths) && dispatch(tab, "drop", at, paths);
    eprintln!("ferryx-engine: file drop {tab}: dropped at ({}, {}), page point {last:?}, sent {sent}", point.x, point.y);
    unsafe { *effect &= if sent { allowed } else { DROPEFFECT_NONE } };
    S_OK
}
