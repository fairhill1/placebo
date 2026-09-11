pub mod check;
mod new;
use std::path::{Path, PathBuf};

pub fn run(args: &[String]) -> Result<bool, String> {
    match args.first().map(String::as_str) {
        Some("check") => {
            let mut root = PathBuf::from(".");
            let mut examples = false;
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--path" => root = rest.next().ok_or("Missing --path DIR.")?.into(),
                    "--examples" => examples = true,
                    "--help" | "-h" => {
                        println!(
                            "placebo check [--path DIR] [--examples]\n\nCheck src/**/*.rs (or examples/**/*.rs) for raw Placebo configuration\nand declared actions registered without typed route adapters. Exits 1\non findings, malformed exceptions, unsupported layout, or read/parse errors."
                        );
                        return Ok(true);
                    }
                    _ => return Err(format!("Unexpected check argument: {arg}")),
                }
            }
            let report = check::check(&root, examples)?;
            report.print();
            if !report.passed() {
                return Err("Project checks failed. Fix the reported bypasses, or document an intentional integration with a scoped placebo:allow exception.".into());
            }
            Ok(true)
        }
        Some("new") => {
            let mut destination = None;
            let mut name = None;
            let mut library = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--name" => name = Some(rest.next().ok_or("Missing --name NAME.")?.as_str()),
                    "--placebo-path" => {
                        library = rest.next().ok_or("Missing --placebo-path DIR.")?.into()
                    }
                    "--help" | "-h" => {
                        println!(
                            "placebo new DIR [--name NAME] [--placebo-path DIR]\n\nCreate a typed app in a new directory. Uses the CLI's source checkout\nas a relative path dependency unless --placebo-path is supplied. Never\noverwrites an existing directory. Does not install packages or start servers."
                        );
                        return Ok(true);
                    }
                    a if !a.starts_with('-') && destination.is_none() => {
                        destination = Some(Path::new(arg))
                    }
                    _ => return Err(format!("Unexpected new argument: {arg}")),
                }
            }
            new::create(
                destination.ok_or("Usage: placebo new DIR [--name NAME] [--placebo-path DIR]")?,
                name,
                &library,
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
