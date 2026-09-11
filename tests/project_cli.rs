#![cfg(feature = "dev")]

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "placebo-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_placebo"))
        .args(args)
        .output()
        .unwrap()
}

fn output(result: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
}

#[test]
fn starter_passes_bypasses_fail_and_exceptions_remain_visible() {
    let temp = Temp::new();
    let app = temp.0.join("app with spaces");
    let dir = app.to_str().unwrap();
    let generated = cli(&["new", dir, "--name", "test-app"]);
    assert!(generated.status.success(), "{}", output(&generated));
    let check = || cli(&["check", "--path", dir]);
    assert!(check().status.success());

    let source_path = app.join("src/main.rs");
    let source = fs::read_to_string(&source_path).unwrap();
    let typed = ".route(SAVE.path(), SAVE.route(save))";
    let raw = ".route(SAVE.path(), axum::routing::post(|| async { \"custom\" }))";
    fs::write(&source_path, source.replace(typed, raw)).unwrap();
    let bypass = check();
    assert!(!bypass.status.success());
    assert!(output(&bypass).contains("src/main.rs:"));
    assert!(output(&bypass).contains("[placebo:untyped-route]"));

    let reason = "Compatibility bridge verifies its own request contract.";
    let allowed = format!("// placebo:allow untyped-route -- {reason}\n        {raw}");
    fs::write(&source_path, source.replace(typed, &allowed)).unwrap();
    let exception = check();
    assert!(exception.status.success(), "{}", output(&exception));
    assert!(output(&exception).contains("[placebo:allowed untyped-route]"));
    assert!(output(&exception).contains(reason));

    fs::write(
        &source_path,
        source.replace(typed, &allowed.replace(raw, typed)),
    )
    .unwrap();
    assert!(output(&check()).contains("[placebo:unused-allow]"));

    let before = fs::read(&source_path).unwrap();
    let again = cli(&["new", dir, "--name", "test-app"]);
    assert!(!again.status.success());
    assert_eq!(
        fs::read(&source_path).unwrap(),
        before,
        "new must not overwrite an existing app"
    );
}

#[test]
fn invalid_layout_and_unreadable_syntax_never_report_a_clean_project() {
    let temp = Temp::new();
    fs::write(temp.0.join("Cargo.toml"), "[package]\nname=\"fixture\"\n").unwrap();
    let dir = temp.0.to_str().unwrap();
    assert!(!cli(&["check", "--path", dir]).status.success());
    fs::create_dir(temp.0.join("src")).unwrap();
    fs::write(temp.0.join("src/main.rs"), "not valid Rust;").unwrap();
    let bad = cli(&["check", "--path", dir]);
    assert!(!bad.status.success());
    assert!(output(&bad).contains("[placebo:syntax]"));
}

#[test]
fn generated_cargo_test_rejects_a_compilable_route_bypass() {
    let temp = Temp::new();
    let app = temp.0.join("checked-starter");
    let generated = cli(&["new", app.to_str().unwrap()]);
    assert!(generated.status.success(), "{}", output(&generated));
    let run = || {
        Command::new(env!("CARGO"))
            .args(["test", "--offline", "--test", "placebo_contract"])
            .current_dir(&app)
            // Reuse compilation between the two checks without colliding with
            // the running repository test binary or depending on a global CLI.
            .env(
                "CARGO_TARGET_DIR",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/starter-contract"),
            )
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap()
    };
    let clean = run();
    assert!(clean.status.success(), "{}", output(&clean));

    let path = app.join("src/main.rs");
    let source = fs::read_to_string(&path).unwrap();
    let typed = ".route(SAVE.path(), SAVE.route(save))";
    assert!(source.contains(typed));
    fs::write(
        path,
        source.replace(
            typed,
            ".route(SAVE.path(), axum::routing::post(|| async { \"bypass\" }))",
        ),
    )
    .unwrap();
    let bypass = run();
    let log = output(&bypass);
    assert!(
        !bypass.status.success(),
        "cargo test must run the contract test"
    );
    assert!(log.contains("[placebo:untyped-route]"), "{log}");
    assert!(log.contains("placebo_source_contract ... FAILED"), "{log}");
}
