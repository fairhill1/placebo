import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { after, before } from "node:test";

const { chromium } = process.env.PLAYWRIGHT_MODULE
  ? createRequire(import.meta.url)(process.env.PLAYWRIGHT_MODULE)
  : await import("playwright");

export function serverFixture(example) {
  const fixture = {};
  fixture.serverLog = "";
  let server;
  before(async () => {
    server = spawn(resolve(`target/debug/examples/${example}`), [], {
      env: { ...process.env, PLACEBO_ADDR: "127.0.0.1:0" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    fixture.origin = await new Promise((accept, reject) => {
      const timeout = setTimeout(() => reject(new Error("Demo did not start within 10 seconds.")), 10000);
      let output = "", stderr = "";
      server.stderr.on("data", chunk => { stderr += chunk; fixture.serverLog += chunk; });
      server.stdout.on("data", chunk => {
        output += chunk;
        const match = output.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (match) { clearTimeout(timeout); accept(match[0]); }
      });
      server.once("error", error => { clearTimeout(timeout); reject(error); });
      server.once("exit", code => { clearTimeout(timeout); reject(new Error(`Demo exited: ${code}\n${stderr}`)); });
    });
    fixture.browser = await chromium.launch({ headless: true });
  });
  after(async () => { await fixture.browser?.close(); server?.kill(); });
  fixture.page = async (t, options = {}) => {
    const context = await fixture.browser.newContext(options);
    t.after(() => context.close());
    const page = await context.newPage();
    page.setDefaultTimeout(8000);
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    t.after(() => assert.deepEqual(errors, [], "no uncaught browser exceptions"));
    await page.addInitScript(() => {
      const NativeEventSource = window.EventSource;
      window.EventSource = class extends NativeEventSource {
        constructor(...args) {
          super(...args);
          this.addEventListener("init", () => { window.reloadReady = true; });
        }
      };
      window.events = [];
      for (const type of ["scheduled", "request", "applied", "discarded", "deferred", "ignored", "error"]) {
        document.addEventListener(`placebo:${type}`, ({ detail }) => window.events.push({ type, ...detail }));
      }
    });
    return page;
  };
  return fixture;
}
