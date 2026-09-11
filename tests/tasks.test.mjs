import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

const fixture = serverFixture("tasks");
const row = id => `[data-task="${id}"]`;
const component = id => `[id="task:${id}"]`;

async function visit(t) {
  const page = await fixture.page(t);
  await page.goto(fixture.origin);
  return page;
}
async function edit(page, id) {
  await page.locator(`${row(id)} [data-dialog-open]`).click();
  await page.locator(`${row(id)} dialog`).waitFor({ state: "visible" });
}
async function submit(page, id, title) {
  await page.locator(`#title-${id}`).fill(title);
  await page.locator(`#title-${id}`).press("Enter");
}
async function applied(page, target, outcome = "applied") {
  await page.waitForFunction(({ target, outcome }) => window.events.some(e =>
    e.type === "applied" && e.target === target && e.outcome === outcome), { target, outcome });
}
async function failure(page, code) {
  await page.waitForFunction(code => window.events.some(e => e.type === "error" && e.code === code), code);
}

test("a save updates the row and completed count together and accepts normalized text", async t => {
  const page = await visit(t);
  const before = Number(await page.locator("#task-count strong").textContent());
  await edit(page, 1);
  const wasDone = await page.locator("#done-1").inputValue() === "true";
  await page.locator("#done-1").selectOption(wasDone ? "false" : "true");
  await submit(page, 1, "  Make   room for something good  ");
  await applied(page, "task:1");
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), "Make room for something good");
  assert.equal(await page.locator("#task-count strong").textContent(), String(before + (wasDone ? -1 : 1)));
  assert.equal(await page.locator("#title-1").inputValue(), "Make room for something good");
  assert.equal(await page.locator(`${row(1)} dialog`).evaluate(dialog => dialog.open), false);
  await page.waitForFunction(() => document.activeElement.matches('[data-task="1"] [data-dialog-open]'));
});

test("validation leaves the dialog open and retains the draft node and selection", async t => {
  const page = await visit(t);
  const heading = await page.locator(`${row(1)} .task-title`).textContent();
  await edit(page, 1);
  await page.locator("#title-1").fill("x");
  await page.evaluate(() => {
    window.input = document.querySelector("#title-1");
    input.setSelectionRange(0, 1);
    input.form.requestSubmit();
  });
  await applied(page, "task:1", "invalid");
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), heading);
  assert.match(await page.locator("#feedback-1").textContent(), /3 and 80/);
  assert.deepEqual(await page.evaluate(() => ({
    same: input === document.querySelector("#title-1"), focused: input === document.activeElement,
    value: input.value, selection: [input.selectionStart, input.selectionEnd],
    open: input.closest("dialog").open,
  })), { same: true, focused: true, value: "x", selection: [0, 1], open: true });
});

test("typing during a save prevents the requested reset and keeps the edit dialog open", async t => {
  const page = await visit(t);
  await edit(page, 1);
  await page.locator("#delay-1").selectOption("700");
  await submit(page, 1, "First submitted thought");
  await page.waitForFunction(() => window.events.some(e => e.type === "request"));
  await page.locator("#title-1").fill("A newer unfinished thought");
  await applied(page, "task:1");
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), "First submitted thought");
  assert.equal(await page.locator("#title-1").inputValue(), "A newer unfinished thought");
  assert.equal(await page.locator(`${row(1)} dialog`).evaluate(dialog => dialog.open), true);
  assert.deepEqual(await page.evaluate(() => window.events.find(e => e.type === "applied").resetLocal), []);
});

test("saving one row preserves another closed dialog's draft and node identity", async t => {
  const page = await visit(t);
  await edit(page, 2);
  await page.locator("#title-2").fill("Keep this second draft");
  await page.evaluate(() => { window.second = document.querySelector("#title-2"); });
  await page.locator(`${row(2)} [data-dialog-close]`).click();
  await edit(page, 1);
  await submit(page, 1, "Only change the first row");
  await applied(page, "task:1");
  await edit(page, 2);
  assert.equal(await page.locator("#title-2").inputValue(), "Keep this second draft");
  assert.ok(await page.evaluate(() => second === document.querySelector("#title-2")));
});

test("adding appends an editable row, resets the composer and preserves existing editors", async t => {
  const page = await visit(t);
  const before = await page.locator(".task-row").count();
  await page.evaluate(() => {
    window.existing = document.querySelector("#title-2");
    existing.value = "Untouched by append";
  });
  await page.locator("#add-task").click();
  await page.locator("#new-title").fill("  A newly   mounted task ");
  await page.locator("#new-title").press("Enter");
  await applied(page, "composer:new");
  assert.equal(await page.locator(".task-row").count(), before + 1);
  assert.equal(await page.locator("#new-title").inputValue(), "");
  assert.ok(await page.evaluate(() => existing === document.querySelector("#title-2") && existing.value === "Untouched by append"));
  const added = page.locator(".task-row").last();
  const id = await added.getAttribute("data-task");
  assert.equal(await added.locator(".task-title").textContent(), "A newly mounted task");
  await added.locator("[data-dialog-open]").click();
  await submit(page, id, "Edited after append");
  await applied(page, `task:${id}`);
  assert.equal(await added.locator(".task-title").textContent(), "Edited after append");
});

test("create validation preserves the draft and does not append", async t => {
  const page = await visit(t);
  const count = await page.locator(".task-row").count();
  await page.locator("#add-task").click();
  await page.locator("#new-title").fill("x");
  await page.locator("#new-title").press("Enter");
  await applied(page, "composer:new", "invalid");
  assert.equal(await page.locator(".task-row").count(), count);
  assert.equal(await page.locator("#new-title").inputValue(), "x");
  assert.ok(await page.locator("#new-title").evaluate(input => input.closest("dialog").open));
});

test("out-of-order independent saves cannot roll back a shared snapshot", async t => {
  const page = await visit(t);
  let releaseFirst;
  const held = new Promise(resolve => { releaseFirst = resolve; });
  t.after(() => releaseFirst());
  let firstCommitted;
  const committed = new Promise(resolve => { firstCommitted = resolve; });
  await page.route("**/actions/save-task", async route => {
    if (new URLSearchParams(route.request().postData()).get("id") !== "1") return route.continue();
    const response = await route.fetch();
    firstCommitted();
    await held;
    await route.fulfill({ response });
  });
  await edit(page, 1);
  await page.locator("#done-1").selectOption("true");
  await submit(page, 1, "Older snapshot delivered last");
  await committed;
  await page.locator(`${row(1)} [data-dialog-close]`).click();
  await edit(page, 2);
  await page.locator("#done-2").selectOption("true");
  await submit(page, 2, "Newer snapshot delivered first");
  await applied(page, "task:2");
  const newest = await page.locator("#task-count").innerHTML();
  const revision = await page.locator("#task-count").getAttribute("data-placebo-revision");
  releaseFirst();
  await applied(page, "task:1");
  assert.equal(await page.locator("#task-count").innerHTML(), newest);
  assert.equal(await page.locator("#task-count").getAttribute("data-placebo-revision"), revision);
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), "Older snapshot delivered last");
  assert.deepEqual(await page.evaluate(() => window.events.find(e => e.type === "applied" && e.target === "task:1").skippedRegions), ["task-count"]);
});

test("a malformed additional patch rejects the whole response before changing any DOM", async t => {
  const page = await visit(t);
  const summary = await page.locator("#task-count").innerHTML();
  const title = await page.locator(`${row(1)} .task-title`).textContent();
  await page.route("**/actions/save-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.patches.at(-1).revision = "invalid";
    await route.fulfill({ response, body: JSON.stringify(update), contentType: "application/vnd.placebo.update+json" });
  });
  await edit(page, 1);
  const version = await page.locator(`${component(1)} input[name=version]`).inputValue();
  await submit(page, 1, "Committed but rejected in the browser");
  await failure(page, "invalid-revision");
  assert.equal(await page.locator("#task-count").innerHTML(), summary);
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), title);
  assert.equal(await page.locator(`${component(1)} input[name=version]`).inputValue(), version);
  assert.equal(await page.locator("#title-1").inputValue(), "Committed but rejected in the browser");
});

test("remounting an additional region invalidates the response's captured ownership", async t => {
  const page = await visit(t);
  await edit(page, 1);
  await page.locator("#delay-1").selectOption("700");
  await submit(page, 1, "Do not update a new summary instance");
  await page.waitForFunction(() => window.events.some(e => e.type === "request"));
  const title = await page.locator(`${row(1)} .task-title`).textContent();
  await page.evaluate(() => {
    const summary = document.querySelector("#task-count");
    summary.replaceWith(summary.cloneNode(true));
  });
  await failure(page, "remounted-target");
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), title);
  assert.ok(await page.locator(`${row(1)} dialog`).evaluate(dialog => dialog.open));
});

test("an undeclared target and duplicate append are rejected before primary reset", async t => {
  const page = await visit(t);
  await page.route("**/actions/add-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.patches[0].html = '<div id="task-count">Duplicate</div>';
    await route.fulfill({ response, body: JSON.stringify(update), contentType: "application/vnd.placebo.update+json" });
  });
  const count = await page.locator(".task-row").count();
  await page.locator("#add-task").click();
  await page.locator("#new-title").fill("Duplicate append rejected");
  await page.locator("#new-title").press("Enter");
  await failure(page, "duplicate-append");
  assert.equal(await page.locator("#new-title").inputValue(), "Duplicate append rejected");
  assert.equal(await page.locator(".task-row").count(), count);
  await page.unroute("**/actions/add-task");
  await page.route("**/actions/add-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.patches[0].target = "task:1";
    await route.fulfill({ response, body: JSON.stringify(update), contentType: "application/vnd.placebo.update+json" });
  });
  await page.locator("#new-title").press("Enter");
  await failure(page, "invalid-patch");
  assert.equal(await page.locator(".task-row").count(), count);
});

test("composition defers the entire update and keeps the new draft when it finishes", async t => {
  const page = await visit(t);
  const summary = await page.locator("#task-count").innerHTML();
  const title = await page.locator(`${row(1)} .task-title`).textContent();
  await edit(page, 1);
  await page.locator("#delay-1").selectOption("700");
  await submit(page, 1, "Save before composing");
  await page.evaluate(() => {
    const input = document.querySelector("#title-1");
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    input.value = "に";
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "deferred"));
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), title);
  assert.equal(await page.locator("#task-count").innerHTML(), summary);
  await page.evaluate(() => {
    const input = document.querySelector("#title-1");
    input.value = "日本";
    input.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
  });
  await applied(page, "task:1");
  assert.equal(await page.locator("#title-1").inputValue(), "日本");
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), "Save before composing");
});

test("behavior lifecycle retains moved locals, cleans removed nodes, and restarts without duplicate handlers", async t => {
  const page = await visit(t);
  await page.evaluate(async () => {
    window.runtime = await import("/placebo.js");
    window.lifecycle = { setup: 0, cleanup: 0, clicks: 0 };
    window.unregister = runtime.behavior("probe", element => {
      lifecycle.setup++;
      const onClick = () => lifecycle.clicks++;
      element.addEventListener("click", onClick);
      return () => { lifecycle.cleanup++; element.removeEventListener("click", onClick); };
    });
    document.querySelector('#title-1').closest('[data-placebo-local]').dataset.placeboBehavior = "probe";
  });
  await page.waitForFunction(() => lifecycle.setup === 1);
  await edit(page, 1);
  await submit(page, 1, "x");
  await applied(page, "task:1", "invalid");
  assert.deepEqual(await page.evaluate(() => lifecycle), { setup: 1, cleanup: 0, clicks: 0 });
  await page.evaluate(() => { runtime.stop(); runtime.start(); document.querySelector("#title-1").click(); });
  assert.deepEqual(await page.evaluate(() => lifecycle), { setup: 2, cleanup: 1, clicks: 1 });
  await page.evaluate(() => document.querySelector('[data-task="1"]').remove());
  await page.waitForFunction(() => lifecycle.cleanup === 2);
  await page.evaluate(() => { unregister(); runtime.stop(); runtime.start(); });
  assert.deepEqual(await page.evaluate(() => lifecycle), { setup: 2, cleanup: 2, clicks: 1 });
});

test("a server conflict keeps the losing dialog draft and refreshes the saved summary", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await edit(first, 1);
  await submit(first, 1, "Won in another tab");
  await applied(first, "task:1");
  await edit(second, 1);
  await submit(second, 1, "A losing draft to review");
  await applied(second, "task:1", "conflict");
  assert.equal(await second.locator("#title-1").inputValue(), "A losing draft to review");
  assert.equal(await second.locator(`${row(1)} .task-title`).textContent(), "Won in another tab");
  assert.match(await second.locator("#feedback-1").textContent(), /changed elsewhere/);
  assert.ok(await second.locator(`${row(1)} dialog`).evaluate(dialog => dialog.open));
});

test("Escape closes the native dialog without a request and preserves the draft", async t => {
  const page = await visit(t);
  await edit(page, 1);
  await page.locator("#title-1").fill("Come back to this");
  await page.keyboard.press("Escape");
  await page.locator(`${row(1)} dialog`).waitFor({ state: "hidden" });
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "request").length), 0);
  await edit(page, 1);
  assert.equal(await page.locator("#title-1").inputValue(), "Come back to this");
});

test("task list and dialog fit desktop and mobile", async t => {
  const page = await visit(t);
  if (process.env.PLACEBO_SCREENSHOTS) {
    await mkdir("test-results", { recursive: true });
    await page.screenshot({ path: "test-results/tasks-desktop.png", fullPage: true });
  }
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  if (process.env.PLACEBO_SCREENSHOTS) await page.screenshot({ path: "test-results/tasks-mobile.png", fullPage: true });
  await page.locator("#add-task").click();
  assert.ok(await page.locator("dialog[open]").evaluate(dialog => dialog.scrollWidth <= dialog.clientWidth));
  if (process.env.PLACEBO_SCREENSHOTS) await page.screenshot({ path: "test-results/tasks-dialog.png", fullPage: true });
});
