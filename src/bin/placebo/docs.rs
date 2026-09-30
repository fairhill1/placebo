//! `placebo rules` and `placebo kit`: Placebo's rules and the styles'
//! reference, from the Placebo the app in the current directory builds
//! against, so they match it after `cargo update -p placebo`. Outside an app,
//! or when Cargo cannot say, this CLI's own copies.
use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

pub const RULES: &str = include_str!("../../../docs/rules.md");
pub const KIT: &str = include_str!("../../../kit/README.md");

/// Print the rules, then where their docs are.
pub fn rules() -> io::Result<()> {
    let source = source();
    print!("{}", read(source.as_deref(), "docs/rules.md", RULES));
    let docs = match &source {
        Some(dir) => dir.join("docs").display().to_string(),
        None => "https://github.com/fairhill1/placebo/tree/main/docs".to_owned(),
    };
    println!(
        "\nThe docs explain each rule: `interactions.md` (replies, drafts, reads, live updates), \
         `typed-forms.md` (controls and payload types), and `diagnostics.md` (console codes), \
         in {docs}."
    );
    Ok(())
}

pub fn kit() -> io::Result<()> {
    print!("{}", read(source().as_deref(), "kit/README.md", KIT));
    Ok(())
}

fn read(source: Option<&Path>, file: &str, own: &str) -> String {
    source
        .and_then(|dir| std::fs::read_to_string(dir.join(file)).ok())
        .unwrap_or_else(|| own.to_owned())
}

/// The directory of the `placebo` package Cargo resolves for the current
/// directory: a clone, or Cargo's checkout of the repository.
pub fn source() -> Option<PathBuf> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let manifest = metadata["packages"]
        .as_array()?
        .iter()
        .find(|package| package["name"] == "placebo")?["manifest_path"]
        .as_str()?;
    Some(Path::new(manifest).parent()?.to_path_buf())
}
