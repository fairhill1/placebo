import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Reads that start themselves: on load, when revealed, and on an interval.
const fixture = serverFixture("triggers");

async function visit(t, options) {
  const page = await fixture.page(t, options);
  await page.goto(fixture.origin);
  return page;
}
const requests = (page, path) => {
  const seen = [];
  page.on("request", request => { if (new URL(request.url()).pathname === path) seen.push(request.url()); });
  return seen;
};

test("a section reads itself once when the page loads", async t => {
  const page = await visit(t);
  await page.locator("#stats-total").waitFor();
  assert.equal(await page.locator("#stats-total").textContent(), "45 entries, 5 pages");
  const scheduled = await page.evaluate(() => window.events.filter(e => e.type === "scheduled" && e.action === "load-stats"));
  assert.equal(scheduled.length, 1);
  assert.equal(scheduled[0].source, "load");
});

test("polling reads on its interval, pauses while hidden, and reads once when shown", async t => {
  const page = await visit(t);
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied" && e.action === "tick").length >= 2);
  const sent = requests(page, "/clock");
  await page.evaluate(() => {
    window.hidden = true;
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => window.hidden ? "hidden" : "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "deferred" && e.reason === "page-hidden"));
  const whileHidden = sent.length;
  await page.waitForTimeout(2200);
  assert.equal(sent.length, whileHidden, "no reads while the page is hidden");
  await page.evaluate(() => { window.hidden = false; document.dispatchEvent(new Event("visibilitychange")); });
  await page.waitForFunction(count => window.events.filter(e => e.type === "applied" && e.action === "tick").length > count,
    await page.evaluate(() => window.events.filter(e => e.type === "applied" && e.action === "tick").length));
  assert.ok(sent.length > whileHidden);
});

test("polling stops quietly when its region is removed", async t => {
  const page = await visit(t);
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.action === "tick"));
  await page.evaluate(() => document.getElementById("clock").remove());
  await page.waitForFunction(() => window.events.some(e => e.type === "discarded" && e.reason === "target-unmounted"));
  const sent = requests(page, "/clock");
  await page.waitForTimeout(1500);
  assert.equal(sent.length, 0);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 0);
});

test("a slow poll is not cancelled by the next interval", async t => {
  const page = await visit(t);
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.action === "tick"));
  await page.route("**/clock*", async route => { await new Promise(resolve => setTimeout(resolve, 1600)); await route.continue(); });
  await page.waitForFunction(() => window.events.some(e => e.type === "ignored" && e.reason === "busy" && e.action === "tick"));
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "discarded" && e.action === "tick").length), 0);
});

test("scrolling to the end of the list loads more entries into it until there are no more", async t => {
  const page = await visit(t);
  await page.evaluate(() => { window.first = document.getElementById("entries/1"); });
  for (const count of [20, 30, 40, 45]) {
    await page.locator("#more").scrollIntoViewIfNeeded();
    await page.waitForFunction(count => document.querySelectorAll("#entries > [data-placebo-item]").length === count, count);
  }
  await page.locator("#end").waitFor();
  const ids = await page.$$eval("#entries > [data-placebo-item]", items => items.map(item => item.id));
  assert.deepEqual(ids, Array.from({ length: 45 }, (_, i) => `entries/${i + 1}`));
  assert.ok(await page.evaluate(() => window.first === document.getElementById("entries/1")));
  const reveals = await page.evaluate(() => window.events.filter(e => e.type === "scheduled" && e.source === "reveal").length);
  assert.equal(reveals, 4);
});

test("a read reply can only insert items into lists its binding declares", async t => {
  const page = await fixture.page(t);
  await page.route("**/entries*", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.patches[0] = { target: "entries", operation: "remove-item", item: "entries/1" };
    await route.fulfill({ response, body: JSON.stringify(update) });
  });
  await page.goto(fixture.origin);
  await page.locator("#more").scrollIntoViewIfNeeded();
  await page.waitForFunction(() => window.events.some(e => e.type === "error" && e.code === "invalid-update"));
  assert.equal(await page.locator("#entries > [data-placebo-item]").count(), 10);
});

test("without JavaScript the forms are plain links to longer pages", async t => {
  const page = await visit(t, { javaScriptEnabled: false });
  assert.equal(await page.locator("#entries > [data-placebo-item]").count(), 10);
  await page.getByRole("button", { name: "Load more" }).click();
  await page.waitForURL("**/entries?shown=10");
  assert.equal(await page.locator("#entries > [data-placebo-item]").count(), 20);
  await page.getByRole("button", { name: "Load more" }).click();
  await page.waitForURL("**/entries?shown=20");
  assert.equal(await page.locator("#entries > [data-placebo-item]").count(), 30);
});
