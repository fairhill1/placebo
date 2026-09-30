---
name: placebo
description: Start or build a web app with Placebo, the Rust + Axum framework (github.com/fairhill1/placebo). Use when someone asks to build, start, or set up an app with Placebo, or works in a directory whose AGENTS.md says it is a Placebo app.
---

# Placebo

Placebo is an experimental framework for server-rendered Rust apps on Axum,
Maud, and Postgres. It is not in your training data: take its rules from the
`AGENTS.md` of the app, never from memory or from other frameworks.

**If the directory already has an `AGENTS.md` for a Placebo app**, read it and
follow it. Nothing else here applies.

**Otherwise, set up a new app in the current directory**, which the person
created for it. Check first that it is empty apart from dotfiles; if it holds
other work, ask where the app should go.

1. Check for Rust with `cargo --version`. If it is missing, install it:
   - macOS and Linux: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`,
     then `. "$HOME/.cargo/env"`.
   - Windows: `winget install Rustlang.Rustup`. Rust needs the MSVC build
     tools too; if linking fails, install them with
     `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`.
2. Check for a running Postgres with `pg_isready`. If it is missing or
   stopped, tell the person what you will run and wait for their go-ahead:
   it needs administrator rights, and they may run Postgres their own way.
   The app connects as the person's user and creates its own database, so
   that user needs a role that may create databases.
   - macOS: `brew install postgresql@17 && brew services start postgresql@17`.
     Homebrew creates the role.
   - Debian and Ubuntu: `sudo apt install -y postgresql`, then
     `sudo -u postgres createuser --createdb "$USER"`.
   - Arch: `sudo pacman -S --noconfirm postgresql`, then
     `sudo -u postgres initdb -D /var/lib/postgres/data`,
     `sudo systemctl enable --now postgresql`, and
     `sudo -u postgres createuser --createdb "$USER"`.
   - Windows: `winget install PostgreSQL.PostgreSQL.17`, which asks for a
     password for its `postgres` user. The app cannot sign in as the Windows
     user, so set `setx PGUSER postgres` and `setx PGPASSWORD <that password>`
     and open a new terminal. The app reads them and still uses its own
     database.
3. Install or update the CLI, even when `placebo` is already installed: the
   starter it writes is built into it, so an old CLI writes an old starter.
   `cargo install --git https://github.com/fairhill1/placebo placebo --features dev --locked`
   finishes at once when it is up to date, and rebuilds when the repository
   has moved on.
   A permission check may refuse this, since it builds code from GitHub. Then
   do not retry it another way: ask the person to run it themselves by typing
   `! cargo install --git https://github.com/fairhill1/placebo placebo --features dev --locked`,
   and continue once it is installed. To let it run on its own next time, they
   can add `Bash(cargo install --git https://github.com/fairhill1/placebo:*)`
   to `permissions.allow` in their Claude Code settings.
4. Run `placebo new` in the empty directory, then read the `AGENTS.md` it
   writes and follow it from there.

Then tell the person the app runs with `placebo dev` on http://127.0.0.1:3000,
and build what they asked for by the rules in `AGENTS.md`.
