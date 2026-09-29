import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Reads that start themselves: a "load more" form when it scrolls into view,
// and the page again on an interval while refresh_every is on it. `/` polls;
// `/quiet` is the same page without polling.
const fixture = serverFixture("triggers");

async function visit(t, path = "/", options) {
  const page = await fixture.page(t, options);
  await page.goto(fixture.origin + path);
  return page;
}

// Page reads the runtime sends: polls, and "load more".
const reads = page => {
  const seen = [];
  page.on("request", request => { if (request.headers()["x-placebo-refresh"]) seen.push(request.url()); });
  return seen;
};
const polled = page => page.evaluate(() => window.events.filter(e => e.type === "applied" && e.source === "interval").length);
const entries = page => page.locator("#entries > .entry").count();

// Serve every HTML answer with `change` applied: the page and its reads.
async function rewrite(page, change) {
  await page.route("**/*", async route => {
    const request = route.request();
    if (request.resourceType() !== "document" && !request.headers()["x-placebo-refresh"]) return route.fallback();
    const response = await route.fetch();
    return route.fulfill({ response, body: change(await response.text()) });
  });
}

test("polling reads the page on its interval, pauses while hidden, and reads once when shown", async t => {
  const page = await visit(t);
  const first = await page.locator("#ticks").textContent();
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied" && e.source === "interval").length >= 2);
  assert.notEqual(await page.locator("#ticks").textContent(), first);
  const sent = reads(page);
  await page.evaluate(() => {
    window.hidden = true;
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => window.hidden ? "hidden" : "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "deferred" && e.reason === "page-hidden"));
  const whileHidden = sent.length;
  await page.waitForTimeout(2200);
  assert.equal(sent.length, whileHidden, "no reads while the page is hidden");
  const before = await polled(page);
  await page.evaluate(() => { window.hidden = false; document.dispatchEvent(new Event("visibilitychange")); });
  await page.waitForFunction(count => window.events.filter(e => e.type === "applied" && e.source === "interval").length > count, before);
  assert.ok(sent.length > whileHidden);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 0);
});

test("polling stops when the page no longer renders its element", async t => {
  const page = await fixture.page(t);
  let done = false;
  // The job the page waits for is done: its reads no longer poll.
  await rewrite(page, body => done ? body.replace(/<div hidden data-placebo-refresh-every="\d+"><\/div>/, "") : body);
  await page.goto(fixture.origin);
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.source === "interval"));
  done = true;
  await page.waitForFunction(() => !document.querySelector("[data-placebo-refresh-every]"));
  const sent = reads(page);
  await page.waitForTimeout(2500);
  assert.deepEqual(sent, []);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 0);
});

test("a slow poll is not doubled by the next interval", async t => {
  const page = await visit(t);
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.source === "interval"));
  await page.route("**/*", async route => {
    if (route.request().headers()["x-placebo-refresh"]) await new Promise(resolve => setTimeout(resolve, 1600));
    await route.fallback();
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "ignored" && e.reason === "busy" && e.source === "interval"));
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "discarded").length), 0);
});

test("scrolling to the end of the list loads more entries until there are no more", async t => {
  const page = await visit(t, "/quiet");
  await page.evaluate(() => { window.first = document.getElementById("entry-1"); });
  for (let i = 0; i < 10 && await page.locator("#end").count() === 0; i++) {
    const before = await entries(page);
    await page.evaluate(() => document.querySelector("form[data-placebo]")?.scrollIntoView());
    await page.waitForFunction(before => document.querySelectorAll("#entries > .entry").length > before || document.getElementById("end"), before);
  }
  await page.locator("#end").waitFor();
  const ids = await page.$$eval("#entries > .entry", items => items.map(item => item.id));
  assert.deepEqual(ids, Array.from({ length: 45 }, (_, i) => `entry-${i + 1}`));
  // Entries already shown keep their nodes, and the address asks for them all.
  assert.ok(await page.evaluate(() => window.first === document.getElementById("entry-1")));
  assert.match(page.url(), /\/quiet\?shown=45$/);
  const reveals = await page.evaluate(() => window.events.filter(e => e.type === "scheduled" && e.source === "reveal").length);
  assert.equal(reveals, 4);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 0);
});

test("polling reads the page at its address, so loaded entries stay", async t => {
  const page = await visit(t);
  await page.evaluate(() => document.querySelector("form[data-placebo]").scrollIntoView());
  await page.waitForFunction(() => document.querySelectorAll("#entries > .entry").length >= 20);
  await page.waitForFunction(() => !document.querySelector("form[data-placebo][aria-busy]"));
  const shown = await entries(page);
  assert.match(page.url(), new RegExp(`\\?shown=${shown}$`));
  const before = await polled(page);
  await page.waitForFunction(count => window.events.filter(e => e.type === "applied" && e.source === "interval").length >= count + 2, before);
  assert.ok(await entries(page) >= shown);
  assert.match(page.url(), new RegExp(`\\?shown=${await entries(page)}$`));
});

test("without JavaScript the load-more form loads the longer page", async t => {
  const page = await visit(t, "/", { javaScriptEnabled: false });
  assert.equal(await entries(page), 10);
  await page.getByRole("button", { name: "Load more" }).click();
  await page.waitForURL(/\/\?shown=20$/);
  assert.equal(await entries(page), 20);
  await page.getByRole("button", { name: "Load more" }).click();
  await page.waitForURL(/\/\?shown=30$/);
  assert.equal(await entries(page), 30);
});

test("a poll keeps an edited control and open details, in a component or not", async t => {
  const page = await fixture.page(t);
  const extra = '<div id="card:1" data-placebo-component><details id="card-more-1"><summary>More</summary><p>Details</p></details></div>' +
    '<details id="more-2"><summary>Also</summary><p>More</p></details><label>Note <textarea id="note"></textarea></label>';
  await rewrite(page, body => body.replace(/(<p id="ticks">.*?<\/p>)/, `$1${extra}`));
  await page.goto(fixture.origin);
  await page.locator("#card-more-1 > summary").click();
  await page.locator("#more-2 > summary").click();
  await page.locator("#note").fill("Typed while polling.");
  const before = await polled(page);
  await page.waitForFunction(count => window.events.filter(e => e.type === "applied" && e.source === "interval").length >= count + 2, before);
  assert.deepEqual(await page.evaluate(() => [document.getElementById("card-more-1").open, document.getElementById("more-2").open]), [true, true]);
  assert.equal(await page.locator("#note").inputValue(), "Typed while polling.");
});

test("an invalid polling interval is reported", async t => {
  const page = await fixture.page(t);
  await rewrite(page, body => body.replace("<main>", '<main><div hidden data-placebo-refresh-every="100"></div>'));
  await page.goto(`${fixture.origin}/quiet`);
  await page.waitForFunction(() => window.events.some(e => e.type === "error" && e.code === "invalid-config"));
  const error = await page.evaluate(() => window.events.find(e => e.code === "invalid-config"));
  assert.match(error.message, /Polling interval '100'/);
  await page.waitForTimeout(1200);
  assert.equal(await polled(page), 0);
});
