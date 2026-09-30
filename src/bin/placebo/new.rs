//! `placebo new [PATH]`: a starter app on Postgres, copied from template/, a
//! workspace member that the repository builds and tests like any other crate.
use std::{fs, io, path::Path};

/// Where this CLI was built from.
const PLACEBO: &str = env!("CARGO_MANIFEST_DIR");
const REPO: &str = "https://github.com/fairhill1/placebo";
const RULES: &str = include_str!("../../../docs/rules.md");
const MANIFEST: &str = include_str!("../../../template/Cargo.toml");

/// Template files copied as they are. The kit files match the kit this
/// Placebo ships, which the app's styles test requires.
const FILES: [(&str, &str); 15] = [
    ("src/main.rs", include_str!("../../../template/src/main.rs")),
    (
        "src/tasks.rs",
        include_str!("../../../template/src/tasks.rs"),
    ),
    (
        "migrations/0001_tasks.sql",
        include_str!("../../../template/migrations/0001_tasks.sql"),
    ),
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
    ("static/kit/main.css", include_str!("../../../kit/main.css")),
    (
        "static/kit/tokens.css",
        include_str!("../../../kit/tokens.css"),
    ),
    (
        "static/kit/reset.css",
        include_str!("../../../kit/reset.css"),
    ),
    ("static/kit/base.css", include_str!("../../../kit/base.css")),
    (
        "static/kit/layout.css",
        include_str!("../../../kit/layout.css"),
    ),
    (
        "static/kit/components.css",
        include_str!("../../../kit/components.css"),
    ),
];
const KIT_README: &str = include_str!("../../../kit/README.md");

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
    let agents = agents(name, PLACEBO);
    let generated = [
        ("Cargo.toml", manifest.as_str()),
        ("AGENTS.md", agents.as_str()),
        ("static/kit/README.md", KIT_README),
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
        format!("placebo = {{ git = {REPO:?} }}")
    } else {
        format!("placebo = {{ path = {placebo:?} }}")
    };
    let manifest = MANIFEST
        .replacen("name = \"starter\"", &format!("name = \"{name}\""), 1)
        .replacen("placebo = { path = \"..\" }", &dependency, 1);
    // A standalone app is its own workspace, not a member of Placebo's.
    manifest.replacen("[features]", "[workspace]\n\n[features]", 1)
}

fn agents(name: &str, placebo: &str) -> String {
    let database = database(name);
    let docs = if from_git(placebo) {
        format!("{REPO}/tree/main/docs")
    } else {
        format!("{placebo}/docs")
    };
    format!(
        "# {name}

A Placebo app: Rust, Axum, Maud, and Postgres. Follow the rules below. The
docs explain each one: `interactions.md` (replies, drafts, reads, live
updates), `typed-forms.md` (controls and payload types), and `diagnostics.md`
(console codes), at {docs}.

## Commands

- `placebo dev` builds and runs the app on http://127.0.0.1:3000. Rust edits
  rebuild and restart it; edits in `static/` reload the browser. Run it from
  this directory, which the app serves `static/` from. Beside another app on
  port 3000, run `PLACEBO_ADDR=127.0.0.1:3001 placebo dev`.
- `cargo test` runs the tests, including the styles test.

## Database

- The app uses the Postgres database `{database}` on the local server and
  creates it on the first debug run. `DATABASE_URL` names another; `PGUSER`
  and `PGPASSWORD` sign in as another role, as on Windows.
- Change the schema with a new numbered file in `migrations/`, such as
  `0002_tags.sql`; it runs when the app starts. Never edit a migration that
  has run.
- Query with `sqlx::query_as` and `.bind` parameters; never format values into
  SQL. Check a version in the same statement as its write, as `save` in
  `src/tasks.rs` does with `UPDATE … WHERE id = $3 AND version = $4 RETURNING …`.
- Remove the database with `dropdb {database}` when you delete the app.

## The starter's demo

`src/main.rs` is the app's shell: the database, `layout()` with its header,
and the theme toggle. Keep it. `src/tasks.rs` is a task demo whose seeded
tasks tour Placebo; read it for the patterns (a list, a page per record, adds,
toggles, deletes, a versioned editor, live updates). When the person starts on
their own app, remove it:

1. Delete `src/tasks.rs`, and in `src/main.rs` remove `mod tasks;` and
   `.merge(tasks::routes())`.
2. Add a migration, such as `0002_remove_demo.sql`, with `DROP TABLE tasks;`.
   Keep `0001_tasks.sql`: it has run.
3. Route `/` to the app's own first page, rendered with `layout()`.

## Styles

`static/kit/README.md` lists the kit's tokens and classes. `static/kit` is
Placebo's kit copied verbatim: never edit it. The app's own components go in
`static/components.css`, from the kit's tokens; `cargo test` fails on values
off the kit's scale.

{RULES}"
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
        assert!(clone.contains("placebo = { path = \"/home/someone/placebo\" }"));
        assert!(clone.contains("[workspace]"));
        assert!(!clone.contains("starter") && !clone.contains("\"..\""));
        let cached = manifest("my-app", CACHED);
        assert!(
            cached.contains(&format!("placebo = {{ git = {REPO:?} }}")),
            "{cached}"
        );
        assert!(!cached.contains(".cargo"));
    }

    #[test]
    fn agents_md_carries_the_rules_and_reachable_docs() {
        let clone = agents("my-app", CLONE);
        assert!(clone.ends_with(RULES));
        assert!(clone.contains("`placebo_my_app`"));
        assert!(clone.contains("/home/someone/placebo/docs"));
        let cached = agents("my-app", CACHED);
        assert!(cached.contains(&format!("{REPO}/tree/main/docs")));
        assert!(!cached.contains(".cargo"));
    }

    #[test]
    fn an_empty_directory_is_set_up_in_place_and_a_used_one_refused() {
        let dir = std::env::temp_dir().join(format!("placebo-new-{}", std::process::id()));
        let app = dir.join("my-app");
        fs::create_dir_all(&app).unwrap();
        run(&app).unwrap();
        assert!(app.join("Cargo.toml").is_file() && app.join("static/kit/main.css").is_file());
        let error = run(&app).unwrap_err().to_string();
        assert!(error.contains("already has src/main.rs"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
