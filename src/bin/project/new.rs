use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const APP: &str = include_str!("template/main.rs.txt");
const CSS: &str = include_str!("template/app.css");
const CONTRACT: &str = include_str!("template/placebo_contract.rs.txt");

fn relative(from: &Path, to: &Path) -> PathBuf {
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return to.to_path_buf();
    }
    let mut path = PathBuf::new();
    for part in &a[common..] {
        if matches!(part, Component::Normal(_)) {
            path.push("..");
        }
    }
    for part in &b[common..] {
        path.push(part.as_os_str());
    }
    if path.as_os_str().is_empty() {
        path.push(".");
    }
    path
}

fn valid_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && syn::parse_str::<syn::Ident>(&name.replace('-', "_")).is_ok()
}

pub fn create(destination: &Path, name: Option<&str>, library: &Path) -> Result<(), String> {
    if destination.try_exists().map_err(|e| e.to_string())? {
        return Err(format!(
            "{} already exists. Choose a new directory; existing files are never overwritten.",
            destination.display()
        ));
    }
    let name = name
        .or_else(|| destination.file_name().and_then(|s| s.to_str()))
        .ok_or("Choose a package name with --name NAME.")?;
    if !valid_name(name) {
        return Err("Package name must start with a letter and contain only ASCII letters, digits, '-' or '_', and must not be a Rust keyword.".into());
    }
    let library = library.canonicalize().map_err(|e| {
        format!(
            "Placebo source path {}: {e}. Use --placebo-path DIR if the checkout moved.",
            library.display()
        )
    })?;
    if !library.join("Cargo.toml").is_file() || !library.join("src/placebo.rs").is_file() {
        return Err("--placebo-path must point to the Placebo source checkout.".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("Destination parent {}: {e}", parent.display()))?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or("Choose a new app directory.")?,
    );
    let dependency = relative(&destination, &library)
        .to_string_lossy()
        .replace('\\', "/");
    let package_name = serde_json::to_string(name).unwrap();
    let dependency_path = serde_json::to_string(&dependency).unwrap();
    let manifest = format!(
        r#"[package]
name = {package_name}
version = "0.1.0"
edition = "2024"

# Keep this app independent if generated inside another Cargo workspace.
[workspace]

[features]
dev = ["placebo/dev"]

[dependencies]
placebo = {{ path = {dependency_path} }}
axum = "0.8.9"
maud = {{ version = "0.27.0", features = ["axum"] }}
serde = {{ version = "1.0.229", features = ["derive"] }}
tokio = {{ version = "1.53.1", features = ["macros", "rt-multi-thread", "net"] }}
"#
    );
    let library_manifest = format!("{dependency}/Cargo.toml");
    let base = vec![
        "run",
        "--manifest-path",
        &library_manifest,
        "--features",
        "dev",
        "--bin",
        "placebo",
        "--",
    ];
    let mut dev = base.clone();
    dev.extend(["dev", "--bin", name, "--features", "dev"]);
    let mut check = base;
    check.push("check");
    let aliases = format!(
        "[alias]\ndev = {}\ncheck-placebo = {}\n",
        serde_json::to_string(&dev).unwrap(),
        serde_json::to_string(&check).unwrap()
    );
    let readme = format!(
        r#"# {name}

A typed Placebo app with two independent editors, validation, draft retention,
normalized saves, and atomic version checks. Data resets when the process restarts.

Run `cargo dev`, then open the printed URL (default http://127.0.0.1:3000).
Rust edits are checked and rebuilt; static CSS changes reload the browser.
`PLACEBO_ADDR=127.0.0.1:0 cargo dev` selects a free port. `cargo run` starts the app
without the development supervisor. Production builds should omit `--features dev`.

## Check locally and in CI

```sh
cargo check-placebo
cargo test
```

`cargo check-placebo` exits nonzero for detected Placebo API bypasses. The dev
supervisor also runs this check before each build, keeping the previous server
if it fails. The generated `tests/placebo_contract.rs` invokes the check during
ordinary `cargo test` too. Its first run may compile the CLI's development
dependencies; release application builds do not acquire those dependencies.
`cargo build` and `cargo run` alone do not run source checks. Keep the contract
test and aliases; editing instructions or suppressing diagnostics is not verification.

Placebo is currently a local path dependency at `{dependency}`. CI needs that
checkout at the same relative location as your app, just as Cargo does. The
aliases invoke its CLI directly; no global Placebo installation is needed after
generation. Adjust both Cargo.toml and .cargo/config.toml if the checkout moves.

Read `src/main.rs` for the typed form/action/handler pattern and `AGENTS.md` for
contributor instructions. See the Placebo checkout's `docs/project-checks.md`
for check coverage and explicit, reasoned exceptions. These checks are not full
Rust name resolution and cannot establish browser correctness.

Before declaring an interaction complete, test it in a browser: invalid input,
save with another unsaved editor, a stale save from another tab, and retry.
Compilation and endpoint checks alone do not verify those flows.
"#
    );
    let instructions = include_str!("template/AGENTS.md.txt");
    let files = [
        ("Cargo.toml", manifest.as_str()),
        ("src/main.rs", APP),
        ("tests/placebo_contract.rs", CONTRACT),
        ("static/app.css", CSS),
        (".cargo/config.toml", aliases.as_str()),
        ("README.md", readme.as_str()),
        ("AGENTS.md", instructions),
        (".gitignore", "/target/\n"),
    ];
    fs::create_dir(&destination).map_err(|e| format!("{}: {e}", destination.display()))?;
    for (file, contents) in files {
        let path = destination.join(file);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let directory = format!(
        "'{}'",
        destination.display().to_string().replace('\'', "'\\''")
    );
    println!(
        "Created {}.\n\n  cd {}\n  cargo dev\n\nBefore committing / in CI:\n  cargo check-placebo\n  cargo test",
        name, directory
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_paths_and_names() {
        assert_eq!(
            relative(Path::new("/work/app"), Path::new("/work/placebo")),
            Path::new("../placebo")
        );
        assert!(valid_name("my-app"));
        assert!(!valid_name("../bad"));
        assert!(!valid_name("type"));
        assert!(!valid_name("3app"));
    }
}
