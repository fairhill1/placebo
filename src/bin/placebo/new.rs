//! `placebo new [PATH]`: a starter app on Postgres, copied from template/, a
//! workspace member that the repository builds and tests like any other crate.
use std::{fs, io, path::Path};

/// Where this CLI was built from.
const PLACEBO: &str = env!("CARGO_MANIFEST_DIR");
const REPO: &str = "https://github.com/fairhill1/placebo";
const MANIFEST: &str = include_str!("../../../template/Cargo.toml");

/// Template files copied as they are. The kit is not among them: Placebo
/// serves it, so it updates with the crate.
const FILES: [(&str, &str); 8] = [
    ("src/main.rs", include_str!("../../../template/src/main.rs")),
    // `sqlx::migrate!` needs the directory before the first migration.
    ("migrations/.gitkeep", ""),
    (
        "tests/styles.rs",
        include_str!("../../../template/tests/styles.rs"),
    ),
    (
        "static/app.css",
        include_str!("../../../template/static/app.css"),
    ),
    (
        "static/components.css",
        include_str!("../../../template/static/components.css"),
    ),
    (
        "static/app.js",
        include_str!("../../../template/static/app.js"),
    ),
    (".gitignore", include_str!("../../../template/.gitignore")),
    ("CLAUDE.md", "@AGENTS.md\n"),
];

pub fn run(path: &Path) -> io::Result<()> {
    // An existing directory, such as `.`, is set up in place, like `cargo init`.
    let path = if path.exists() {
        path.canonicalize()?
    } else {
        std::path::absolute(path)?
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("Name the app's directory, as in `placebo new my-app`."))?;
    check_name(name).map_err(io::Error::other)?;
    if path.exists() && !path.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory.",
            path.display()
        )));
    }
    let manifest = manifest(name, PLACEBO);
    let agents = agents(name);
    let generated = [
        ("Cargo.toml", manifest.as_str()),
        ("AGENTS.md", agents.as_str()),
    ];
    let files: Vec<_> = FILES.into_iter().chain(generated).collect();
    let taken: Vec<&str> = files
        .iter()
        .map(|(file, _)| *file)
        .filter(|file| path.join(file).exists())
        .collect();
    if !taken.is_empty() {
        return Err(io::Error::other(format!(
            "{} already has {}; start in an empty directory.",
            path.display(),
            taken.join(", ")
        )));
    }
    for (file, contents) in files {
        let target = path.join(file);
        fs::create_dir_all(target.parent().expect("a file has a parent"))?;
        fs::write(target, contents)?;
    }
    let here = std::env::current_dir()?.canonicalize()? == path;
    println!(
        "Created {name} with database {}.\n\n{}  placebo dev\n\nThe first run creates the database on your local Postgres.",
        database(name),
        if here {
            String::new()
        } else {
            format!("  cd {}\n", path.display())
        }
    );
    Ok(())
}

/// A CLI installed with `cargo install --git` was built in Cargo's cache
/// (`$CARGO_HOME/git/checkouts`), which Cargo may clean, so its apps depend on
/// the repository instead. One built from a clone depends on that clone.
fn from_git(placebo: &str) -> bool {
    let parts: Vec<_> = Path::new(placebo).iter().collect();
    parts
        .windows(2)
        .any(|pair| pair[0] == "git" && pair[1] == "checkouts")
}

/// Lowercase so the package, its database, and `\l placebo_*` all agree.
fn check_name(name: &str) -> Result<(), String> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "`{name}` cannot name an app: start with a lowercase letter and use only \
             lowercase letters, digits, `-`, and `_`."
        ))
    }
}

/// The name `database()` in the template's main.rs derives from the package.
fn database(name: &str) -> String {
    format!("placebo_{}", name.replace('-', "_"))
}

fn manifest(name: &str, placebo: &str) -> String {
    let dependency = if from_git(placebo) {
        format!("placebo = {{ git = {REPO:?}, features = [\"time\"] }}")
    } else {
        format!("placebo = {{ path = {placebo:?}, features = [\"time\"] }}")
    };
    let manifest = MANIFEST
        .replacen("name = \"starter\"", &format!("name = \"{name}\""), 1)
        .replacen("placebo = { path = \"..\", features = [\"time\"] }", &dependency, 1);
    // A standalone app is its own workspace, not a member of Placebo's.
    manifest.replacen("[features]", "[workspace]\n\n[features]", 1)
}

fn agents(name: &str) -> String {
    let database = database(name);
    format!(
        "# {name}

A Placebo app: Rust, Axum, Maud, and Postgres.

## Placebo's rules

Run `placebo rules` before changing any code, and follow them: they say how
pages, forms, replies, and styles work here, and where the docs are. Run
`placebo kit` for the CSS kit's tokens and classes. Both print the Placebo
this app builds against, so they stay current when it updates.

## Commands

- `placebo dev` builds and runs the app on http://127.0.0.1:3000. Rust edits
  rebuild and restart it; edits in `static/` reload the browser. Run the app
  with it, not `cargo run`, which does neither. Run it from this directory,
  which the app serves `static/` from. Beside another app on port 3000, run
  `PLACEBO_ADDR=127.0.0.1:3001 placebo dev`.
- `cargo test` runs the tests, including the styles test.
- `cargo update -p placebo` takes Placebo's updates: its Rust, its script,
  its kit, and its rules. Run `cargo test` after it. `cargo install --git
  {REPO} placebo --features dev --locked` updates the `placebo` command.

## Database

- The app uses the Postgres database `{database}` on the local server and
  creates it on the first debug run. `DATABASE_URL` names another; `PGUSER`
  and `PGPASSWORD` sign in as another role, as on Windows.
- Change the schema with a new numbered file in `migrations/`, such as
  `0001_projects.sql`; it runs when the app starts. Never edit a migration
  that has run.
- Query with `sqlx::query_as` and `.bind` parameters; never format values into
  SQL. Check a version in the same statement as its write:
  `UPDATE … SET …, version = version + 1 WHERE id = $1 AND version = $2 RETURNING …`.
- Remove the database with `dropdb {database}` when you delete the app.

## The shell

`src/main.rs` is the app's shell: the database, `layout()` with the sidebar,
and the theme. Every page renders with `layout()`; a page in the sidebar also
gets a row in `pages()`. Home and Settings are the first two pages: replace
Home's content with the app's own, and keep Settings for the app's settings.
More modules go beside it, such as `src/projects.rs` with its own routes.

## Styles

The kit is served by Placebo at `/placebo/kit/`, and `static/app.css` imports
it. The app's own components go in `static/components.css`, built from the
kit's tokens; `cargo test` fails on values off the kit's scale.
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_must_be_lowercase_package_names() {
        for name in ["crm", "my-app", "app_2"] {
            assert_eq!(check_name(name), Ok(()), "{name}");
        }
        for name in ["MyApp", "2app", "-app", "my app", "app.rs", ""] {
            assert!(check_name(name).is_err(), "{name}");
        }
        assert_eq!(database("my-app"), "placebo_my_app");
    }

    const CLONE: &str = "/home/someone/placebo";
    const CACHED: &str = "/home/someone/.cargo/git/checkouts/placebo-1a2b3c/275b40c";

    #[test]
    fn a_clone_is_a_path_dependency_and_a_git_install_the_repository() {
        let clone = manifest("my-app", CLONE);
        assert!(clone.contains("name = \"my-app\""), "{clone}");
        assert!(clone.contains("placebo = { path = \"/home/someone/placebo\", features = [\"time\"] }"));
        assert!(clone.contains("[workspace]"));
        assert!(!clone.contains("starter") && !clone.contains("\"..\""));
        let cached = manifest("my-app", CACHED);
        assert!(
            cached.contains(&format!("placebo = {{ git = {REPO:?}, features = [\"time\"] }}")),
            "{cached}"
        );
        assert!(!cached.contains(".cargo"));
    }

    #[test]
    fn agents_md_sends_agents_to_the_current_rules() {
        let agents = agents("my-app");
        assert!(agents.contains("`placebo rules`") && agents.contains("`placebo kit`"));
        assert!(agents.contains("`placebo_my_app`"));
    }

    #[test]
    fn an_empty_directory_is_set_up_in_place_and_a_used_one_refused() {
        let dir = std::env::temp_dir().join(format!("placebo-new-{}", std::process::id()));
        let app = dir.join("my-app");
        fs::create_dir_all(&app).unwrap();
        run(&app).unwrap();
        assert!(app.join("Cargo.toml").is_file() && app.join("static/app.css").is_file());
        let error = run(&app).unwrap_err().to_string();
        assert!(error.contains("already has src/main.rs"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
