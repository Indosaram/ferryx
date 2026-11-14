//! Live QA for todos 14 (ssh_config alias provenance) and 15 (auth prompt broker).
//!
//! Runs against a disposable sshd (see the harness in the task evidence) with a temporary HOME
//! that the real `ssh` resolves through the passwd database (nss_wrapper), so no `ssh -F` wrapper
//! is involved. This binary is its own askpass helper, exactly like the app.
//!
//! Env: FXQA_PORT, FXQA_PORT2 (second port to the same sshd), FXQA_KEY (passphrase-protected key),
//! FXQA_PASSPHRASE, FXQA_PW (pwuser password), FXQA_JUMP_PW (jumpuser password),
//! FXQA_OTP (comma-separated HOTP codes in order), FXQA_SECTION (14 | 15 | timeout | all).
use ferryx_lib::ssh::{
    self,
    password::{self, AuthPrompt, Password, PromptKind},
    SshAuthMethod, SshHost, SshHostSource,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn ssh_g(host: &SshHost) -> String {
    let plan = ssh::direct::ssh_plan(host, "true".into(), false).unwrap();
    let mut args = plan.args.clone();
    args.pop();
    args.insert(0, "-G".into());
    let out = std::process::Command::new(&plan.program).args(&args).output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| {
            ["hostname ", "port ", "user ", "hostkeyalias "].iter().any(|key| line.starts_with(key))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn target_argv(host: &SshHost) -> Vec<String> {
    let mut args = ssh::direct::ssh_plan(host, "true".into(), false).unwrap().args;
    args.pop();
    // Drop the fixed `-o` options; keep the endpoint part the decision is about.
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-o" {
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out
}

/// A UI stand-in on the real broker: records prompts and answers by kind.
struct Ui {
    seen: Arc<Mutex<Vec<AuthPrompt>>>,
}

fn register_ui(answer: impl Fn(&AuthPrompt) -> Option<String> + Send + Sync + 'static) -> Ui {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let answer = Arc::new(answer);
    password::prompts().set_listener(
        true,
        Arc::new(move |prompt: &AuthPrompt| {
            record.lock().unwrap().push(prompt.clone());
            println!(
                "QA prompt -> UI: kind={:?} host={} text={:?} expiresInMs={}",
                prompt.kind, prompt.host, prompt.prompt, prompt.expires_in_ms
            );
            if let Some(value) = answer(prompt) {
                let id = prompt.id.clone();
                std::thread::spawn(move || password::prompts().answer(&id, Some(Password::new(value))).unwrap());
            }
        }),
    );
    Ui { seen }
}

fn unregister_ui() {
    password::prompts().set_listener(false, Arc::new(|_: &AuthPrompt| {}));
}

fn host(id: &str, user: &str, port: u16, auth: SshAuthMethod) -> SshHost {
    SshHost {
        id: id.into(),
        label: id.into(),
        hostname: "127.0.0.1".into(),
        username: Some(user.into()),
        port: Some(port),
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: auth,
        disabled: None,
        system_config_alias: None,
    }
}

async fn summary(label: &str, host: &SshHost) -> ferryx_lib::ipc::ssh::SshTargetSummary {
    let started = Instant::now();
    let summary = ferryx_lib::ipc::ssh::ssh_test_connection_summary(host.clone()).await;
    println!(
        "QA {label}: reachable={} code={:?} reason={:?} elapsed={:.1}s",
        summary.reachable,
        summary.diagnostic.as_ref().map(|d| d.code.to_string()),
        summary.diagnostic.as_ref().and_then(|d| d.details.as_ref()).and_then(|d| d.get("reason")).cloned(),
        started.elapsed().as_secs_f64()
    );
    summary
}

async fn section_14(app: &tauri::AppHandle<tauri::test::MockRuntime>) {
    let port2: u16 = var("FXQA_PORT2").parse().unwrap();
    let passphrase = var("FXQA_PASSPHRASE");
    let ui = register_ui(move |p| (p.kind == PromptKind::Passphrase).then(|| passphrase.clone()));
    let home = std::path::PathBuf::from(var("HOME"));
    let system = std::fs::read_to_string(home.join(".ssh/config")).unwrap();

    // Default-config import through the real IPC command (path names ~/.ssh/config).
    let hosts = ferryx_lib::ipc::ssh::cmd_ssh_import_config(app.clone(), system.clone(), Some("~/.ssh/config".into()))
        .await
        .unwrap();
    let alias = hosts.iter().find(|h| h.label == "fxqa-alias").unwrap().clone();
    println!("QA 14 system import: config_alias={:?} argv={:?}", alias.config_alias(), target_argv(&alias));
    println!("QA 14 system import ssh -G: {}", ssh_g(&alias));
    assert_eq!(alias.config_alias(), Some("fxqa-alias"));
    assert!(summary("14 system-import alias (HostKeyAlias only in the block)", &alias).await.reachable);

    // Pasted import (no path) of a block that is NOT in ~/.ssh/config.
    // Second sshd port: a distinct endpoint from the stored alias host (import skips stored ones).
    let pasted_text = format!(
        "Host fxqa-pasted\n  HostName 127.0.0.1\n  Port {}\n  User keyuser\n  IdentityFile {}\n",
        port2,
        var("FXQA_KEY")
    );
    let hosts = ferryx_lib::ipc::ssh::cmd_ssh_import_config(app.clone(), pasted_text, None).await.unwrap();
    let pasted = hosts.iter().find(|h| h.label == "fxqa-pasted").unwrap().clone();
    println!("QA 14 pasted import: config_alias={:?} argv={:?}", pasted.config_alias(), target_argv(&pasted));
    let g = ssh_g(&pasted);
    println!("QA 14 pasted ssh -G: {g}");
    assert_eq!(pasted.config_alias(), None);
    assert!(g.contains("hostname 127.0.0.1") && g.contains(&format!("port {port2}")), "{g}");
    assert!(summary("14 pasted import", &pasted).await.reachable);

    // The system text pasted (no path) into a fresh store gets no provenance for the same alias.
    let data = std::env::var_os("FERRYX_DATA_DIR").unwrap();
    let fresh = std::path::PathBuf::from(&data).join("fresh");
    std::fs::create_dir_all(&fresh).unwrap();
    std::env::set_var("FERRYX_DATA_DIR", &fresh);
    let hosts = ferryx_lib::ipc::ssh::cmd_ssh_import_config(app.clone(), system.clone(), None).await.unwrap();
    std::env::set_var("FERRYX_DATA_DIR", &data);
    let repasted = hosts.iter().find(|h| h.label == "fxqa-alias").unwrap().clone();
    println!("QA 14 system text pasted: config_alias={:?} ssh -G: {}", repasted.config_alias(), ssh_g(&repasted));
    assert_eq!(repasted.config_alias(), None);

    // Edit the system-imported alias host through the real IPC update command.
    drop(system);
    let mut relabeled = alias.clone();
    relabeled.label = "renamed".into();
    let hosts = ferryx_lib::ipc::ssh::cmd_ssh_update_host(app.clone(), relabeled).await.unwrap();
    let relabeled = hosts.iter().find(|h| h.id == alias.id).unwrap().clone();
    println!("QA 14 relabel keeps alias: {:?}", relabeled.config_alias());
    assert_eq!(relabeled.config_alias(), Some("fxqa-alias"));
    // The edit dialog sends all fields; the user fills in the endpoint explicitly.
    let mut edited = relabeled.clone();
    edited.hostname = "127.0.0.1".into();
    edited.port = Some(port2);
    edited.identity_file = Some(var("FXQA_KEY"));
    let hosts = ferryx_lib::ipc::ssh::cmd_ssh_update_host(app.clone(), edited).await.unwrap();
    let edited = hosts.iter().find(|h| h.id == alias.id).unwrap().clone();
    let g = ssh_g(&edited);
    println!("QA 14 edited host: config_alias={:?} argv={:?}", edited.config_alias(), target_argv(&edited));
    println!("QA 14 edited ssh -G: {g}");
    assert_eq!(edited.config_alias(), None);
    assert!(g.contains(&format!("port {port2}")), "{g}");
    assert!(summary("14 edited host (port changed to second sshd port)", &edited).await.reachable);

    // Manual host unchanged: explicit flags.
    let mut manual = host("fxqa-manual", "keyuser", var("FXQA_PORT").parse().unwrap(), SshAuthMethod::Key);
    manual.identity_file = Some(var("FXQA_KEY"));
    println!("QA 14 manual argv: {:?}", target_argv(&manual));
    assert!(summary("14 manual host", &manual).await.reachable);

    let seen = ui.seen.lock().unwrap();
    println!("QA 14 prompts surfaced: {} (all passphrase: {})", seen.len(), seen.iter().all(|p| p.kind == PromptKind::Passphrase));
    drop(seen);
    unregister_ui();
}

async fn section_15() {
    let port: u16 = var("FXQA_PORT").parse().unwrap();
    let port2: u16 = var("FXQA_PORT2").parse().unwrap();
    let otp: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(var("FXQA_OTP").split(',').map(str::to_string).collect()));
    let passphrase = var("FXQA_PASSPHRASE");
    let jump_pw = var("FXQA_JUMP_PW");
    let codes = otp.clone();
    let ui = register_ui(move |p| match p.kind {
        PromptKind::Passphrase => Some(passphrase.clone()),
        PromptKind::Code => codes.lock().unwrap().pop_front(),
        PromptKind::Password if p.host.starts_with("jumpuser@") => Some(jump_pw.clone()),
        _ => None,
    });

    // Happy 1: passphrase-protected key.
    let mut key_host = host("fxqa-key", "keyuser", port, SshAuthMethod::Key);
    key_host.identity_file = Some(var("FXQA_KEY"));
    let before = ui.seen.lock().unwrap().len();
    assert!(summary("15 passphrase key", &key_host).await.reachable);
    let kinds: Vec<_> = ui.seen.lock().unwrap()[before..].iter().map(|p| p.kind).collect();
    println!("QA 15 passphrase prompts: {kinds:?}");
    assert!(!kinds.is_empty() && kinds.iter().all(|k| *k == PromptKind::Passphrase));

    // Happy 2: keyboard-interactive OTP (PAM pam_oath, HOTP).
    let otp_host = host("fxqa-otp", "otpuser", port, SshAuthMethod::Agent);
    let before = ui.seen.lock().unwrap().len();
    assert!(summary("15 keyboard-interactive OTP", &otp_host).await.reachable);
    let kinds: Vec<_> = ui.seen.lock().unwrap()[before..].iter().map(|p| p.kind).collect();
    println!("QA 15 OTP prompts: {kinds:?}");
    assert!(!kinds.is_empty() && kinds.iter().all(|k| *k == PromptKind::Code));

    // Happy 3: stored password answered over keyboard-interactive (PAM), never shown to the UI.
    let pw_host = host("fxqa-pw", "pwuser", port, SshAuthMethod::Password);
    password::set(&pw_host, Password::new(var("FXQA_PW"))).unwrap();
    let before = ui.seen.lock().unwrap().len();
    assert!(summary("15 stored password via keyboard-interactive", &pw_host).await.reachable);
    println!("QA 15 stored-password prompts surfaced: {}", ui.seen.lock().unwrap().len() - before);
    assert_eq!(ui.seen.lock().unwrap().len(), before);

    // Routing: target password stored; the jump host's password prompt goes to the UI.
    let mut via_jump = host("fxqa-jump", "pwuser", port, SshAuthMethod::Password);
    via_jump.jump_host = Some(format!("jumpuser@127.0.0.1:{port2}"));
    password::set(&via_jump, Password::new(var("FXQA_PW"))).unwrap();
    let before = ui.seen.lock().unwrap().len();
    assert!(summary("15 jump host + stored target password", &via_jump).await.reachable);
    let routed: Vec<_> = ui.seen.lock().unwrap()[before..].iter().map(|p| (p.kind, p.host.clone())).collect();
    println!("QA 15 jump prompts surfaced: {routed:?}");
    assert!(!routed.is_empty() && routed.iter().all(|(_, host)| host.starts_with("jumpuser@")));
    unregister_ui();

    // Failure: no prompt UI registered -> fails at once, no hang.
    let started = Instant::now();
    let s = summary("15 OTP with no prompt UI", &otp_host).await;
    assert!(!s.reachable);
    assert!(started.elapsed() < Duration::from_secs(30));
}

async fn section_timeout() {
    let port: u16 = var("FXQA_PORT").parse().unwrap();
    // A UI that shows the prompt but never answers.
    let ui = register_ui(|_| None);
    let started = Instant::now();
    let s = summary("15 OTP prompt never answered", &host("fxqa-otp", "otpuser", port, SshAuthMethod::Agent)).await;
    let elapsed = started.elapsed();
    assert!(!s.reachable);
    let diagnostic = s.diagnostic.unwrap();
    println!("QA 15 timeout diagnostic: code={} message={:?} details={:?}", diagnostic.code, diagnostic.message, diagnostic.details);
    assert_eq!(diagnostic.code.to_string(), "TIMEOUT");
    assert_eq!(diagnostic.details.unwrap()["reason"], "prompt_timeout");
    assert!(elapsed >= password::PROMPT_TIMEOUT && elapsed < password::PROMPT_TIMEOUT + Duration::from_secs(30), "{elapsed:?}");
    println!("QA 15 prompts surfaced before timeout: {}", ui.seen.lock().unwrap().len());
    unregister_ui();
}

fn main() {
    if let Some(code) = password::run_askpass() {
        std::process::exit(code);
    }
    let section = std::env::var("FXQA_SECTION").unwrap_or_else(|_| "all".into());
    let app = tauri::test::mock_app();
    let handle = app.handle().clone();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        if section == "14" || section == "all" {
            section_14(&handle).await;
            println!("QA SECTION 14 OK");
        }
        if section == "15" || section == "all" {
            section_15().await;
            println!("QA SECTION 15 OK");
        }
        if section == "timeout" || section == "all" {
            section_timeout().await;
            println!("QA SECTION TIMEOUT OK");
        }
    });
}
