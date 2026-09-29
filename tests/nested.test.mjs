import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// A checklist component mounts entry components and a notes dialog component.
const fixture = serverFixture("nested");

async function visit(t) {
  const page = await fixture.page(t);
  await page.goto(fixture.origin);
  return page;
}
async function applied(page, target, count = 1) {
  await page.waitForFunction(({ target, count }) =>
    window.events.filter(e => e.type === "applied" && e.target === target).length >= count, { target, count });
}
const lastApplied = (page, target) => page.evaluate(target => window.events.filter(e => e.type === "applied" && e.target === target).at(-1), target);
async function saveList(page, { name, locked, keep } = {}) {
  if (name !== undefined) await page.locator("#name").fill(name);
  if (locked !== undefined) await page.locator("#locked").setChecked(locked);
  if (keep !== undefined) await page.locator("#keep").fill(String(keep));
  await page.locator('[id="checklist:1"] > form button[type=submit]').click();
}

test("a checklist refresh keeps its entries' nodes and drafts and refreshes their other markup", async t => {
  const page = await visit(t);
  await page.locator("#entry-2").fill("A draft in entry two");
  await page.evaluate(() => {
    window.nodes = { entry: document.getElementById("entry:2"), input: document.querySelector("#entry-2"),
      notes: document.getElementById("notes:1") };
    window.nodes.notes.showModal();
    window.nodes.notes.close();
  });
  await page.locator("#name").focus();
  await saveList(page, { name: "Renamed list", locked: true });
  await applied(page, "checklist:1");
  const event = await lastApplied(page, "checklist:1");
  assert.deepEqual(event.refreshedComponents.sort(), ["entry:1", "entry:2", "entry:3", "notes:1"]);
  assert.deepEqual(event.skippedComponents, []);
  assert.deepEqual(await page.evaluate(() => ({
    entry: window.nodes.entry === document.getElementById("entry:2"),
    input: window.nodes.input === document.querySelector("#entry-2"),
    notes: window.nodes.notes === document.getElementById("notes:1"),
    draft: document.querySelector("#entry-2").value,
    untouched: document.querySelector("#entry-1").value,
    locked: Array.from(document.querySelectorAll('[id^="entry:"] button'), button => button.disabled),
    heading: document.querySelector("#notes-heading").textContent,
  })), { entry: true, input: true, notes: true, draft: "A draft in entry two", untouched: "Thing 1",
    locked: [true, true, true], heading: "Notes for Renamed list" });
  // Unlock again for the other tests.
  await saveList(page, { locked: false });
  await applied(page, "checklist:1", 2);
});

test("an entry with its own save in flight is left for its own reply", async t => {
  const page = await visit(t);
  await page.locator("#entry-delay-2").selectOption("700");
  await page.locator("#entry-2").fill("Saved slowly");
  await page.locator("#entry-2").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "request" && e.target === "entry:2"));
  await saveList(page, { name: "Renamed while an entry saves" });
  await applied(page, "checklist:1");
  const event = await lastApplied(page, "checklist:1");
  assert.deepEqual(event.skippedComponents, [{ target: "entry:2", reason: "busy" }]);
  assert.equal(await page.locator('[id="entry:2"]').getAttribute("aria-busy"), "true");
  await applied(page, "entry:2");
  assert.equal(await page.locator('[id="entry:2"] .feedback').textContent(), "Saved.");
  assert.equal(await page.locator("#entry-2").inputValue(), "Saved slowly");
});

test("an open notes dialog stays open, with its draft, when the checklist refreshes", async t => {
  const page = await visit(t);
  await page.evaluate(() => document.getElementById("notes:1").showModal());
  await page.locator("#notes").fill("Pack the charger");
  await page.evaluate(() => {
    const form = document.querySelector('[id="checklist:1"] > form');
    form.querySelector("#name").value = "Renamed behind the dialog";
    form.requestSubmit();
  });
  await applied(page, "checklist:1");
  assert.ok(await page.locator('[id="notes:1"]').evaluate(dialog => dialog.open && dialog.matches(":modal")));
  assert.equal(await page.locator("#notes").inputValue(), "Pack the charger");
  assert.ok(await page.locator("#notes").evaluate(input => input === document.activeElement));
  assert.equal(await page.locator("#notes-heading").textContent(), "Notes for Renamed behind the dialog");
});

test("a checklist refresh that drops an entry cancels that entry's save and says the write is unknown", async t => {
  const page = await visit(t);
  await page.locator("#entry-delay-3").selectOption("700");
  await page.locator("#entry-3").fill("Removed mid-save");
  await page.locator("#entry-3").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "request" && e.target === "entry:3"));
  await saveList(page, { keep: 2 });
  await applied(page, "checklist:1");
  assert.equal(await page.locator('[id="entry:3"]').count(), 0);
  await page.waitForFunction(() => window.events.some(e => e.type === "discarded" && e.target === "entry:3"));
  const warning = await page.evaluate(() => window.events.find(e => e.code === "mutation-interrupted"));
  assert.equal(warning.reason, "unmounted");
  assert.equal(warning.writeState, "unknown");
});

test("a form inside a nested component cannot target the component around it", async t => {
  const page = await visit(t);
  await page.evaluate(() => {
    const outer = document.querySelector('[id="checklist:1"] > form');
    const inner = document.querySelector('[id="entry:1"] form');
    inner.dataset.placebo = outer.dataset.placebo;
    inner.action = outer.action;
    inner.requestSubmit();
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "error" && e.code === "invalid-component"));
  const error = await page.evaluate(() => window.events.find(e => e.code === "invalid-component"));
  assert.match(error.message, /inside nested component 'entry:1' but targets 'checklist:1'/);
  assert.equal(error.requestState, "not-started");
});

test("a nested component that changes its root element rejects the whole reply", async t => {
  const page = await visit(t);
  await page.route("**/actions/save-list", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.html = update.html.replace('<div id="entry:1"', '<section id="entry:1"');
    await route.fulfill({ response, body: JSON.stringify(update) });
  });
  const name = await page.locator("#list-name").textContent();
  await saveList(page, { name: "Never shown" });
  await page.waitForFunction(() => window.events.some(e => e.type === "error" && e.code === "nested-component"));
  assert.equal(await page.locator("#list-name").textContent(), name);
  assert.equal(await page.locator('[id="checklist:1"]').getAttribute("data-placebo-stale"), "");
});
