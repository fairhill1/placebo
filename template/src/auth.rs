//! Accounts: sign up, sign in, sign out, and the signed-in person on every
//! request.
//!
//! `session` runs around the whole app. It looks up the person from the
//! session cookie once per request (a save's page render reuses it) and sends
//! anyone signed out to /login, except on the paths `public` lists. A handler
//! that needs the person takes `User`.
//!
//! Passwords are hashed with Argon2id. A session is a random token in an
//! HttpOnly cookie, kept in the database only as its SHA-256, for 30 days.
use crate::{App, Failed, solo};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Router,
    extract::{FromRequestParts, OptionalFromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, Method, Uri, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use maud::{Markup, html};
use placebo::{Component, Control, FormInput, Input, MutationAction, fields, icon};
use serde::Deserialize;
use sqlx::PgPool;
use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

/// The signed-in person. `session` has sent anyone without one to /login
/// before a handler runs, except on public paths, where `Option<User>` tells.
#[derive(Clone)]
pub struct User {
    pub id: i64,
    pub email: String,
}

impl<S: Send + Sync> FromRequestParts<S> for User {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Response> {
        parts
            .extensions
            .get::<User>()
            .cloned()
            .ok_or_else(|| to_login(&parts.method, &parts.uri))
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for User {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Option<Self>, Infallible> {
        Ok(parts.extensions.get::<User>().cloned())
    }
}

/// Paths anyone may open, signed in or not. Every other path needs a session.
fn public(path: &str) -> bool {
    let kit = placebo::KIT_PATH.trim_end_matches("{file}");
    matches!(path, "/login" | "/signup" | "/placebo.js")
        || ["/auth/", "/static/", kit]
            .into_iter()
            .any(|prefix| path.starts_with(prefix))
}

/// Finds the signed-in person for the request, or sends a signed-out one to
/// /login. It wraps `native_forms`, so a save's page render has the person
/// without another query.
pub async fn session(State(app): State<App>, mut request: Request, next: Next) -> Response {
    let user = match token(request.headers()) {
        Some(token) => match signed_in(&app.db, &token).await {
            Ok(user) => user,
            Err(error) => return Failed::from(error).into_response(),
        },
        None => None,
    };
    match user {
        Some(user) => {
            request.extensions_mut().insert(user);
        }
        None if !public(request.uri().path()) => {
            return to_login(request.method(), request.uri());
        }
        None => {}
    }
    let mut response = next.run(request).await;
    // A page shows who is signed in, so no shared cache may hand it to another
    // person, and none may hand two people one form's idempotency key.
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("private"));
    response
}

/// Where a signed-out request goes: /login, and back to the page it asked for
/// after signing in.
fn to_login(method: &Method, uri: &Uri) -> Response {
    let asked = uri.path_and_query().map_or("/", |path| path.as_str());
    if method == Method::GET && asked != "/" {
        Redirect::to(&format!("/login?next={}", encode(asked))).into_response()
    } else {
        Redirect::to("/login").into_response()
    }
}

async fn signed_in(db: &PgPool, token: &[u8; 32]) -> sqlx::Result<Option<User>> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT users.id, users.email FROM sessions JOIN users ON users.id = sessions.user_id \
         WHERE sessions.token_hash = sha256($1) AND sessions.expires_at > now()",
    )
    .bind(&token[..])
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, email)| User { id, email }))
}

// Sessions

const COOKIE: &str = "session";
const DAYS: i32 = 30;

/// Signs `user` in on this browser: a new session, and the `Set-Cookie` value
/// that carries its token.
async fn start(db: &PgPool, user: i64) -> Result<String, Failed> {
    let mut token = [0; 32];
    getrandom::fill(&mut token).expect("read the system's random source");
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND expires_at <= now()")
        .bind(user)
        .execute(db)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) \
         VALUES (sha256($1), $2, now() + make_interval(days => $3))",
    )
    .bind(&token[..])
    .bind(user)
    .bind(DAYS)
    .execute(db)
    .await?;
    Ok(cookie(&hex(&token), i64::from(DAYS) * 24 * 60 * 60))
}

fn cookie(value: &str, max_age: i64) -> String {
    // Debug builds serve plain http on 127.0.0.1, where a Secure cookie is
    // not sent by every browser.
    let secure = if cfg!(debug_assertions) { "" } else { "; Secure" };
    format!("{COOKIE}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}")
}

fn token(headers: &HeaderMap) -> Option<[u8; 32]> {
    let value = crate::cookie(headers, COOKIE)?.as_bytes();
    if value.len() != 64 {
        return None;
    }
    let mut token = [0; 32];
    for (byte, pair) in token.iter_mut().zip(value.chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(token)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Percent-encodes a path for a query value, keeping its slashes readable.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// `next` if it is a page on this site, else Home. It comes from the address,
/// so anyone can write one: never go to another site or an action.
fn back_to(next: &str) -> &str {
    let on_site = next.starts_with('/') && !next.starts_with("//") && !next.starts_with("/\\");
    if on_site && !next.starts_with("/auth/") && next.bytes().all(|byte| byte.is_ascii_graphic()) {
        next
    } else {
        "/"
    }
}

// Passwords

const MIN_PASSWORD: usize = 8;
/// Refused by the browser and the server alike, so no request has a
/// megabyte hashed.
const MAX_PASSWORD: usize = 256;

/// Hashing takes tens of milliseconds on purpose, so it runs off the async
/// threads.
async fn hash(password: String) -> String {
    tokio::task::spawn_blocking(move || {
        let hash: PasswordHash = Argon2::default()
            .hash_password(password.as_bytes())
            .expect("hash a password");
        hash.to_string()
    })
    .await
    .expect("the hashing thread finished")
}

/// Checks a password against a stored hash. An unknown email checks against a
/// stand-in hash, so it takes as long as a wrong password and the time an
/// answer takes does not tell which addresses have accounts.
async fn verify(password: String, stored: Option<String>) -> bool {
    static STAND_IN: LazyLock<String> = LazyLock::new(|| {
        let hash: PasswordHash = Argon2::default()
            .hash_password(b"no account has this password")
            .expect("hash a password");
        hash.to_string()
    });
    tokio::task::spawn_blocking(move || {
        let known = stored.is_some();
        let stored = stored.unwrap_or_else(|| STAND_IN.clone());
        let matches = PasswordHash::new(&stored).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        });
        known && matches
    })
    .await
    .unwrap_or(false)
}

/// Failed sign-ins per email, in this process. After `LIMIT` in `WINDOW` the
/// address is refused until the window passes, so a password cannot be
/// guessed at speed.
static FAILURES: LazyLock<Mutex<HashMap<String, (u32, Instant)>>> = LazyLock::new(Default::default);
const LIMIT: u32 = 10;
const WINDOW: Duration = Duration::from_secs(15 * 60);

fn throttled(email: &str) -> bool {
    let failures = FAILURES.lock().unwrap();
    failures
        .get(email)
        .is_some_and(|(count, since)| *count >= LIMIT && since.elapsed() < WINDOW)
}

fn failed(email: &str) {
    let mut failures = FAILURES.lock().unwrap();
    if failures.len() > 10_000 {
        failures.retain(|_, (_, since)| since.elapsed() < WINDOW);
    }
    let entry = failures
        .entry(email.to_owned())
        .or_insert((0, Instant::now()));
    if entry.1.elapsed() >= WINDOW {
        *entry = (0, Instant::now());
    }
    entry.0 += 1;
}

/// One address is one account, however it is typed.
fn normalize(email: &str) -> String {
    email.trim().to_lowercase()
}

fn plausible(email: &str) -> bool {
    let Some((name, domain)) = email.split_once('@') else {
        return false;
    };
    !name.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.len() <= 254
        && !email.contains(char::is_whitespace)
}

// The pages and actions

/// The account pages and actions, for the app's router.
pub fn routes() -> Router<App> {
    Router::new()
        .route("/login", get(login))
        .route("/signup", get(signup))
        .route(SIGN_IN.path(), SIGN_IN.route(sign_in))
        .route(SIGN_UP.path(), SIGN_UP.route(sign_up))
        .route(SIGN_OUT.path(), SIGN_OUT.route(sign_out))
}

#[derive(Deserialize, FormInput)]
struct SignIn {
    email: String,
    password: String,
    next: String,
}

#[derive(Deserialize, FormInput)]
struct SignUp {
    email: String,
    password: String,
}

#[derive(Deserialize, FormInput)]
struct SignOut {}

const SIGN_IN: MutationAction<SignIn> = MutationAction::new("sign-in", "/auth/login");
const SIGN_UP: MutationAction<SignUp> = MutationAction::new("sign-up", "/auth/signup");
const SIGN_OUT: MutationAction<SignOut> = MutationAction::new("sign-out", "/auth/logout");

/// The page to return to after signing in, from the address.
#[derive(Deserialize, FormInput)]
struct Back {
    next: Option<String>,
}

fn sign_in_form(email: &str, next: &str, error: Option<&str>) -> Markup {
    let password = Control::password()
        .id("sign-in-password")
        .required()
        .max_length(MAX_PASSWORD as u32)
        .autocomplete("current-password");
    let password = match error {
        Some(_) => password.invalid(true).described_by("sign-in-error"),
        None => password,
    };
    let fields = fields! { SignIn {
        @field next = Control::hidden(next.to_owned());
        div .stack {
            @if let Some(error) = error {
                p .alert #sign-in-error role="alert" { (error) }
            }
            div .field {
                label for="sign-in-email" { "Email" }
                @field email = Control::email(email).id("sign-in-email").required().autocomplete("username");
            }
            div .field {
                label for="sign-in-password" { "Password" }
                @field password = password;
            }
            button .btn .btn-block type="submit" { "Sign in" }
        }
    } };
    SIGN_IN.bind(&Component::new("account", "sign-in")).form(fields)
}

async fn login(user: Option<User>, headers: HeaderMap, Input(back): Input<Back>) -> Response {
    if user.is_some() {
        return Redirect::to("/").into_response();
    }
    let next = back.next.as_deref().map_or("/", back_to);
    let form = sign_in_form("", next, None);
    solo(
        &headers,
        "Sign in",
        html! {
            section .card {
                div .stack style="--stack-space: var(--space-lg)" {
                    h1 { "Sign in" }
                    (Component::new("account", "sign-in").mount(form))
                }
            }
            p .muted .solo-note { "No account yet? " a href="/signup" { "Sign up" } }
        },
    )
    .into_response()
}

async fn sign_in(State(app): State<App>, Input(input): Input<SignIn>) -> Result<Response, Failed> {
    let email = normalize(&input.email);
    let binding = SIGN_IN.bind(&Component::new("account", "sign-in"));
    let refuse = |message| binding.invalid(sign_in_form(&input.email, &input.next, Some(message)));
    if throttled(&email) {
        return Ok(refuse("Too many tries for this email. Wait 15 minutes, then try again.").into_response());
    }
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&app.db)
            .await?;
    let (id, stored) = row.unzip();
    let matches = input.password.chars().count() <= MAX_PASSWORD
        && verify(input.password.clone(), stored).await;
    let Some(id) = id.filter(|_| matches) else {
        failed(&email);
        return Ok(refuse("That email and password don't match an account.").into_response());
    };
    FAILURES.lock().unwrap().remove(&email);
    let cookie = start(&app.db, id).await?;
    let reply = binding.reply(html! {}).navigate(back_to(&input.next));
    Ok(([(header::SET_COOKIE, cookie)], reply).into_response())
}

fn sign_up_form(email: &str, error: Option<(&str, &str)>) -> Markup {
    // The error names the control it is about, "email" or "password", which
    // is marked and described by the error before its own hint.
    let (about, message) = error.unzip();
    let mark = |control: Control<String>, name: &str, hint: &str| match about {
        Some(about) if about == name => control
            .invalid(true)
            .described_by(format!("sign-up-error {hint}").trim_end()),
        _ if !hint.is_empty() => control.described_by(hint),
        _ => control,
    };
    let fields = fields! { SignUp {
        div .stack {
            @if let Some(message) = message {
                p .alert #sign-up-error role="alert" { (message) }
            }
            div .field {
                label for="sign-up-email" { "Email" }
                @field email = mark(Control::email(email).id("sign-up-email").required().autocomplete("email"), "email", "");
            }
            div .field {
                label for="sign-up-password" { "Password" }
                @field password = mark(Control::password().id("sign-up-password").required().max_length(MAX_PASSWORD as u32).autocomplete("new-password"), "password", "sign-up-hint");
                p .field-hint #sign-up-hint { "At least " (MIN_PASSWORD) " characters." }
            }
            button .btn .btn-block type="submit" { "Create account" }
        }
    } };
    SIGN_UP.bind(&Component::new("account", "sign-up")).form(fields)
}

async fn signup(user: Option<User>, headers: HeaderMap) -> Response {
    if user.is_some() {
        return Redirect::to("/").into_response();
    }
    solo(
        &headers,
        "Sign up",
        html! {
            section .card {
                div .stack style="--stack-space: var(--space-lg)" {
                    h1 { "Create your account" }
                    (Component::new("account", "sign-up").mount(sign_up_form("", None)))
                }
            }
            p .muted .solo-note { "Have an account? " a href="/login" { "Sign in" } }
        },
    )
    .into_response()
}

async fn sign_up(State(app): State<App>, Input(input): Input<SignUp>) -> Result<Response, Failed> {
    let email = normalize(&input.email);
    let binding = SIGN_UP.bind(&Component::new("account", "sign-up"));
    let refuse = |about, message| binding.invalid(sign_up_form(&input.email, Some((about, message))));
    if !plausible(&email) {
        return Ok(refuse("email", "Enter an email address, like name@example.com.").into_response());
    }
    if !(MIN_PASSWORD..=MAX_PASSWORD).contains(&input.password.chars().count()) {
        let message = format!("Use {MIN_PASSWORD} to {MAX_PASSWORD} characters for the password.");
        return Ok(refuse("password", &message).into_response());
    }
    let password_hash = hash(input.password.clone()).await;
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash) VALUES ($1, $2) \
         ON CONFLICT (email) DO NOTHING RETURNING id",
    )
    .bind(&email)
    .bind(&password_hash)
    .fetch_optional(&app.db)
    .await?;
    let Some(id) = id else {
        return Ok(refuse("email", "An account with this email already exists. Sign in instead.").into_response());
    };
    let cookie = start(&app.db, id).await?;
    let reply = binding.reply(html! {}).navigate("/");
    Ok(([(header::SET_COOKIE, cookie)], reply).into_response())
}

/// The account menu at the foot of the sidebar: who is signed in, and
/// signing out.
pub fn account_menu(user: &User) -> Markup {
    let initial = user.email.chars().next().unwrap_or('?').to_uppercase();
    let fields = fields! { SignOut {
        button .menu-item type="submit" { (icon!("log-out")) "Sign out" }
    } };
    let component = Component::new("account", "menu");
    html! {
        details .dropdown .shell-account data-placebo-behavior="dropdown" {
            summary title=(user.email) {
                // A hue of its own for each person, spread round the wheel.
                span .avatar style=(format!("--avatar-hue: {}", user.id * 137 % 360)) aria-hidden="true" { (initial) }
                span .shell-account-name { (user.email) }
                (icon!("chevron-up"))
            }
            div .menu .menu-up {
                (component.mount(SIGN_OUT.bind(&component).form(fields)))
            }
        }
    }
}

/// Ends this browser's session. It is a public path, so signing out of a
/// session that already ended still goes to /login.
async fn sign_out(
    State(app): State<App>,
    headers: HeaderMap,
    Input(_): Input<SignOut>,
) -> Result<Response, Failed> {
    if let Some(token) = token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = sha256($1)")
            .bind(&token[..])
            .execute(&app.db)
            .await?;
    }
    let reply = SIGN_OUT
        .bind(&Component::new("account", "menu"))
        .reply(html! {})
        .navigate("/login");
    Ok(([(header::SET_COOKIE, cookie("", 0))], reply).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_account_pages_and_assets_are_public() {
        for path in ["/login", "/signup", "/auth/login", "/auth/logout", "/static/app.css", "/placebo/kit/main.css", "/placebo.js"] {
            assert!(public(path), "{path}");
        }
        for path in ["/", "/settings", "/theme", "/loginx", "/static"] {
            assert!(!public(path), "{path}");
        }
    }

    #[test]
    fn next_stays_on_this_site() {
        for next in ["/", "/settings", "/projects/7?tab=notes"] {
            assert_eq!(back_to(next), next);
        }
        for next in ["https://evil.example", "//evil.example", "/\\evil.example", "settings", "/auth/logout", "/a b", "/a\r\nSet-Cookie: x"] {
            assert_eq!(back_to(next), "/", "{next:?}");
        }
        assert_eq!(encode("/projects/7?tab=notes&x=1"), "/projects/7%3Ftab%3Dnotes%26x%3D1");
    }

    #[test]
    fn a_session_token_round_trips_through_its_cookie() {
        let sent: [u8; 32] = std::array::from_fn(|i| i as u8 * 7);
        let mut headers = HeaderMap::new();
        let set = cookie(&hex(&sent), 60);
        let pair = set.split(';').next().unwrap();
        headers.insert(header::COOKIE, HeaderValue::from_str(&format!("theme=dark; {pair}")).unwrap());
        assert_eq!(token(&headers), Some(sent));
        headers.insert(header::COOKIE, HeaderValue::from_static("session=zz"));
        assert_eq!(token(&headers), None);
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax"));
    }

    #[test]
    fn emails_are_one_account_however_typed() {
        assert_eq!(normalize("  Ada@Example.COM "), "ada@example.com");
        assert!(plausible("ada@example.com"));
        for email in ["ada", "@example.com", "ada@example", "ada@.com", "a da@example.com"] {
            assert!(!plausible(email), "{email}");
        }
    }
}
