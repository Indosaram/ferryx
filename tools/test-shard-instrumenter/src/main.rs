//! Byte-preserving instrumentation for disposable test worktrees, not production sources.
use std::{env, fs, path::{Path, PathBuf}};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use serde::Serialize;
use sha2::{Digest, Sha256};
use syn::{spanned::Spanned, visit::{self, Visit}};

type Result<T> = std::result::Result<T, String>;
const MARKER: &str = "ferryx_test_shard";
#[derive(Debug, Serialize)]
struct Test {
    function: String,
    offset: usize,
    fn_offset: usize,
    bucket: usize,
    attribute: String,
}
#[derive(Serialize)]
struct FilePlan {
    path: String,
    source_sha256: String,
    instrumented_sha256: String,
    tests: Vec<Test>,
    cohort: Option<String>,
    #[serde(skip)] source: String,
    #[serde(skip)] instrumented: String,
}
#[derive(Serialize)]
struct Plan {
    format_version: u32,
    shards: usize,
    root: String,
    files: Vec<FilePlan>,
    test_count: usize,
    bucket_counts: Vec<usize>,
}
fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn bucket(path: &str, offset: usize, shards: usize) -> usize {
    let mut h = 0xcbf29ce484222325u64;
    for b in format!("{path}:{offset}").bytes() { h ^= u64::from(b); h = h.wrapping_mul(0x100000001b3); }
    (h % shards as u64) as usize
}
fn attr_kind(attr: &syn::Attribute) -> Result<Option<&'static str>> {
    let parts: Vec<_> = attr.path().segments.iter().map(|s| s.ident.to_string()).collect();
    if parts == ["test"] { return Ok(Some("test")); }
    if parts == ["tokio", "test"] { return Ok(Some("tokio::test")); }
    if parts.last().is_some_and(|s| ["test", "rstest", "test_case", "quickcheck", "proptest"].contains(&s.as_str())) {
        return Err(format!("unsupported test attribute {}", parts.join("::")));
    }
    // Conditional test registration cannot be assigned reliably without expansion.
    if attr.path().is_ident("cfg_attr") {
        if let syn::Meta::List(list) = &attr.meta {
            let nested = list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                .map_err(|e| e.to_string())?;
            for meta in nested.iter().skip(1) {
                let name = meta.path().segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                if ["test", "rstest", "test_case", "quickcheck", "proptest", "cfg_attr"].contains(&name.as_str()) {
                    return Err("unsupported conditional test registration".into());
                }
            }
        }
    }
    Ok(None)
}
fn macro_has_tests(tokens: TokenStream) -> bool {
    let ts: Vec<_> = tokens.into_iter().collect();
    for (i, t) in ts.iter().enumerate() {
        if let TokenTree::Punct(p) = t {
            if p.as_char() == '#' {
                if let Some(TokenTree::Group(g)) = ts.get(i + 1) {
                    if g.delimiter() == Delimiter::Bracket {
                        let text = g.stream().to_string().replace(' ', "");
                        if text == "test" || text.starts_with("test(") || text.contains("::test") || ["rstest", "test_case", "quickcheck", "proptest"].iter().any(|name| text.contains(name)) || text.starts_with("cfg_attr") && text.contains(",test") { return true; }
                    }
                }
            }
        }
        if let TokenTree::Group(g) = t { if macro_has_tests(g.stream()) { return true; } }
    }
    false
}
struct Scanner<'a> {
    path: &'a str, shards: usize, modules: Vec<String>, tests: Vec<Test>,
    errors: Vec<String>, attrs_seen: usize,
}
impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.modules.push(item.ident.to_string());
        visit::visit_item_mod(self, item);
        self.modules.pop();
    }
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        match attr_kind(attr) {
            Ok(Some(_)) => self.attrs_seen += 1,
            Err(e) => self.errors.push(e),
            _ => (),
        }
        visit::visit_attribute(self, attr);
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let attrs: Vec<_> = item.attrs.iter().filter_map(|a| attr_kind(a).ok().flatten()).collect();
        if attrs.len() > 1 { self.errors.push(format!("multiple test attributes on {}", item.sig.ident)); }
        if let Some(kind) = attrs.first() {
            let offset = item.attrs.first().map(|a| a.span().byte_range().start).unwrap_or(item.span().byte_range().start);
            let fn_offset = item.sig.fn_token.span.byte_range().start;
            let mut parts = self.modules.clone(); parts.push(item.sig.ident.to_string());
            self.tests.push(Test { function: parts.join("::"), offset, fn_offset, bucket: bucket(self.path, fn_offset, self.shards), attribute: kind.to_string() });
        }
        visit::visit_item_fn(self, item);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if macro_has_tests(mac.tokens.clone()) || mac.path.segments.last().is_some_and(|s| ["proptest", "quickcheck"].contains(&s.ident.to_string().as_str())) {
            self.errors.push("test-generating macro requires explicit inventory support".into());
        }
        visit::visit_macro(self, mac);
    }
}
fn self_test_cohort(path: &str, source: &str) -> Option<String> {
    // The daemon SSH fixture and bridge tests re-enter the same ignored supervisor
    // test through transport_unix's test-only ENV_SUPERVISOR_LIBTEST path.
    if ["src-tauri/src/ssh/bridge_tests.rs", "src-tauri/src/ssh/transport_unix.rs",
        "src-tauri/src/daemon/remote_ssh_tests.rs", "src-tauri/src/daemon/remote_ssh_qa.rs"].contains(&path) {
        return Some("ssh-supervisor-fixtures".into());
    }
    // Conservative file-level grouping also covers substring/--ignored dispatch.
    // Grouping a path-only current_exe use is harmless; splitting a child test is not.
    (source.contains("current_exe") || source.contains("--exact"))
        .then(|| path.to_owned())
}
fn instrument(path: &str, source: String, shards: usize) -> Result<FilePlan> {
    if source.contains(MARKER) { return Err(format!("{path}: already instrumented or reserved marker present")); }
    let ast = syn::parse_file(&source).map_err(|e| format!("{path}: {e}"))?;
    let mut scan = Scanner { path, shards, modules: vec![], tests: vec![], errors: vec![], attrs_seen: 0 };
    scan.visit_file(&ast);
    if scan.attrs_seen != scan.tests.len() { scan.errors.push("test attributes outside supported free function items".into()); }
    if !scan.errors.is_empty() { return Err(format!("{path}: {}", scan.errors.join("; "))); }
    scan.tests.sort_by_key(|t| t.offset);
    let cohort = self_test_cohort(path, &source);
    if let Some(key) = &cohort {
        for test in &mut scan.tests { test.bucket = bucket(key, 0, shards); }
    }
    let mut instrumented = source.clone();
    for t in scan.tests.iter().rev() {
        if !source.is_char_boundary(t.offset) { return Err(format!("{path}: non-UTF8 span boundary")); }
        instrumented.insert_str(t.offset, &format!("#[cfg(any(not(ferryx_test_sharding), ferryx_test_shard = \"{}\"))]\n", t.bucket));
    }
    Ok(FilePlan { path: path.into(), source_sha256: hash(source.as_bytes()), instrumented_sha256: hash(instrumented.as_bytes()), tests: scan.tests, cohort, source, instrumented })
}
fn collect(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let meta = fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() { return Err(format!("refusing symlink {}", dir.display())); }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() { return Err(format!("refusing symlink {}", path.display())); }
        if meta.is_dir() { collect(&path, paths)?; }
        else if path.extension().is_some_and(|x| x == "rs") { paths.push(path); }
    }
    Ok(())
}
fn rebalance_cohorts(files: &mut [FilePlan], shards: usize) {
    let mut loads=vec![0usize;shards];
    let mut groups=std::collections::BTreeMap::<String,Vec<usize>>::new();
    for (index,file) in files.iter().enumerate() {
        if let Some(key)=&file.cohort { groups.entry(key.clone()).or_default().push(index); }
        else { for test in &file.tests { loads[test.bucket]+=1; } }
    }
    let mut groups:Vec<_>=groups.into_iter().collect();
    groups.sort_by(|a,b| {
        let weight=|indices:&Vec<usize>| indices.iter().map(|&i|files[i].tests.len()).sum::<usize>();
        weight(&b.1).cmp(&weight(&a.1)).then_with(||a.0.cmp(&b.0))
    });
    for (_,indices) in groups {
        let bucket=(0..shards).min_by_key(|&i|(loads[i],i)).unwrap();
        for index in indices {
            let file=&mut files[index];
            for test in &mut file.tests { test.bucket=bucket; }
            loads[bucket]+=file.tests.len();
            file.instrumented=file.source.clone();
            for test in file.tests.iter().rev() {
                file.instrumented.insert_str(test.offset,&format!("#[cfg(any(not(ferryx_test_sharding), ferryx_test_shard = \"{}\"))]\n",test.bucket));
            }
            file.instrumented_sha256=hash(file.instrumented.as_bytes());
        }
    }
}
fn collect_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = vec![];
    // Library #[path] modules also reuse fixtures outside src/. Keep their test
    // functions in the same ownership plan instead of registering them in every shard.
    for relative in ["src-tauri/src", "src-tauri/tests/support"] {
        let dir = root.join(relative);
        if !dir.exists() && relative.ends_with("tests/support") { continue; }
        if dir.canonicalize().map_err(|e| e.to_string())? != dir {
            return Err("source root must not traverse symlinks".into());
        }
        collect(&dir, &mut paths)?;
    }
    paths.sort();
    Ok(paths)
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 5 || !["plan", "apply"].contains(&args[0].as_str()) || args[1] != "--root" || args[3] != "--shards" {
        return Err("usage: ferryx-test-shard-instrumenter plan|apply --root ABS_CANONICAL_ROOT --shards COUNT".into());
    }
    let root = PathBuf::from(&args[2]);
    if !root.is_absolute() || root.canonicalize().map_err(|e| e.to_string())? != root { return Err("root must be canonical absolute path without symlinks".into()); }
    let shards: usize = args[4].parse().map_err(|_| "invalid shard count")?;
    if !(1..=64).contains(&shards) { return Err("shard count must be 1..64".into()); }
    let paths = collect_sources(&root)?;
    let mut files = vec![];
    for path in paths {
        let relative = path.strip_prefix(&root).map_err(|e| e.to_string())?.to_str().ok_or("non-UTF8 path")?;
        let file = instrument(relative, fs::read_to_string(&path).map_err(|e| e.to_string())?, shards)?;
        if !file.tests.is_empty() || file.cohort.is_some() { files.push(file); }
    }
    rebalance_cohorts(&mut files, shards);
    let mut counts = vec![0; shards];
    for f in &files { for t in &f.tests { counts[t.bucket] += 1; } }
    let plan = Plan { format_version: 2, shards, root: root.display().to_string(), test_count: counts.iter().sum(), bucket_counts: counts, files };
    // Validate every file before the first write; refuse concurrent source edits/symlinks.
    if args[0] == "apply" {
        for f in &plan.files {
            let p = root.join(&f.path);
            if p.canonicalize().map_err(|e| e.to_string())? != p || fs::symlink_metadata(&p).map_err(|e| e.to_string())?.file_type().is_symlink() || fs::read_to_string(&p).map_err(|e| e.to_string())? != f.source { return Err(format!("source changed before apply: {}", f.path)); }
        }
        for f in &plan.files { fs::write(root.join(&f.path), &f.instrumented).map_err(|e| e.to_string())?; }
    }
    println!("{}", serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?);
    Ok(())
}
fn main() { if let Err(e) = run() { eprintln!("{e}"); std::process::exit(1); } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_bytes_unicode_attrs_and_tokio_options() {
        let source = "// 한글 é\nmod nested {\n    /// doc\n    #[cfg(unix)]\n    #[ignore = \"manual\"]\n    #[tokio::test(start_paused = true)]\n    async fn original() { assert!(true); }\n}\n";
        let p = instrument("src-tauri/src/x.rs", source.into(), 8).unwrap();
        assert_eq!(p.tests.len(), 1); assert_eq!(p.tests[0].function, "nested::original");
        let t=&p.tests[0]; assert_eq!(&source[t.offset..t.offset+3], "///");
        let added=format!("#[cfg(any(not(ferryx_test_sharding), ferryx_test_shard = \"{}\"))]\n",t.bucket);
        assert_eq!(p.instrumented.replacen(&added,"",1), source);
        assert_eq!(&source[t.fn_offset..t.fn_offset+2], "fn");
    }
    #[test]
    fn independent_included_files_and_helpers() {
        let outer=instrument("src-tauri/src/a.rs", "include!(\"tests.rs\"); fn helper() {}".into(),8).unwrap();
        assert!(outer.tests.is_empty());assert_eq!(outer.source,outer.instrumented);
        let inner=instrument("src-tauri/src/tests.rs", "#[test]\nfn x() {}\nfn helper() {}".into(),8).unwrap();
        assert_eq!(inner.tests.len(),1);assert!(inner.instrumented.ends_with("fn helper() {}"));
    }
    #[test]
    fn rejects_unsupported_and_macro_generated_tests() {
        for src in ["#[rstest] fn x() {}", "#[test_log::test] fn x() {}", "#[cfg_attr(unix,test)] fn x() {}", "macro_rules! make { () => { #[test] fn x() {} } }", "#[test] impl X {}"] {
            assert!(instrument("x.rs",src.into(),8).is_err(),"{src}");
        }
    }
    #[test]
    fn rejects_reinstrumentation_and_keeps_determinism() {
        let a=instrument("x.rs","#[test] fn x() {} #[test] fn y() {}".into(),8).unwrap();
        let b=instrument("x.rs",a.source.clone(),8).unwrap();
        assert_eq!(a.instrumented,b.instrumented);
        assert!(instrument("x.rs",a.instrumented,8).is_err());
    }
    #[test]
    fn self_executable_parent_and_probe_remain_in_one_binary() {
        let source="#[test] fn parent() { let _ = std::env::current_exe(); } #[test] fn helper() {}";
        let plan=instrument("src-tauri/src/example.rs", source.into(), 8).unwrap();
        assert!(plan.cohort.is_some());
        assert_eq!(plan.tests[0].bucket,plan.tests[1].bucket);
    }
    #[test]
    fn cohort_balancing_is_deterministic_and_never_splits_a_file() {
        let mut files=vec![
            instrument("a.rs","// current_exe\n#[test] fn x() {} #[test] fn y() {}".into(),2).unwrap(),
            instrument("b.rs","// --exact\n#[test] fn z() {} #[test] fn w() {}".into(),2).unwrap(),
        ];
        rebalance_cohorts(&mut files,2);
        assert_ne!(files[0].tests[0].bucket,files[1].tests[0].bucket);
        for file in &files {
            assert_eq!(file.tests[0].bucket,file.tests[1].bucket);
            assert_eq!(file.instrumented_sha256,hash(file.instrumented.as_bytes()));
        }
        let before:Vec<_>=files.iter().map(|f|f.instrumented.clone()).collect();
        rebalance_cohorts(&mut files,2);
        assert_eq!(before,files.iter().map(|f|f.instrumented.clone()).collect::<Vec<_>>());
    }
    #[test]
    fn multiple_insertions_preserve_every_original_byte() {
        let source="// é first\n#[test] fn alpha() {}\nmod inner { #[test] fn beta() {} }\n";
        let p=instrument("x.rs",source.into(),8).unwrap();
        let mut restored=p.instrumented.clone();
        for t in &p.tests {
            let added=format!("#[cfg(any(not(ferryx_test_sharding), ferryx_test_shard = \"{}\"))]\n", t.bucket);
            restored=restored.replacen(&added,"",1);
        }
        assert_eq!(restored,source);
    }
    #[test]
    fn collects_library_path_modules_in_shared_test_support() {
        let unique=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root=std::env::temp_dir().join(format!("ferryx-shard-path-test-{}-{unique}",std::process::id()));
        fs::create_dir_all(root.join("src-tauri/src/remote")).unwrap();
        fs::create_dir_all(root.join("src-tauri/tests/support")).unwrap();
        fs::write(root.join("src-tauri/src/remote/server.rs"), "#[path = \"../../tests/support/shared.rs\"] mod shared;").unwrap();
        let shared=root.join("src-tauri/tests/support/shared.rs");
        fs::write(&shared, "#[test] fn shared_test() {} fn helper() {}").unwrap();
        let paths=collect_sources(&root).unwrap();
        assert_eq!(paths.len(),2); assert!(paths.contains(&shared));
        let plan=instrument("src-tauri/tests/support/shared.rs",fs::read_to_string(shared).unwrap(),8).unwrap();
        assert_eq!(plan.tests.len(),1);assert!(plan.instrumented.ends_with("fn helper() {}"));
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlink_files_and_directories() {
        use std::os::unix::fs::symlink;
        let unique=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root=std::env::temp_dir().join(format!("ferryx-shard-test-{}-{unique}",std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("real.rs"),"#[test] fn x() {} ").unwrap();
        symlink(root.join("real.rs"),root.join("alias.rs")).unwrap();
        assert!(collect(&root,&mut vec![]).is_err());
        fs::remove_file(root.join("alias.rs")).unwrap();
        symlink(&root,root.join("loop")).unwrap();
        assert!(collect(&root,&mut vec![]).is_err());
        fs::remove_file(root.join("loop")).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

}
