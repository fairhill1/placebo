import assert from "node:assert/strict";
import { test } from "node:test";
import { spawn, execFile } from "node:child_process";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { createServer } from "node:net";
import { createRequire } from "node:module";
import { promisify } from "node:util";
import { once } from "node:events";

const exec = promisify(execFile);
const { chromium } = process.env.PLAYWRIGHT_MODULE
  ? createRequire(import.meta.url)(process.env.PLAYWRIGHT_MODULE)
  : await import("playwright");

async function until(check, description, timeout = 30000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw new Error(`Timed out: ${description}`);
}

test("placebo dev rebuilds, reloads, survives compile errors, and cleans up on termination", {
  skip: !process.env.PLACEBO_TEST_DEV,
  timeout: 90000,
}, async () => {
  const root = resolve(".");
  const directory = await mkdtemp(join(tmpdir(), "placebo-dev-"));
  const reservation = createServer();
  await new Promise(resolve => reservation.listen(0, "127.0.0.1", resolve));
  const port = reservation.address().port;
  await new Promise(resolve => reservation.close(resolve));
  const origin = `http://127.0.0.1:${port}`;
  let supervisor, browser;
  let output = "";
  try {
    await mkdir(join(directory, "src"));
    await mkdir(join(directory, "static"));
    await writeFile(join(directory, "Cargo.toml"), `[package]
name = "placebo_dev_fixture"
version = "0.0.0"
edition = "2024"
[features]
dev = ["placebo/dev"]
[dependencies]
placebo = { path = ${JSON.stringify(root)} }
axum = "0.8.9"
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "net"] }
`);
    const source = `use axum::{Router, routing::get, response::Html};
const HEADING: &str = "Version one";
#[tokio::main]
async fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let reload = placebo::dev::watch([root.join("static")]).unwrap();
    let app = Router::new()
        .route("/", get(async || Html(format!("<!doctype html><html><head><link rel=stylesheet href=/style.css></head><body><h1>{HEADING}</h1></body></html>"))))
        .route("/style.css", get(async || ([ ("content-type", "text/css"), ("cache-control", "no-store") ], std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/static/style.css")).unwrap())))
        .layer(reload.layer());
    let listener = tokio::net::TcpListener::bind(std::env::var("PLACEBO_ADDR").unwrap()).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
`;
    await writeFile(join(directory, "src/main.rs"), source);
    await writeFile(join(directory, "static/style.css"), ":root { --probe: initial; }");
    const env = { ...process.env, CARGO_TARGET_DIR: join(root, "target"), CARGO_NET_OFFLINE: "true", PLACEBO_ADDR: `127.0.0.1:${port}` };
    await exec("cargo", ["generate-lockfile", "--offline"], { cwd: directory, env });
    supervisor = spawn(join(root, "target/debug/placebo"), ["dev", "--bin", "placebo_dev_fixture", "--features", "dev"], {
      cwd: directory, env, stdio: ["ignore", "pipe", "pipe"],
    });
    supervisor.stdout.on("data", data => { output += data; });
    supervisor.stderr.on("data", data => { output += data; });
    await until(async () => {
      try { return (await fetch(origin)).ok; } catch { return false; }
    }, "initial dev server", 45000);
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    page.setDefaultTimeout(15000);
    await page.addInitScript(() => {
      const Native = EventSource;
      window.EventSource = class extends Native {
        constructor(...args) { super(...args); this.addEventListener("init", () => { window.reloadReady = true; }); }
      };
    });
    await page.goto(origin);
    await page.waitForFunction(() => window.reloadReady);
    const beforeStatic = (output.match(/\[placebo:build\]/g) ?? []).length;
    const staticReload = page.waitForEvent("load");
    await writeFile(join(directory, "static/style.css"), ":root { --probe: static-edit; }");
    await staticReload;
    assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--probe").trim()), "static-edit");
    assert.equal((output.match(/\[placebo:build\]/g) ?? []).length, beforeStatic, "static edits do not invoke Cargo");

    await page.waitForFunction(() => window.reloadReady);
    await writeFile(join(directory, "src/main.rs"), source.replace("Version one", "Version two"));
    await page.waitForFunction(() => document.querySelector("h1").textContent === "Version two");
    assert.ok((output.match(/\[placebo:ready\]/g) ?? []).length >= 2, "a rebuilt application was launched");

    const beforeBypass = (output.match(/\[placebo:build\]/g) ?? []).length;
    await writeFile(join(directory, "src/main.rs"), source.replace("Version one", "<form data-placebo='manual'></form>"));
    await until(() => output.includes("[placebo:raw-config]"), "API bypass diagnostic");
    assert.equal((output.match(/\[placebo:build\]/g) ?? []).length, beforeBypass, "a detected bypass never reaches Cargo");
    assert.ok((await (await fetch(origin)).text()).includes("Version two"), "the previous server survives a failed project check");

    const previousFailures = (output.match(/\[placebo:build-failed\]/g) ?? []).length;
    await writeFile(join(directory, "src/main.rs"), source.replace('"Version one"', "42"));
    await until(() => (output.match(/\[placebo:build-failed\]/g) ?? []).length > previousFailures, "compiler failure diagnostic");
    assert.ok((await (await fetch(origin)).text()).includes("Version two"), "the previous server survives a failed build");

    await writeFile(join(directory, "src/main.rs"), source.replace("Version one", "Recovered"));
    await page.waitForFunction(() => document.querySelector("h1").textContent === "Recovered");
    const pids = Array.from(output.matchAll(/\[placebo:ready\].*\(pid (\d+)\)/g), match => Number(match[1]));
    const exited = once(supervisor, "exit");
    supervisor.kill("SIGTERM");
    await exited;
    assert.ok(output.includes("[placebo:stopped]"));
    for (const pid of pids) assert.throws(() => process.kill(pid, 0), { code: "ESRCH" }, "application process was reaped");
    await assert.rejects(fetch(origin), "the listening socket was released");
    supervisor = null;
  } catch (error) {
    error.message += `\nSupervisor output:\n${output}`;
    throw error;
  } finally {
    await browser?.close();
    if (supervisor && supervisor.exitCode === null) {
      const exited = once(supervisor, "exit");
      supervisor.kill("SIGTERM");
      await exited;
    }
    await rm(directory, { recursive: true, force: true });
  }
});
