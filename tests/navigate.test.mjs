import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Links between pages show the next page without a document load.
const fixture = serverFixture("pages");

async function visit(t, path = "/") {
  const page = await fixture.page(t);
  await page.goto(fixture.origin + path);
  // Gone after a document load.
  await page.evaluate(() => { window.marker = "same document"; });
  return page;
}

const navigated = (page, count = 1) =>
  page.waitForFunction(count => window.events.filter(e => e.type === "navigated").length >= count, count);
const marker = page => page.evaluate(() => window.marker ?? null);

test("a link shows its page without a document load, focused on its heading", async t => {
  const page = await visit(t);
  await page.getByRole("link", { name: "Second note" }).click();
  await navigated(page);
  assert.equal(new URL(page.url()).pathname, "/notes/2");
  assert.equal(await marker(page), "same document");
  assert.equal(await page.title(), "Second note");
  assert.equal(await page.evaluate(() => document.activeElement.textContent), "Second note");
  const event = await page.evaluate(() => window.events.find(e => e.type === "navigated"));
  assert.equal(event.how, "push");
  assert.equal(event.path, "/notes/2");
});

test("Back and Forward read their page again and return to where it was scrolled", async t => {
  const page = await visit(t);
  await page.evaluate(() => scrollTo(0, 900));
  await page.evaluate(() => document.querySelector('nav a[href="/notes/1"]').click());
  await navigated(page);
  assert.equal(await page.evaluate(() => scrollY), 0);
  await page.evaluate(() => history.back());
  await navigated(page, 2);
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(await page.evaluate(() => scrollY), 900);
  await page.evaluate(() => history.forward());
  await navigated(page, 3);
  assert.equal(await page.locator("h1").textContent(), "First note");
  assert.equal(await marker(page), "same document");
  const hows = await page.evaluate(() => window.events.filter(e => e.type === "navigated").map(e => e.how));
  assert.deepEqual(hows, ["push", "restore", "restore"]);
});

test("a link to a place on the page, one that opts out, and a modified click are the browser's", async t => {
  const page = await visit(t);
  await page.getByRole("link", { name: "To the end" }).click();
  assert.equal(new URL(page.url()).hash, "#end");
  assert.ok(await page.evaluate(() => scrollY > 0));
  // Back to the top of the same page, without reading it.
  await page.evaluate(() => history.back());
  await page.waitForFunction(() => location.hash === "");
  // The runtime leaves a click with a modifier key alone.
  const prevented = await page.evaluate(() => {
    let prevented = null;
    addEventListener("click", event => { prevented = event.defaultPrevented; event.preventDefault(); }, { once: true });
    document.querySelector('nav a[href="/notes/2"]').dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, shiftKey: true }));
    return prevented;
  });
  assert.equal(prevented, false);
  await page.getByRole("link", { name: "First note, loaded" }).click();
  await page.waitForURL(`${fixture.origin}/notes/1`);
  assert.equal(await marker(page), null);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "navigated").length), 0);
});

test("a text file and a page with another stylesheet load as usual", async t => {
  const page = await visit(t);
  await page.getByRole("link", { name: "As text" }).click();
  await page.waitForURL(`${fixture.origin}/notes.txt`);
  assert.match(await page.textContent("body"), /First note\nSecond note/);
  await page.goBack();
  await page.evaluate(() => { window.marker = "same document"; });
  await page.getByRole("link", { name: "Styled page" }).click();
  await page.waitForURL(`${fixture.origin}/styled`);
  await page.waitForFunction(() => document.title === "Styled");
  assert.equal(await marker(page), null);
  assert.equal(await page.locator("h1").evaluate(node => getComputedStyle(node).fontStyle), "italic");
});

test("what was typed on one note does not follow to another", async t => {
  const page = await visit(t, "/notes/1");
  await page.locator("#title").fill("Unsaved draft");
  await page.getByRole("link", { name: "All notes" }).click();
  await navigated(page);
  await page.getByRole("link", { name: "Second note" }).click();
  await navigated(page, 2);
  assert.equal(await page.locator("#title").inputValue(), "Second note");
});

test("a save that goes elsewhere shows that page without a load, and Back shows the write", async t => {
  const page = await visit(t, "/notes/1");
  await page.locator("#title").fill("Renamed note");
  await page.getByRole("button", { name: "Save" }).click();
  await navigated(page);
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(await marker(page), "same document");
  assert.equal(await page.locator('nav a[href="/notes/1"]').textContent(), "Renamed note");
  await page.evaluate(() => history.back());
  await navigated(page, 2);
  assert.equal(await page.locator("h1").textContent(), "Renamed note");
});

test("a slow page shows a progress bar until it arrives", async t => {
  const page = await visit(t);
  await page.route("**/notes/2", async route => {
    await new Promise(resolve => setTimeout(resolve, 700));
    await route.continue();
  });
  await page.evaluate(() => new MutationObserver(records => {
    if (records.some(record => Array.from(record.addedNodes).some(node => node.dataset?.placeboProgress === ""))) window.progressShown = true;
  }).observe(document.documentElement, { childList: true }));
  await page.getByRole("link", { name: "Second note" }).click();
  await navigated(page);
  assert.equal(await page.evaluate(() => window.progressShown), true);
  assert.equal(await page.locator("[data-placebo-progress]").count(), 0);
});
