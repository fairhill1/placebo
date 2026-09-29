import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Mutation forms are plain HTML forms. These tests submit them with
// JavaScript disabled, or before the runtime has loaded.
const fixture = serverFixture("editors");

async function visit(t, options = { javaScriptEnabled: false }) {
  const page = await fixture.page(t, options);
  await page.goto(fixture.origin);
  return page;
}

async function nativeSave(page, id, title, waitUntil = "load") {
  await page.locator(`#title-${id}`).fill(title);
  const navigation = page.waitForNavigation({ waitUntil });
  await page.locator(`#title-${id}`).press("Enter");
  return navigation;
}

test("a save without JavaScript redirects back and shows the saved title", async t => {
  const page = await visit(t);
  const title = `Saved natively ${Date.now()}`;
  const response = await nativeSave(page, 1, title);
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(response.status(), 200);
  assert.equal(response.request().method(), "GET", "post/redirect/get");
  assert.equal(await page.locator('[id="editor:1"] h2').textContent(), title);
  assert.equal(await page.locator("#title-1").inputValue(), title);
});

test("an invalid save without JavaScript renders the whole page with the draft and feedback", async t => {
  const page = await visit(t);
  await page.locator("#title-2").fill("A draft in the other editor");
  const response = await nativeSave(page, 1, "x");
  assert.equal(response.status(), 422);
  assert.equal(await page.locator("h1").count(), 1, "the whole page, not only the component");
  assert.equal(await page.locator("#title-1").inputValue(), "x");
  assert.match(await page.locator("#feedback-1").textContent(), /3 and 80/);
  assert.equal(await page.locator("#title-1").getAttribute("aria-invalid"), "true");
  assert.ok(await page.locator("#title-1").evaluate(input => input === document.activeElement));
  // The fixed title can be saved from the rejected page.
  const saved = await nativeSave(page, 1, "Fixed after a native rejection");
  assert.equal(saved.status(), 200);
  assert.equal(await page.locator('[id="editor:1"] h2').textContent(), "Fixed after a native rejection");
});

test("a conflict without JavaScript keeps this person's edit and takes the new version", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await nativeSave(first, 2, "Won in the first tab");
  const response = await nativeSave(second, 2, "Typed in the second tab");
  assert.equal(response.status(), 409);
  assert.equal(await second.locator("#title-2").inputValue(), "Typed in the second tab");
  assert.equal(await second.locator('[id="editor:2"] h2').textContent(), "Won in the first tab");
  assert.match(await second.locator("#feedback-2").textContent(), /changed elsewhere/);
  const retry = second.waitForNavigation();
  await second.locator("#title-2").press("Enter");
  assert.equal((await retry).status(), 200);
  assert.equal(await second.locator('[id="editor:2"] h2').textContent(), "Typed in the second tab");
});

test("a save submitted while the runtime is still loading is handled, not refused", async t => {
  const page = await fixture.page(t);
  let release;
  const held = new Promise(resolve => { release = resolve; });
  t.after(() => release());
  await page.route("**/placebo.js", async route => { await held; await route.continue(); });
  await page.goto(fixture.origin, { waitUntil: "commit" });
  await page.locator("#title-1").waitFor();
  const title = `Before the runtime ${Date.now()}`;
  // The next page waits for the held runtime too, so wait for its response only.
  const response = await nativeSave(page, 1, title, "commit");
  assert.equal(response.status(), 200);
  await page.locator('[id="editor:1"] h2', { hasText: title }).waitFor();
});

test("the same form submitted twice without JavaScript saves once", async t => {
  const page = await visit(t);
  await page.locator("#title-1").fill(`Submitted twice ${Date.now()}`);
  const version = await page.locator('[id="editor:1"] .version').textContent();
  // What the browser posts for this form, sent twice: a double click, or the
  // Back button and submit again.
  const form = await page.locator('[id="editor:1"] form').evaluate(form =>
    Array.from(form.querySelectorAll("[name]"), control => [control.name, control.value]));
  const headers = { Origin: fixture.origin, Referer: `${fixture.origin}/`, "Sec-Fetch-Site": "same-origin" };
  for (let attempt = 0; attempt < 2; attempt++) {
    const response = await page.request.post(`${fixture.origin}/actions/save-title`,
      { form: Object.fromEntries(form), headers, maxRedirects: 0 });
    assert.equal(response.status(), 303, "both attempts redirect back, the second one replayed");
  }
  await page.reload();
  const saved = Number(version.match(/\d+/)[0]) + 1;
  assert.equal(await page.locator('[id="editor:1"] .version').textContent(), `Version ${saved}`);
});
