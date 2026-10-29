use super::*;
use std::cell::Cell;

const CHILD_ENV: &str = "FERRYX_RUNTIME_STAGE_TEST_CHILD";

fn write_exe(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn file_source(executable: &Path) -> StageSource {
    StageSource::File {
        executable: executable.to_path_buf(),
        file_name: DAEMON_FILE_NAME.to_string(),
    }
}

fn dir_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A generation dir with an explicit staged-at value, so ordering never depends on the clock.
fn fake_generation(root: &Path, key: &str, staged_at: u128) -> PathBuf {
    let dir = root.join(key);
    write_exe(&dir.join(DAEMON_FILE_NAME), key.as_bytes());
    fs::write(dir.join(STAGED_AT_FILE), staged_at.to_string()).unwrap();
    dir
}

struct FakeUsage {
    running: Option<Vec<PathBuf>>,
    jobs: Vec<PathBuf>,
    running_calls: Cell<usize>,
}

impl FakeUsage {
    fn new(running: Option<Vec<PathBuf>>, jobs: Vec<PathBuf>) -> Self {
        Self {
            running,
            jobs,
            running_calls: Cell::new(0),
        }
    }
}

impl RuntimeUsage for FakeUsage {
    fn running_executables(&self) -> io::Result<Vec<PathBuf>> {
        self.running_calls.set(self.running_calls.get() + 1);
        self.running
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "process table unreadable"))
    }

    fn registered_job_executables(&self) -> Vec<PathBuf> {
        self.jobs.clone()
    }
}

#[test]
fn generation_key_is_version_and_first_eight_hex_of_the_hash() {
    let mut hash = [0u8; 32];
    hash[..5].copy_from_slice(&[0x0a, 0xbc, 0xde, 0xf1, 0xff]);
    assert_eq!(generation_key("2026.1013.2", &hash), "2026.1013.2-0abcdef1");
}

#[test]
fn runtime_root_prefers_ferryx_data_dir_for_isolated_qa() {
    let base = Some(PathBuf::from("base"));
    assert_eq!(
        runtime_root_from(Some(OsStr::new("qa-data")), base.clone()),
        Some(Path::new("qa-data").join(RUNTIME_DIR_NAME))
    );
    assert_eq!(
        runtime_root_from(Some(OsStr::new("")), base.clone()),
        Some(Path::new("base").join(RUNTIME_DIR_NAME))
    );
    assert_eq!(runtime_root_from(None, base), Some(Path::new("base").join(RUNTIME_DIR_NAME)));
    assert_eq!(runtime_root_from(None, None), None);
}

#[test]
fn staging_renames_a_complete_verified_copy_into_place() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("app").join(DAEMON_FILE_NAME);
    write_exe(&source, b"daemon build one");
    let root = temp.path().join("runtime");

    let staged = stage_from(&root, &file_source(&source), "1.0.0").unwrap();

    let expected_key = generation_key("1.0.0", &sha256_file(&source).unwrap());
    assert_eq!(staged.key, expected_key);
    assert_eq!(staged.executable, root.join(&expected_key).join(DAEMON_FILE_NAME));
    assert_eq!(fs::read(&staged.executable).unwrap(), b"daemon build one");
    assert!(root.join(&expected_key).join(STAGED_AT_FILE).is_file());
    // Only the final generation dir remains; the temp dir was renamed, not copied.
    assert_eq!(dir_names(&root), vec![expected_key]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&staged.executable).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    // The staged copy outlives the GUI install it came from.
    fs::remove_file(&source).unwrap();
    assert_eq!(fs::read(&staged.executable).unwrap(), b"daemon build one");
}

#[test]
fn staging_the_same_executable_twice_reuses_one_generation() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("app").join(DAEMON_FILE_NAME);
    write_exe(&source, b"same build");
    let root = temp.path().join("runtime");

    let first = stage_from(&root, &file_source(&source), "1.0.0").unwrap();
    let marker = fs::read_to_string(root.join(&first.key).join(STAGED_AT_FILE)).unwrap();
    let second = stage_from(&root, &file_source(&source), "1.0.0").unwrap();

    assert_eq!(first, second);
    assert_eq!(dir_names(&root), vec![first.key.clone()]);
    // Reuse never rewrites the generation.
    assert_eq!(fs::read_to_string(root.join(&first.key).join(STAGED_AT_FILE)).unwrap(), marker);
}

#[test]
fn two_builds_with_the_same_version_get_two_generations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    let build_a = temp.path().join("a").join(DAEMON_FILE_NAME);
    let build_b = temp.path().join("b").join(DAEMON_FILE_NAME);
    write_exe(&build_a, b"build a");
    write_exe(&build_b, b"build b");

    let a = stage_from(&root, &file_source(&build_a), "1.0.0").unwrap();
    let b = stage_from(&root, &file_source(&build_b), "1.0.0").unwrap();

    assert_ne!(a.key, b.key);
    assert!(a.key.starts_with("1.0.0-") && b.key.starts_with("1.0.0-"));
    assert_eq!(fs::read(&a.executable).unwrap(), b"build a");
    assert_eq!(fs::read(&b.executable).unwrap(), b"build b");
}

#[test]
fn an_existing_generation_with_other_contents_is_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("app").join(DAEMON_FILE_NAME);
    write_exe(&source, b"real build");
    let root = temp.path().join("runtime");
    let key = generation_key("1.0.0", &sha256_file(&source).unwrap());
    // A generation dir under the same key whose executable was tampered with.
    write_exe(&root.join(&key).join(DAEMON_FILE_NAME), b"tampered");

    let error = stage_from(&root, &file_source(&source), "1.0.0").unwrap_err();

    assert!(matches!(error, RuntimeStageError::ExistingGenerationDiffers { key: ref k } if *k == key));
    assert_eq!(fs::read(root.join(&key).join(DAEMON_FILE_NAME)).unwrap(), b"tampered");
    assert_eq!(dir_names(&root), vec![key]);
}

#[test]
fn a_missing_source_leaves_no_partial_generation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    let error = stage_from(&root, &file_source(&temp.path().join("absent")), "1.0.0").unwrap_err();
    assert!(matches!(error, RuntimeStageError::Io { .. }));
    assert!(!root.exists() || dir_names(&root).is_empty());
}

#[test]
fn gc_keeps_the_newest_two_and_removes_unused_older_generations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fake_generation(root, "1.0.0-00000001", 1);
    fake_generation(root, "1.0.0-00000002", 2);
    fake_generation(root, "1.0.0-00000003", 3);
    fake_generation(root, "1.0.0-00000004", 4);
    // An in-flight staging dir belongs to its stager, whatever its age.
    fs::create_dir(root.join("1.0.0-00000009.tmp-42-0")).unwrap();

    let mut removed = collect_garbage(root, &FakeUsage::new(Some(Vec::new()), Vec::new())).unwrap();

    removed.sort();
    assert_eq!(removed, vec!["1.0.0-00000001".to_string(), "1.0.0-00000002".to_string()]);
    assert_eq!(
        dir_names(root),
        vec!["1.0.0-00000003", "1.0.0-00000004", "1.0.0-00000009.tmp-42-0"]
    );
}

#[test]
fn gc_keeps_generations_used_by_a_process_or_a_registered_job() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let running = fake_generation(root, "1.0.0-00000001", 1);
    let registered = fake_generation(root, "1.0.0-00000002", 2);
    fake_generation(root, "1.0.0-00000003", 3);
    fake_generation(root, "1.0.0-00000004", 4);
    fake_generation(root, "1.0.0-00000005", 5);
    let usage = FakeUsage::new(
        Some(vec![running.join(DAEMON_FILE_NAME)]),
        vec![registered.join(DAEMON_FILE_NAME)],
    );

    let removed = collect_garbage(root, &usage).unwrap();

    assert_eq!(removed, vec!["1.0.0-00000003".to_string()]);
    assert!(running.is_dir() && registered.is_dir());
}

#[test]
fn gc_removes_nothing_when_the_process_table_is_unreadable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (index, key) in ["1.0.0-00000001", "1.0.0-00000002", "1.0.0-00000003"].iter().enumerate() {
        fake_generation(root, key, index as u128);
    }
    let usage = FakeUsage::new(None, Vec::new());

    assert!(collect_garbage(root, &usage).unwrap().is_empty());
    assert_eq!(usage.running_calls.get(), 1);
    assert_eq!(dir_names(root).len(), 3);
}

/// Child mode for [`gc_leaves_a_generation_with_a_running_process_intact`]: blocks until its stdin
/// closes, so the parent decides exactly when it exits. A plain run returns at once.
#[test]
fn runtime_stage_blocking_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let mut sink = Vec::new();
    let _ = io::Read::read_to_end(&mut io::stdin(), &mut sink);
}

#[test]
fn gc_leaves_a_generation_with_a_running_process_intact() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let in_use = fake_generation(root, "1.0.0-00000001", 1);
    let unused = fake_generation(root, "1.0.0-00000002", 2);
    fake_generation(root, "1.0.0-00000003", 3);
    fake_generation(root, "1.0.0-00000004", 4);
    // Run a real process from inside the oldest generation dir.
    let child_exe = in_use.join(DAEMON_FILE_NAME);
    fs::copy(std::env::current_exe().unwrap(), &child_exe).unwrap();
    let mut child = std::process::Command::new(&child_exe)
        .args([
            "--exact",
            "daemon::runtime_stage::tests::runtime_stage_blocking_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // spawn returns after the exec, so the process table already names the staged executable.
    let usage = SystemRuntimeUsage {
        registered_job_executables: Vec::new(),
    };

    let removed = collect_garbage(root, &usage);

    drop(child.stdin.take());
    let status = child.wait().unwrap();
    assert!(status.success());
    assert_eq!(removed.unwrap(), vec!["1.0.0-00000002".to_string()]);
    assert!(in_use.is_dir(), "a generation a live process runs from must survive GC");
    assert!(!unused.exists());
}

#[test]
fn macos_helper_source_reports_a_missing_helper_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let gui = temp.path().join("Ferryx.app/Contents/MacOS/ferryx");
    write_exe(&gui, b"gui");

    let error = macos_helper_source(&gui).unwrap_err();

    assert!(matches!(
        error,
        RuntimeStageError::HelperMissing { ref path }
            if *path == temp.path().join("Ferryx.app/Contents/Helpers").join(HELPER_BUNDLE_NAME)
    ));
}

fn write_helper_assets(dir: &Path) {
    write_exe(&dir.join("manifest.json"), b"{\"helperVersion\":\"1\"}");
    write_exe(&dir.join("x86_64-unknown-linux-gnu/ferryx-remote-helper"), b"linux helper");
    write_exe(&dir.join("x86_64-pc-windows-msvc/ferryx-remote-helper.exe"), b"windows helper");
}

#[test]
fn helper_assets_are_found_next_to_every_install_and_staged_layout() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let layouts = [
        // Windows install and Windows/Linux staged generation: `<dir>/helpers`.
        ("win/ferryx.exe", "win/helpers"),
        // macOS app bundle.
        ("Ferryx.app/Contents/MacOS/ferryx", "Ferryx.app/Contents/Resources/helpers"),
        // Linux deb or mounted AppImage.
        ("usr/bin/ferryx", "usr/lib/Ferryx/helpers"),
        // macOS staged generation: assets sit beside the sealed bundle.
        ("gen/Ferryx Daemon.app/Contents/MacOS/ferryx", "gen/helpers"),
    ];
    for (exe, assets) in layouts {
        write_exe(&root.join(exe), b"exe");
        write_helper_assets(&root.join(assets));
        let found = helper_assets_dir_near(&root.join(exe)).unwrap_or_else(|| panic!("no assets for {exe}"));
        assert_eq!(fs::canonicalize(found).unwrap(), fs::canonicalize(root.join(assets)).unwrap(), "{exe}");
    }
    write_exe(&root.join("bare/ferryx"), b"exe");
    assert_eq!(helper_assets_dir_near(&root.join("bare/ferryx")), None);
}

#[test]
fn helper_assets_are_copied_into_a_generation_and_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let assets = temp.path().join("app/helpers");
    write_helper_assets(&assets);
    // A generation staged before helper assets existed (the backfill case).
    let generation = fake_generation(&temp.path().join("runtime"), "1.0.0-00000001", 1);

    ensure_helper_assets(&generation, &assets).unwrap();

    let staged = generation.join(HELPER_ASSETS_DIR);
    assert_eq!(fs::read(staged.join("x86_64-unknown-linux-gnu/ferryx-remote-helper")).unwrap(), b"linux helper");
    assert_eq!(fs::read(staged.join("x86_64-pc-windows-msvc/ferryx-remote-helper.exe")).unwrap(), b"windows helper");
    // No temp dir is left behind next to the copy.
    let mut names = dir_names(&generation);
    names.retain(|name| name.contains(TMP_MARKER));
    assert!(names.is_empty(), "leftover temp dirs: {names:?}");
    // The staged daemon resolves its own copy, and a later app's assets never replace it.
    assert_eq!(helper_assets_dir_near(&generation.join(DAEMON_FILE_NAME)), Some(staged.clone()));
    write_exe(&assets.join("x86_64-unknown-linux-gnu/ferryx-remote-helper"), b"other build");
    ensure_helper_assets(&generation, &assets).unwrap();
    assert_eq!(fs::read(staged.join("x86_64-unknown-linux-gnu/ferryx-remote-helper")).unwrap(), b"linux helper");
}

#[cfg(unix)]
#[test]
fn staged_helper_binaries_stay_executable() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let assets = temp.path().join("app/helpers");
    write_helper_assets(&assets);
    let binary = "x86_64-unknown-linux-gnu/ferryx-remote-helper";
    fs::set_permissions(assets.join(binary), fs::Permissions::from_mode(0o755)).unwrap();
    let generation = fake_generation(&temp.path().join("runtime"), "1.0.0-00000001", 1);

    ensure_helper_assets(&generation, &assets).unwrap();

    let mode = fs::metadata(generation.join(HELPER_ASSETS_DIR).join(binary)).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
}

#[test]
fn a_missing_assets_source_leaves_no_partial_copy() {
    let temp = tempfile::tempdir().unwrap();
    let generation = fake_generation(&temp.path().join("runtime"), "1.0.0-00000001", 1);

    let error = ensure_helper_assets(&generation, &temp.path().join("absent")).unwrap_err();

    assert!(matches!(error, RuntimeStageError::Io { .. }));
    assert_eq!(dir_names(&generation), vec![STAGED_AT_FILE.to_string(), DAEMON_FILE_NAME.to_string()]);
}

#[test]
fn a_bundle_source_stages_the_whole_bundle_and_hashes_its_executable() {
    let temp = tempfile::tempdir().unwrap();
    let gui = temp.path().join("Ferryx.app/Contents/MacOS/ferryx");
    write_exe(&gui, b"gui");
    let helper = temp.path().join("Ferryx.app/Contents/Helpers").join(HELPER_BUNDLE_NAME);
    write_exe(&helper.join("Contents/MacOS").join(HELPER_EXECUTABLE), b"helper exe");
    fs::write(helper.join("Contents/Info.plist"), b"plist").unwrap();
    let source = macos_helper_source(&gui).unwrap();
    assert_eq!(
        source.staged_executable_relative(),
        Path::new(HELPER_BUNDLE_NAME).join("Contents/MacOS").join(HELPER_EXECUTABLE)
    );
    // Off macOS there is no signature to check, so the copy itself is observable everywhere.
    if cfg!(target_os = "macos") {
        return;
    }
    let root = temp.path().join("runtime");

    let staged = stage_from(&root, &source, "1.0.0").unwrap();

    let helper_hash = sha256_file(source.hashed_executable()).unwrap();
    assert_eq!(staged.key, generation_key("1.0.0", &helper_hash));
    assert_eq!(fs::read(&staged.executable).unwrap(), b"helper exe");
    assert_eq!(
        fs::read(root.join(&staged.key).join(HELPER_BUNDLE_NAME).join("Contents/Info.plist")).unwrap(),
        b"plist"
    );
}

#[cfg(target_os = "macos")]
mod macos_signature {
    use super::*;
    use std::process::Command;

    /// An ad-hoc signed helper bundle built from a copy of a real Mach-O executable.
    fn signed_helper(dir: &Path) -> PathBuf {
        let bundle = dir.join(HELPER_BUNDLE_NAME);
        let macos = bundle.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::copy("/usr/bin/true", macos.join(HELPER_EXECUTABLE)).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#,
                r#"<plist version="1.0"><dict>"#,
                r#"<key>CFBundleExecutable</key><string>ferryx</string>"#,
                r#"<key>CFBundleIdentifier</key><string>com.ferryx.app.daemon.test</string>"#,
                r#"<key>CFBundlePackageType</key><string>APPL</string>"#,
                r#"</dict></plist>"#,
            ),
        )
        .unwrap();
        let status = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&bundle)
            .status()
            .unwrap();
        assert!(status.success(), "ad-hoc signing the test helper failed");
        bundle
    }

    /// A byte inside the first architecture's load commands, which the code signature covers
    /// (a fat file's header padding is not covered by any slice signature).
    fn signed_byte_offset(bytes: &[u8]) -> usize {
        let fat_magic = [0xca, 0xfe, 0xba, 0xbe];
        if bytes[..4] == fat_magic {
            let first_slice = u32::from_be_bytes(bytes[16..20].try_into().unwrap()) as usize;
            return first_slice + 100;
        }
        100
    }

    fn flip_signed_byte(executable: &Path) {
        let mut bytes = fs::read(executable).unwrap();
        let offset = signed_byte_offset(&bytes);
        bytes[offset] ^= 0xff;
        fs::write(executable, bytes).unwrap();
    }

    fn bundle_source(bundle: &Path) -> StageSource {
        StageSource::Bundle {
            bundle: bundle.to_path_buf(),
            executable: bundle.join("Contents/MacOS").join(HELPER_EXECUTABLE),
        }
    }

    #[test]
    fn staged_helper_copy_passes_strict_codesign_verification() {
        let temp = tempfile::tempdir().unwrap();
        let helper = signed_helper(&temp.path().join("app"));
        let root = temp.path().join("runtime");

        let staged = stage_from(&root, &bundle_source(&helper), "1.0.0").unwrap();

        let staged_bundle = root.join(&staged.key).join(HELPER_BUNDLE_NAME);
        verify_signature(&staged_bundle).unwrap();
        assert_eq!(
            fs::read(&staged.executable).unwrap(),
            fs::read(helper.join("Contents/MacOS").join(HELPER_EXECUTABLE)).unwrap()
        );
        // Staging again reuses the verified generation.
        assert_eq!(stage_from(&root, &bundle_source(&helper), "1.0.0").unwrap(), staged);
    }

    #[test]
    fn a_flipped_byte_in_the_staged_executable_fails_with_signature_invalid() {
        let temp = tempfile::tempdir().unwrap();
        let helper = signed_helper(&temp.path().join("app"));
        let root = temp.path().join("runtime");
        let staged = stage_from(&root, &bundle_source(&helper), "1.0.0").unwrap();
        let staged_bundle = root.join(&staged.key).join(HELPER_BUNDLE_NAME);

        flip_signed_byte(&staged.executable);

        assert!(matches!(
            verify_signature(&staged_bundle),
            Err(RuntimeStageError::SignatureInvalid { ref path, .. }) if *path == staged_bundle
        ));
    }

    #[test]
    fn a_helper_with_a_broken_signature_is_never_staged() {
        let temp = tempfile::tempdir().unwrap();
        let helper = signed_helper(&temp.path().join("app"));
        flip_signed_byte(&helper.join("Contents/MacOS").join(HELPER_EXECUTABLE));
        let root = temp.path().join("runtime");

        let error = stage_from(&root, &bundle_source(&helper), "1.0.0").unwrap_err();

        assert!(matches!(error, RuntimeStageError::SignatureInvalid { .. }));
        // No generation dir and no leftover temp dir: there is no fallback to anything unsigned.
        assert!(dir_names(&root).is_empty());
    }
}

fn write_bundled_mhc(mhc: &Path, marker: &[u8]) -> PathBuf {
    let triple = mhc.join(ferryx_hub::meta::supervisor::host_triple());
    let binary = triple.join("mhc").join(if cfg!(windows) { "mhc.exe" } else { "mhc" });
    write_exe(&binary, marker);
    write_exe(&triple.join("mhc/skills/demo/SKILL.md"), b"skill");
    write_exe(&triple.join("bun").join(if cfg!(windows) { "bun.exe" } else { "bun" }), b"bun");
    binary
}

#[test]
fn bundled_mhc_is_staged_into_a_generation_and_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let app_mhc = temp.path().join("app/mhc");
    write_bundled_mhc(&app_mhc, b"mhc v1");
    let generation = fake_generation(&temp.path().join("runtime"), "1.0.0-00000001", 1);

    ensure_bundled_mhc(&generation, &app_mhc).unwrap();

    let staged_dir = generation.join(ferryx_hub::meta::supervisor::BUNDLED_MHC_DIR);
    let staged = ferryx_hub::meta::supervisor::bundled_mhc_in(&staged_dir).expect("staged mhc");
    assert_eq!(fs::read(&staged).unwrap(), b"mhc v1");
    let triple_dir = staged_dir.join(ferryx_hub::meta::supervisor::host_triple());
    assert_eq!(fs::read(triple_dir.join("mhc/skills/demo/SKILL.md")).unwrap(), b"skill");
    // The staged daemon resolves its own copy (Windows/Linux generation layout: `<key>/ferryx`).
    assert_eq!(
        ferryx_hub::meta::supervisor::bundled_mhc_dir_near(&generation.join(DAEMON_FILE_NAME)),
        Some(staged_dir.clone())
    );
    // No temp dir is left behind, and a later app's payload never replaces the staged one.
    let mut names = dir_names(&staged_dir);
    names.retain(|name| name.contains(TMP_MARKER));
    assert!(names.is_empty(), "leftover temp dirs: {names:?}");
    let replacement = temp.path().join("app2/mhc");
    write_bundled_mhc(&replacement, b"mhc v2");
    ensure_bundled_mhc(&generation, &replacement).unwrap();
    assert_eq!(fs::read(&staged).unwrap(), b"mhc v1");
}

#[test]
fn a_missing_mhc_payload_leaves_no_partial_copy() {
    let temp = tempfile::tempdir().unwrap();
    let generation = fake_generation(&temp.path().join("runtime"), "1.0.0-00000001", 1);

    let error = ensure_bundled_mhc(&generation, &temp.path().join("absent")).unwrap_err();

    assert!(matches!(error, RuntimeStageError::Io { .. }));
    let staged_dir = generation.join(ferryx_hub::meta::supervisor::BUNDLED_MHC_DIR);
    assert!(dir_names(&staged_dir).is_empty());
}
