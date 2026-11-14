//! TWIN-ONLY QA driver for todo 19 (never committed): prints a helper upload or prune script.
//! usage: tpi19_qa_scripts <b64|octal> <dest> <root> <binary> | tpi19_qa_scripts prune <versions> <current>
use ferryx_lib::ssh::helper_setup::{
    build_posix_octal_upload_script, build_posix_upload_script, posix_prune_versions_script, HelperLocation,
};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "prune" {
        print!("{}", posix_prune_versions_script(&a[2], &a[3]));
        return;
    }
    let loc = HelperLocation { executable: a[2].clone(), root: a[3].clone() };
    let bytes = std::fs::read(&a[4]).expect("binary");
    let script = match a[1].as_str() {
        "b64" => build_posix_upload_script(&loc, &bytes),
        "octal" => build_posix_octal_upload_script(&loc, &bytes),
        other => panic!("mode {other}"),
    };
    print!("{script}");
}
