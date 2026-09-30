//! `placebo css`: the app's stylesheet, `styles/app.css`, compiled by
//! Tailwind's standalone CLI into `static/app.css`. The input imports
//! Tailwind and Basecoat, vendored in Placebo's kit,
//! which this copies into `.placebo/kit` from the Placebo the app builds
//! against, so it updates with `cargo update -p placebo`.
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// The Tailwind this Placebo is tested with.
const TAILWIND: &str = "v4.3.3";
pub const INPUT: &str = "styles/app.css";
const OUTPUT: &str = "static/app.css";

/// Build once, as before a release build: `placebo css --minify`.
pub fn run(minify: bool) -> io::Result<()> {
    let root = std::env::current_dir()?;
    let status = Command::new(prepare(&root)?)
        .args(arguments(minify, false))
        .current_dir(&root)
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!("Tailwind failed: {status}.")));
    }
    Ok(())
}

/// The Tailwind binary, with Basecoat in place, and its arguments; `placebo
/// dev` runs it with `--watch` beside the app.
pub fn watch_command(root: &Path) -> io::Result<(PathBuf, Vec<&'static str>)> {
    Ok((prepare(root)?, arguments(false, true)))
}

fn arguments(minify: bool, watch: bool) -> Vec<&'static str> {
    let mut arguments = vec!["--input", INPUT, "--output", OUTPUT];
    if minify {
        arguments.push("--minify");
    }
    if watch {
        // `always`: keep watching though the supervisor gives it no stdin.
        arguments.push("--watch=always");
    }
    arguments
}

fn prepare(root: &Path) -> io::Result<PathBuf> {
    if !root.join(INPUT).is_file() {
        return Err(io::Error::other(format!(
            "No {INPUT} here; run placebo css from the app's directory."
        )));
    }
    let placebo = crate::docs::source()
        .or_else(|| Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))))
        .filter(|dir| dir.join("kit/basecoat/basecoat.css").is_file())
        .ok_or_else(|| io::Error::other("Could not find Placebo's kit; run `cargo fetch`."))?;
    let kit = root.join(".placebo/kit");
    if kit.exists() {
        fs::remove_dir_all(&kit)?;
    }
    copy(&placebo.join("kit"), &kit)?;
    tailwind()
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else if entry.path().extension().is_some_and(|ext| ext == "css") {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// `PLACEBO_TAILWINDCSS`, or the release this Placebo pins, downloaded once
/// into the user's cache.
fn tailwind() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("PLACEBO_TAILWINDCSS") {
        return Ok(path.into());
    }
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
        .ok_or_else(|| io::Error::other("No cache directory; set PLACEBO_TAILWINDCSS."))?
        .join("placebo");
    let binary = cache.join(format!(
        "tailwindcss-{TAILWIND}{}",
        std::env::consts::EXE_SUFFIX
    ));
    if binary.is_file() {
        return Ok(binary);
    }
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "tailwindcss-linux-x64",
        ("linux", "aarch64") => "tailwindcss-linux-arm64",
        ("macos", "x86_64") => "tailwindcss-macos-x64",
        ("macos", "aarch64") => "tailwindcss-macos-arm64",
        ("windows", "x86_64") => "tailwindcss-windows-x64.exe",
        (os, arch) => {
            return Err(io::Error::other(format!(
                "No Tailwind build for {os} {arch}; set PLACEBO_TAILWINDCSS to a tailwindcss binary."
            )));
        }
    };
    fs::create_dir_all(&cache)?;
    let url =
        format!("https://github.com/tailwindlabs/tailwindcss/releases/download/{TAILWIND}/{asset}");
    eprintln!(
        "[placebo:tailwind] Downloading Tailwind {TAILWIND} into {}",
        cache.display()
    );
    let partial = binary.with_extension("partial");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(&url)
        .status()
        .map_err(|error| io::Error::other(format!("Could not run curl for {url}: {error}")))?;
    if !status.success() {
        return Err(io::Error::other(format!("Could not download {url}.")));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&partial, &binary)?;
    Ok(binary)
}
