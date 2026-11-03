//! CEF subprocess (renderer, GPU, utility) for the macOS app bundle.
use cef::{args::Args, *};

fn main() {
    let args = Args::new();

    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
        assert!(loader.load());
        loader
    };

    #[cfg(all(target_os = "macos", feature = "sandbox"))]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    execute_process(Some(args.as_main_args()), None::<&mut App>, std::ptr::null_mut());
}
