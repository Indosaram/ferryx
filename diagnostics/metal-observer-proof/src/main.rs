use core_graphics_types::geometry::{CGPoint, CGRect, CGSize};
use objc::rc::StrongPtr;
use objc::runtime::{Object, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

#[link(name = "Foundation", kind = "framework")]
#[link(name = "QuartzCore", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
#[link(name = "Metal", kind = "framework")]
extern "C" {
    fn MTLCreateSystemDefaultDevice() -> *mut Object;
}

static DRAWABLES_REQUESTED: AtomicUsize = AtomicUsize::new(0);
static DRAWABLES_ACQUIRED: AtomicUsize = AtomicUsize::new(0);

#[path = "/Users/indo/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-hal-24.0.4/src/metal/layer_observer.rs"]
mod original_observer;

mod corrected_observer;

unsafe fn create_and_drop_cycle(
    new_layer_fn: unsafe fn(*mut Object) -> StrongPtr,
    device: *mut Object,
    with_drawables: bool,
) {
    let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];

    let _: () = msg_send![class!(CATransaction), begin];

    let view: *mut Object = msg_send![class!(NSView), alloc];
    let initial_rect = CGRect::new(
        &CGPoint::new(0.0, 0.0),
        &CGSize::new(200.0, 200.0),
    );
    let view: *mut Object = msg_send![view, initWithFrame: initial_rect];
    let _: () = msg_send![view, setWantsLayer: YES];
    let root_layer: *mut Object = msg_send![view, layer];

    let observer_layer = new_layer_fn(root_layer);

    if !device.is_null() {
        let raw_layer: *mut Object = *observer_layer;
        let _: () = msg_send![raw_layer, setDevice: device];

        if with_drawables {
            // MTLPixelFormatBGRA8Unorm = 80
            let _: () = msg_send![raw_layer, setPixelFormat: 80usize];
            let _: () = msg_send![raw_layer, setDrawableSize: CGSize::new(512.0, 512.0)];
            let _: () = msg_send![raw_layer, setAllowsNextDrawableTimeout: YES];

            DRAWABLES_REQUESTED.fetch_add(1, Ordering::SeqCst);
            let drawable_pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
            let drawable: *mut Object = msg_send![raw_layer, nextDrawable];
            if !drawable.is_null() {
                DRAWABLES_ACQUIRED.fetch_add(1, Ordering::SeqCst);
            }
            let _: () = msg_send![drawable_pool, drain];
        }
    }

    let resized_rect = CGRect::new(
        &CGPoint::new(0.0, 0.0),
        &CGSize::new(300.0, 300.0),
    );
    let _: () = msg_send![view, setFrame: resized_rect];

    let _: () = msg_send![class!(CATransaction), commit];
    let _: () = msg_send![class!(CATransaction), flush];

    let _: () = msg_send![class!(CATransaction), begin];

    drop(observer_layer);

    let _: () = msg_send![view, removeFromSuperview];
    let _: () = msg_send![view, release];

    let _: () = msg_send![class!(CATransaction), commit];
    let _: () = msg_send![class!(CATransaction), flush];

    let _: () = msg_send![pool, drain];
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("original");
    let cycles: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let with_drawables = args.iter().any(|arg| arg == "--with-drawables");
    let wait_stdin = args.iter().any(|arg| arg == "--wait-stdin");

    println!("=== WGPU Metal Layer Observer Proof Harness ===");
    println!("Process PID: {}", std::process::id());
    println!(
        "Mode: {}, Target cycles: {}, With drawables: {}",
        mode, cycles, with_drawables
    );

    unsafe {
        let device = MTLCreateSystemDefaultDevice();
        println!("Metal default device: {:p}", device);

        match mode {
            "original" => {
                for _ in 0..cycles {
                    create_and_drop_cycle(
                        original_observer::new_observer_layer,
                        device,
                        with_drawables,
                    );
                }
            }
            "corrected" => {
                for _ in 0..cycles {
                    create_and_drop_cycle(
                        corrected_observer::new_observer_layer,
                        device,
                        with_drawables,
                    );
                }
            }
            unknown => {
                eprintln!("Unknown mode: {}. Use 'original' or 'corrected'.", unknown);
                std::process::exit(1);
            }
        }

        let _: () = msg_send![device, release];

        println!("Drawables requested: {}", DRAWABLES_REQUESTED.load(Ordering::SeqCst));
        println!("Drawables acquired: {}", DRAWABLES_ACQUIRED.load(Ordering::SeqCst));
        if with_drawables && DRAWABLES_ACQUIRED.load(Ordering::SeqCst) == 0 {
            eprintln!("Notice: Headless nextDrawable returned null (offscreen windowless CAMetalLayer).");
        }

        if wait_stdin {
            println!("PAUSED_FOR_HEAP_INSPECTION");
            std::io::stdout().flush().expect("failed to flush stdout");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).expect("failed to read stdin");
        }
    }

    println!("Experiment run finished cleanly.");
}
