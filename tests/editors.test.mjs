import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const fixture = serverFixture("editors");
const card = id => `[id="editor:${id}"]`;

async function visit(t) {
  const page = await fixture.page(t);
  await page.goto(fixture.origin);
  return page;
}

async function save(page, id, title) {
  await page.locator(`#title-${id}`).fill(title);
  await page.locator(`#title-${id}`).press("Enter");
}

async function applied(page, id, outcome = "applied") {
  await page.waitForFunction(({ id, outcome }) => window.events.some(event =>
    event.type === "applied" && event.target === `editor:${id}` && event.outcome === outcome), { id, outcome });
}

test("two runtime instances bind one reusable server action", async t => {
  const page = await visit(t);
  const configs = await page.locator("form").evaluateAll(forms => forms.map(form => JSON.parse(form.dataset.placebo)));
  assert.deepEqual(configs.map(c => c.action), ["save-title", "save-title"]);
  assert.deepEqual(configs.map(c => c.target), ["editor:1", "editor:2"]);
});

test("validation refreshes surrounding markup while retaining input identity, focus and selection", async t => {
  const page = await visit(t);
  const heading = await page.locator(`${card(1)} h2`).textContent();
  await page.locator("#title-2").fill("Other editor's unsaved draft");
  await page.locator("#title-1").fill("x");
  await page.evaluate(() => {
    const input = document.querySelector("#title-1");
    window.originalInput = input;
    window.originalHeading = document.querySelector('[id="editor:1"] h2');
    input.setSelectionRange(0, 1);
    input.form.requestSubmit();
  });
  await applied(page, 1, "invalid");
  assert.equal(await page.locator(`${card(1)} h2`).textContent(), heading);
  assert.equal(await page.locator("#title-2").inputValue(), "Other editor's unsaved draft");
  assert.match(await page.locator("#feedback-1").textContent(), /3 and 80/);
  assert.deepEqual(await page.evaluate(() => ({
    sameInput: document.querySelector("#title-1") === window.originalInput,
    newHeading: document.querySelector('[id="editor:1"] h2') !== window.originalHeading,
    focused: document.activeElement === window.originalInput,
    selection: [window.originalInput.selectionStart, window.originalInput.selectionEnd],
    value: window.originalInput.value,
  })), { sameInput: true, newHeading: true, focused: true, selection: [0, 1], value: "x" });
});

test("typing during a save survives its older server-rendered response", async t => {
  const page = await visit(t);
  await page.locator("#delay-1").selectOption("600");
  await save(page, 1, "Submitted title");
  await page.waitForFunction(() => window.events.some(e => e.type === "request"));
  await page.locator("#title-1").fill("A newer unsaved draft");
  await applied(page, 1);
  assert.equal(await page.locator(`${card(1)} h2`).textContent(), "Submitted title");
  assert.equal(await page.locator("#title-1").inputValue(), "A newer unsaved draft");
  assert.equal(await page.locator("#delay-1").inputValue(), "600");
  assert.equal(await page.locator(card(1)).getAttribute("aria-busy"), null);
});

test("duplicate submissions do not send another write or cancel the first", async t => {
  const page = await visit(t);
  const before = Number(await page.locator(`${card(1)} input[name=version]`).inputValue());
  await page.locator("#delay-1").selectOption("600");
  await save(page, 1, "Save exactly once");
  await page.waitForFunction(() => window.events.some(e => e.type === "request"));
  await page.locator("#title-1").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "ignored" && e.reason === "busy"));
  await applied(page, 1);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "request").length), 1);
  assert.equal(Number(await page.locator(`${card(1)} input[name=version]`).inputValue()), before + 1);
});

test("different instances may save concurrently", async t => {
  const page = await visit(t);
  await page.locator("#delay-1").selectOption("600");
  await page.locator("#delay-2").selectOption("600");
  await save(page, 1, "First independent save");
  await save(page, 2, "Second independent save");
  await page.waitForFunction(() => window.events.filter(e => e.type === "request").length === 2);
  await applied(page, 1);
  await applied(page, 2);
  assert.equal(await page.locator(`${card(1)} h2`).textContent(), "First independent save");
  assert.equal(await page.locator(`${card(2)} h2`).textContent(), "Second independent save");
});

test("another tab's write produces a conflict and preserves the losing draft", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await save(first, 1, "Won in the first tab");
  await applied(first, 1);
  await save(second, 1, "Keep this losing draft");
  await applied(second, 1, "conflict");
  assert.equal(await second.locator(`${card(1)} h2`).textContent(), "Won in the first tab");
  assert.equal(await second.locator("#title-1").inputValue(), "Keep this losing draft");
  assert.match(await second.locator("#feedback-1").textContent(), /changed elsewhere/);
  await second.evaluate(() => { window.events = []; });
  await second.locator("#title-1").press("Enter");
  await applied(second, 1);
  assert.equal(await second.locator(`${card(1)} h2`).textContent(), "Keep this losing draft");
});

test("a response never steals focus moved to the other editor", async t => {
  const page = await visit(t);
  await page.locator("#delay-1").selectOption("600");
  await save(page, 1, "Focus stays elsewhere");
  await page.locator("#title-2").focus();
  await applied(page, 1);
  assert.equal(await page.evaluate(() => document.activeElement.id), "title-2");
});

test("refresh waits for an active IME composition to finish", async t => {
  const page = await visit(t);
  await page.locator("#delay-1").selectOption("600");
  const before = await page.locator(`${card(1)} h2`).textContent();
  await save(page, 1, "Saved before composition");
  await page.evaluate(() => {
    const input = document.querySelector("#title-1");
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    input.value = "に";
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "deferred"));
  assert.equal(await page.locator(`${card(1)} h2`).textContent(), before);
  await page.evaluate(() => {
    const input = document.querySelector("#title-1");
    input.value = "日本";
    input.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
  });
  await applied(page, 1);
  assert.equal(await page.locator("#title-1").inputValue(), "日本");
  assert.equal(await page.locator(`${card(1)} h2`).textContent(), "Saved before composition");
});

test("duplicate local keys reject a response before altering the live component", async t => {
  const page = await visit(t);
  await page.route("**/actions/save-title", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.html = update.html.replace("</form>", '<input name="title" data-placebo-field="title"></form>');
    await route.fulfill({ response, body: JSON.stringify(update), contentType: "application/vnd.placebo.update+json" });
  });
  const heading = await page.locator(`${card(1)} h2`).textContent();
  await save(page, 1, "x");
  await page.waitForFunction(() => window.events.some(e => e.type === "error" && e.code === "duplicate-local"));
  assert.equal(await page.locator(`${card(1)} h2`).textContent(), heading);
  assert.equal(await page.locator("#title-1").inputValue(), "x");
});

test("mutation endpoint rejects a form posted from another site", async t => {
  const page = await visit(t);
  for (const headers of [{ "Sec-Fetch-Site": "cross-site" }, { Origin: "https://attacker.example" }]) {
    const response = await page.request.post(`${fixture.origin}/actions/save-title`, {
      form: { id: "1", version: "1", title: "Cross-site request" }, headers,
    });
    assert.equal(response.status(), 403);
    assert.match(await response.text(), /another website/);
  }
  await page.reload();
  assert.notEqual(await page.locator('[id="editor:1"] h2').textContent(), "Cross-site request");
});

test("dev static edits reload the browser and serve the new bytes", { skip: !process.env.PLACEBO_TEST_DEV }, async t => {
  const page = await visit(t);
  await page.waitForFunction(() => window.reloadReady);
  const path = "examples/static/editors.css";
  const before = await readFile(path, "utf8");
  const marker = `probe-${Date.now()}`;
  try {
    const reloaded = page.waitForEvent("load");
    await writeFile(path, `${before}\n:root { --placebo-probe: ${marker}; }\n`);
    await reloaded;
    assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--placebo-probe").trim()), marker);
  } finally {
    const restored = page.waitForEvent("load");
    await writeFile(path, before);
    await restored;
    await page.waitForFunction(() => window.reloadReady);
  }
});

test("editors render at desktop and mobile sizes", async t => {
  const page = await visit(t);
  if (process.env.PLACEBO_SCREENSHOTS) {
    await mkdir("test-results", { recursive: true });
    await page.screenshot({ path: "test-results/editors-desktop.png", fullPage: true });
  }
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  if (process.env.PLACEBO_SCREENSHOTS) await page.screenshot({ path: "test-results/editors-mobile.png", fullPage: true });
});

test("submitting from the button keeps focus in the editor and moves it to the invalid field", async t => {
  const page = await visit(t);
  await page.evaluate(() => { window.status1 = document.querySelector("#feedback-1"); });
  await page.locator("#title-1").fill("x");
  await page.locator(`${card(1)} button[type=submit]`).focus();
  await page.keyboard.press("Enter");
  await applied(page, 1, "invalid");
  assert.equal(await page.evaluate(() => document.activeElement.id), "title-1");
  assert.equal(await page.locator("#title-1").getAttribute("aria-invalid"), "true");
  // The status element is the same node, so screen readers announce the new text.
  assert.ok(await page.evaluate(() => window.status1 === document.querySelector("#feedback-1")));
  assert.match(await page.locator("#feedback-1").textContent(), /3 and 80/);

  await page.locator("#title-1").fill("A valid title again");
  await page.evaluate(() => { window.events = []; });
  await page.locator(`${card(1)} button[type=submit]`).focus();
  await page.keyboard.press("Enter");
  await applied(page, 1);
  assert.ok(await page.evaluate(() => document.activeElement.matches('[id="editor:1"] button[type=submit]')));
  assert.equal(await page.locator("#title-1").getAttribute("aria-invalid"), null);
  assert.ok(await page.evaluate(() => window.status1 === document.querySelector("#feedback-1")));
});

test("an untouched field shows the value another tab saved", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await second.locator("#delay-2").selectOption("600");
  await save(first, 2, "Saved in the first tab");
  await applied(first, 2);
  // The second tab only changed the delay; the conflict shows the new title.
  await second.locator("#title-2").press("Enter");
  await applied(second, 2, "conflict");
  assert.equal(await second.locator("#title-2").inputValue(), "Saved in the first tab");
  assert.equal(await second.locator("#delay-2").inputValue(), "600");
});
