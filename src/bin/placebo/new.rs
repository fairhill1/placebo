//! `placebo new [PATH]`: a starter app on Postgres, copied from template/, a
//! workspace member that the repository builds and tests like any other crate.
use std::{fs, io, path::Path};

/// Where this CLI was built from.
const PLACEBO: &str = env!("CARGO_MANIFEST_DIR");
const REPO: &str = "https://github.com/fairhill1/placebo";
const MANIFEST: &str = include_str!("../../../template/Cargo.toml");

/// Template files copied as they are. Basecoat is not among them: `placebo
/// css` copies it from the Placebo the app builds against, so it updates with
/// the crate.
const FILES: [(&str, &str); 8] = [
    ("src/main.rs", include_str!("../../../template/src/main.rs")),
    ("src/auth.rs", include_str!("../../../template/src/auth.rs")),
    (
        "migrations/0001_accounts.sql",
        include_str!("../../../template/migrations/0001_accounts.sql"),
    ),
    (
        "tests/styles.rs",
        include_str!("../../../template/tests/styles.rs"),
    ),
    (
        "styles/app.css",
        include_str!("../../../template/styles/app.css"),
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
    // A standalone app is its own workspace, not a member of Placebo's, so it
    // takes the workspace's profile for password hashing too.
    let manifest = manifest.replacen("[features]", "[workspace]\n\n[features]", 1);
    format!("{manifest}\n{}", profiles())
}

/// The `[profile]` tables of Placebo's own manifest, which a workspace member
/// cannot set: the password hashing, optimized in debug builds.
fn profiles() -> &'static str {
    const WORKSPACE: &str = include_str!("../../../Cargo.toml");
    let start = WORKSPACE
        .find("\n# Argon2 password hashing")
        .expect("Placebo's manifest sets the template's profiles");
    WORKSPACE[start..].trim_start()
}

fn agents(name: &str) -> String {
    let database = database(name);
    format!(
        "# {name}

A Placebo app: Rust, Axum, Maud, and Postgres.

## Placebo's rules

Run `placebo rules` before changing any code, and follow them: they say how
pages, forms, replies, and styles work here, and where the docs are. Run
`placebo kit` for the styles: Tailwind and Basecoat. Both print the Placebo
this app builds against, so they stay current when it updates.

## Commands

- `placebo dev` builds and runs the app on http://127.0.0.1:3000. Rust edits
  rebuild and restart it; it compiles `styles/app.css` with Tailwind into
  `static/app.css` as views and styles change, and edits in `static/` reload
  the browser. Run the app
  with it, not `cargo run`, which does neither. Run it from this directory,
  which the app serves `static/` from. Beside another app on port 3000, run
  `PLACEBO_ADDR=127.0.0.1:3001 placebo dev`.
- `cargo test` runs the tests, including the styles test.
- `placebo css --minify` compiles the stylesheet once, before a release build;
  `static/app.css` is built, not committed.
- `cargo update -p placebo` takes Placebo's updates: its Rust, its script,
  its kit, and its rules. Run `cargo test` after it. `cargo install --git
  {REPO} placebo --features dev --locked` updates the `placebo` command.

## Database

- The app uses the Postgres database `{database}` on the local server and
  creates it on the first debug run. `DATABASE_URL` names another; `PGUSER`
  and `PGPASSWORD` sign in as another role, as on Windows.
- Change the schema with the next numbered file in `migrations/`, such as
  `0002_projects.sql`; it runs when the app starts. Never edit a migration
  that has run. `0001_accounts.sql` holds the users and their sessions.
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

## Accounts

`src/auth.rs` has sign-up, sign-in, and sign-out, with passwords hashed by
Argon2 and sessions in the database. Every page needs someone signed in,
except the paths its `public()` lists; a signed-out visit goes to /login and
back after signing in. A handler that needs the person takes `user: User`
(its `id` and `email`) and passes it to `layout()`, which shows it in the
sidebar's account menu.

- Scope every query to the signed-in person where the data is theirs:
  `WHERE owner_id = $1` bound to `user.id`, on reads and writes alike. A
  record id from a form or the address proves nothing about who may see it.
- A new public page (a landing page, a shared link) is a path in `public()`;
  it takes `Option<User>`.
- Anyone can sign up. To close it, remove the sign-up route and page, or check
  the email against an allowed list in `sign_up`.
- Password reset, email verification, and sign-in with another provider are
  not here: they need an email sender or a provider's keys, so ask the person
  before adding one.

## Styles

Pages use Basecoat's components (`.btn`, `.card`, `.field`...) and lay out
with Tailwind's utilities, all from the theme's scale; `placebo kit` lists
them. `styles/app.css` imports Tailwind and Basecoat, which `placebo css` and
`placebo dev` copy into `.placebo/`. A pattern used twice is a Rust `const` of
classes; `cargo test` fails on arbitrary values such as `p-[13px]` and on
values off the theme.
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
        assert!(clone.contains("[profile.dev.package.argon2]\nopt-level = 3"));
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
        assert!(agents.contains("`src/auth.rs`"));
    }

    #[test]
    fn an_empty_directory_is_set_up_in_place_and_a_used_one_refused() {
        let dir = std::env::temp_dir().join(format!("placebo-new-{}", std::process::id()));
        let app = dir.join("my-app");
        fs::create_dir_all(&app).unwrap();
        run(&app).unwrap();
        assert!(app.join("Cargo.toml").is_file() && app.join("styles/app.css").is_file());
        assert!(app.join("src/auth.rs").is_file() && app.join("migrations/0001_accounts.sql").is_file());
        let error = run(&app).unwrap_err().to_string();
        assert!(error.contains("already has src/main.rs"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
