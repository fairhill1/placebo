//! Development supervisor. Cargo remains the compiler; this process owns its
//! rebuild loop and the running application. Browser reload lives in the app.
mod new;

use notify::{RecursiveMode, Watcher};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
    task::JoinHandle,
};

struct Options {
    kind: String,
    target: String,
    features: Option<String>,
}

enum Task {
    Dev(Options),
    New(PathBuf),
}

fn options() -> Result<Option<Task>, String> {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    let help = |arg: Option<&str>| matches!(arg, Some("--help" | "-h"));
    if first.is_none()
        || help(first.as_deref())
        || std::env::args()
            .nth(2)
            .as_deref()
            .is_some_and(|arg| help(Some(arg)))
    {
        println!(
            "Placebo development tools\n\n  placebo new PATH\n  placebo dev [--bin NAME | --example NAME] [--features FEATURES]\n\n`new` creates a starter app on Postgres, with an AGENTS.md of Placebo's rules.\nIts database is placebo_NAME on the local server.\n\n`dev` runs from the Cargo package directory. Without --bin or --example it runs\nthe package's binary; without --features it enables `dev`. Rust edits rebuild\nand restart the app; the app's Placebo dev layer handles browser reload. Ctrl-C stops."
        );
        return Ok(None);
    }
    if first.as_deref() == Some("new") {
        return match (args.next(), args.next()) {
            (Some(path), None) if !path.starts_with('-') => Ok(Some(Task::New(path.into()))),
            _ => Err("Expected `placebo new PATH`; use --help for usage.".into()),
        };
    }
    if first.as_deref() != Some("dev") {
        return Err("Expected `placebo dev` or `placebo new`; use --help for usage.".into());
    }
    let mut target = None;
    let mut features = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--example" | "--bin" if target.is_none() => {
                let name = args.next().ok_or("Missing target name.")?;
                if name.starts_with('-') {
                    return Err("Invalid target name.".into());
                }
                target = Some((arg, name));
            }
            "--features" => {
                features = Some(args.next().ok_or("Missing features value.")?);
            }
            _ => return Err(format!("Unexpected argument: {arg}")),
        }
    }
    let (kind, target) = match target {
        Some(target) => target,
        None => ("--bin".to_owned(), package_binary()?),
    };
    Ok(Some(Task::Dev(Options {
        kind,
        target,
        features: Some(features.unwrap_or_else(|| "dev".to_owned())),
    })))
}

/// The binary of the package in the current directory, when it has one.
fn package_binary() -> Result<String, String> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|error| format!("Could not run cargo metadata: {error}"))?;
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Run placebo dev from a Cargo package directory.".to_owned())?;
    let manifest = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join("Cargo.toml");
    let package = metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| package["manifest_path"].as_str().map(Path::new) == Some(&manifest))
        .ok_or("Run placebo dev from a Cargo package directory.")?;
    let binaries: Vec<&str> = package["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .filter_map(|target| target["name"].as_str())
        .collect();
    match binaries.as_slice() {
        [binary] => Ok((*binary).to_owned()),
        [] => Err("This package has no binary; choose one with --example NAME.".into()),
        _ => Err(format!(
            "This package has several binaries ({}); choose one with --bin NAME.",
            binaries.join(", ")
        )),
    }
}

fn is_rust_input(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if relative.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some("target" | ".git" | "node_modules")
        )
    }) {
        return false;
    }
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("Cargo.toml" | "Cargo.lock")
    ) || path.extension().is_some_and(|ext| ext == "rs")
        || relative == Path::new(".cargo/config.toml")
}

fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.kill_on_drop(true);
    // The child and its descendants are isolated from the supervisor's group.
    #[cfg(unix)]
    command.process_group(0);
    command
}

struct Build {
    child: Child,
    artifact: JoinHandle<io::Result<Option<PathBuf>>>,
}

fn build(options: &Options) -> io::Result<Build> {
    eprintln!("[placebo:build] Compiling {}", options.target);
    let mut command = command("cargo");
    command.args([
        "build",
        "--message-format=json-render-diagnostics",
        &options.kind,
        &options.target,
    ]);
    if let Some(features) = &options.features {
        command.args(["--features", features]);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let target = options.target.clone();
    let artifact = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut executable = None;
        while let Some(line) = lines.next_line().await? {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                match value["reason"].as_str() {
                    Some("compiler-artifact")
                        if value["target"]["name"].as_str() == Some(&target) =>
                    {
                        if let Some(path) = value["executable"].as_str() {
                            executable = Some(PathBuf::from(path));
                        }
                    }
                    Some("compiler-message") => {
                        if let Some(message) = value["message"]["rendered"].as_str() {
                            eprint!("{message}");
                        }
                    }
                    _ => {}
                }
            } else {
                eprintln!("{line}");
            }
        }
        Ok(executable)
    });
    Ok(Build { child, artifact })
}

async fn stop(child: &mut Child) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(id as i32),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    #[cfg(not(unix))]
    let _ = child.start_kill();
    if tokio::time::timeout(Duration::from_millis(1500), child.wait())
        .await
        .is_err()
    {
        #[cfg(unix)]
        if let Some(id) = child.id() {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(id as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = child.kill().await;
    }
}

async fn termination() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        signal.recv().await;
        Ok(())
    }
    #[cfg(not(unix))]
    std::future::pending().await
}

async fn supervise(options: Options) -> io::Result<()> {
    let root = std::env::current_dir()?.canonicalize()?;
    if !root.join("Cargo.toml").is_file() {
        return Err(io::Error::other(
            "Run placebo dev from a Cargo package directory.",
        ));
    }
    let (sender, mut events) = mpsc::unbounded_channel();
    let watch_root = root.clone();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event)
                if !event.kind.is_access()
                    && event
                        .paths
                        .iter()
                        .any(|path| is_rust_input(&watch_root, path)) =>
            {
                let _ = sender.send(());
            }
            Ok(_) => {}
            Err(error) => eprintln!("[placebo:watch-error] {error}"),
        })
        .map_err(io::Error::other)?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(io::Error::other)?;
    let mut server: Option<Child> = None;
    let mut building: Option<Build> = None;
    let mut dirty = true;
    let mut changed = Instant::now() - Duration::from_secs(1);
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let interrupt = tokio::signal::ctrl_c();
    let terminate = termination();
    tokio::pin!(interrupt, terminate);
    eprintln!("[placebo:watching] {}", root.display());

    // Cleanup runs for both normal shutdown and errors in the loop.
    let result: io::Result<()> = async {
        loop {
            tokio::select! {
                signal = &mut interrupt => { signal?; break; }
                signal = &mut terminate => { signal?; break; }
                Some(()) = events.recv() => { dirty = true; changed = Instant::now(); }
                _ = tick.tick() => {
                    if let Some(build) = &mut building
                        && let Some(status) = build.child.try_wait()? {
                            let finished = building.take().unwrap();
                            let artifact = finished.artifact.await.map_err(io::Error::other)??;
                            if status.success() {
                                let executable = artifact.ok_or_else(|| io::Error::other("Cargo did not report an executable for the selected target."))?;
                                if let Some(mut previous) = server.take() { stop(&mut previous).await; }
                                let next = command(&executable).spawn()?;
                                eprintln!("[placebo:ready] {} (pid {})", options.target, next.id().unwrap_or_default());
                                server = Some(next);
                            } else {
                                eprintln!("[placebo:build-failed] Fix the error and save. The last working server is unchanged.");
                            }
                    }
                    if let Some(child) = &mut server
                        && let Some(status) = child.try_wait()? {
                            eprintln!("[placebo:server-exited] {status}. Waiting for a Rust edit.");
                            server = None;
                    }
                    if dirty && building.is_none() && changed.elapsed() >= Duration::from_millis(150) {
                        dirty = false;
                        building = Some(build(&options)?);
                    }
                }
            }
        }
        Ok(())
    }.await;
    drop(watcher);
    if let Some(mut build) = building {
        stop(&mut build.child).await;
        build.artifact.abort();
    }
    if let Some(mut child) = server {
        stop(&mut child).await;
    }
    eprintln!("[placebo:stopped] Development processes stopped.");
    result
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = match options() {
        Ok(Some(Task::Dev(options))) => supervise(options).await,
        Ok(Some(Task::New(path))) => new::run(&path),
        Ok(None) => return,
        Err(error) => Err(io::Error::other(error)),
    };
    if let Err(error) = result {
        eprintln!("[placebo:error] {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_assets_and_build_outputs_do_not_trigger_recompilation() {
        let root = Path::new("/project");
        for path in [
            "src/main.rs",
            "examples/editors.rs",
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config.toml",
        ] {
            assert!(is_rust_input(root, &root.join(path)), "{path}");
        }
        for path in [
            "examples/static/demo.css",
            "client/placebo.js",
            "target/debug/build/generated.rs",
            "node_modules/example/index.rs",
        ] {
            assert!(!is_rust_input(root, &root.join(path)), "{path}");
        }
    }
}
