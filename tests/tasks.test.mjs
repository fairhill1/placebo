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
  assert.deepEqual(await page.evaluate(() => window.events.find(e => e.type === "applied").preservedLocal),
    [{ key: "/actions/save-task#title", reason: "edited-since-submission" }]);
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
    update.patches[0].item = "tasks/1";
    update.patches[0].html = '<div id="tasks/1" data-placebo-item>Duplicate</div>';
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
    document.querySelector('#title-1').dataset.placeboBehavior = "probe";
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


test("mounted dialog survives invalid, successful and repeated creates as the same native node", async t => {
  const page = await visit(t);
  await page.locator("#add-task").click();
  await page.evaluate(() => { window.composerDialog = document.getElementById("composer:new"); });
  assert.equal(await page.evaluate(() => composerDialog.tagName), "DIALOG");
  await page.locator("#new-title").fill("x");
  await page.locator("#new-title").press("Enter");
  await applied(page, "composer:new", "invalid");
  assert.ok(await page.evaluate(() => composerDialog === document.getElementById("composer:new") && composerDialog.open));
  assert.equal(await page.locator("#add-heading").count(), 1);
  await page.locator("#new-title").fill("A valid second attempt");
  await page.locator("#new-title").press("Enter");
  await applied(page, "composer:new");
  assert.ok(await page.evaluate(() => composerDialog === document.getElementById("composer:new") && !composerDialog.open));
  await page.locator("#add-task").click();
  assert.ok(await page.evaluate(() => composerDialog.open));
  await page.keyboard.press("Escape");
  assert.ok(await page.evaluate(() => !composerDialog.open));
});

test("dialog nested in replaceable contents fails before sending a write", async t => {
  const page = await visit(t);
  const requests = [];
  page.on("request", request => { if (request.method() === "POST") requests.push(request.url()); });
  await page.evaluate(() => {
    const component = document.getElementById("composer:new");
    const wrapper = document.createElement("div");
    for (const name of ["id", "data-placebo-region", "data-placebo-component"]) {
      wrapper.setAttribute(name, component.getAttribute(name)); component.removeAttribute(name);
    }
    component.before(wrapper); wrapper.append(component);
    window.unsafeDialog = component;
    component.showModal();
  });
  await page.locator("#new-title").fill("Never send this write");
  await page.locator("#new-title").press("Enter");
  await failure(page, "unstable-dialog");
  assert.equal(requests.length, 0);
  assert.ok(await page.evaluate(() => unsafeDialog.isConnected && unsafeDialog.open));
  const diagnostic = await page.evaluate(() => events.find(e => e.code === "unstable-dialog"));
  assert.equal(diagnostic.target, "composer:new");
  assert.equal(diagnostic.writeState, "not-started");
  assert.match(diagnostic.hint, /mount_dialog/);
});

test("incoming dialog wrapper rejects the entire update and retains the open dialog", async t => {
  const page = await visit(t);
  await page.route("**/actions/add-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.html = `<dialog>${update.html}</dialog>`;
    await route.fulfill({ response, body: JSON.stringify(update) });
  });
  const count = await page.locator(".task-row").count();
  const summary = await page.locator("#task-count").innerHTML();
  await page.locator("#add-task").click();
  await page.locator("#new-title").fill("Committed but malformed reply");
  await page.locator("#new-title").press("Enter");
  await failure(page, "unstable-dialog");
  assert.equal(await page.locator(".task-row").count(), count);
  assert.equal(await page.locator("#task-count").innerHTML(), summary);
  assert.equal(await page.locator("#new-title").inputValue(), "Committed but malformed reply");
  assert.ok(await page.locator('[id="composer:new"]').evaluate(dialog => dialog.open));
});

const order = page => page.$$eval("#tasks > [data-placebo-item]", items => items.map(item => item.id));

async function addTask(page, title) {
  await page.locator("#add-task").click();
  await page.locator("#new-title").fill(title);
  await page.locator("#new-title").press("Enter");
  await page.waitForFunction(title => Array.from(document.querySelectorAll(".task-title")).some(node => node.textContent === title), title);
  await page.keyboard.press("Escape");
  return page.locator(".task-row", { hasText: title }).getAttribute("data-task");
}

test("moving a task keeps its row node, the draft in its dialog, and focus on the button", async t => {
  const page = await visit(t);
  await edit(page, 2);
  await page.locator("#title-2").fill("Draft while moving");
  // The dialog's close event returns focus to its Edit button in a later task.
  await page.evaluate(() => new Promise(resolve => {
    const dialog = document.querySelector('[data-task="2"] dialog');
    dialog.addEventListener("close", () => setTimeout(resolve, 0), { once: true });
    dialog.querySelector("[data-dialog-close]").click();
  }));
  await page.evaluate(() => { window.moved = document.getElementById("tasks/2"); window.draft = document.querySelector("#title-2"); });
  const before = await order(page);
  const index = before.indexOf("tasks/2");
  await page.locator(`${row(2)} button[aria-label="Move up task 2"]`).focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(index => document.querySelectorAll("#tasks > [data-placebo-item]")[index - 1]?.id === "tasks/2", index);
  const after = await order(page);
  assert.deepEqual(after, [...before.slice(0, index - 1), "tasks/2", before[index - 1], ...before.slice(index + 1)]);
  assert.ok(await page.evaluate(() => moved === document.getElementById("tasks/2") && draft === document.querySelector("#title-2")));
  assert.equal(await page.locator("#title-2").inputValue(), "Draft while moving");
  assert.ok(await page.evaluate(() => document.activeElement.matches('[data-task="2"] button[aria-label="Move up task 2"]')));
  await page.locator(`${row(2)} button[aria-label="Move down task 2"]`).click();
  await page.waitForFunction(before => JSON.stringify(Array.from(document.querySelectorAll("#tasks > [data-placebo-item]"), n => n.id)) === JSON.stringify(before), before);
});

test("deleting a task removes its row, updates the count, and moves focus to a neighbour", async t => {
  const page = await visit(t);
  const id = await addTask(page, "Delete me soon");
  const count = await page.locator(".task-row").count();
  const total = await page.locator("#task-count").textContent();
  await edit(page, id);
  await page.locator(`${row(id)} button.danger`).focus();
  await page.keyboard.press("Enter");
  await page.locator(row(id)).waitFor({ state: "detached" });
  assert.equal(await page.locator(".task-row").count(), count - 1);
  assert.notEqual(await page.locator("#task-count").textContent(), total);
  // It was the last row, so focus moves to the row before it.
  assert.ok(await page.evaluate(() => document.activeElement.closest("[data-placebo-item]") === document.querySelector("#tasks > [data-placebo-item]:last-child")));
  assert.equal(await page.locator("dialog[open]").count(), 0);
});

test("an item order places listed items first and reports items that are gone", async t => {
  const page = await visit(t);
  const before = await order(page);
  const reversed = [...before].reverse();
  await page.route("**/actions/move-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.patches.push({ target: "tasks", operation: "order-items", items: [...reversed, "tasks/999"] },
      { target: "tasks", operation: "remove-item", item: "tasks/998" });
    await route.fulfill({ response, body: JSON.stringify(update) });
  });
  const first = before[0].split("/")[1];
  await page.locator(`${row(first)} button[aria-label="Move up task ${first}"]`).click();
  await applied(page, `task-order:${first}`);
  assert.deepEqual(await order(page), reversed);
  assert.deepEqual(await page.evaluate(() => window.events.find(e => e.type === "applied").missingItems), ["tasks/999", "tasks/998"]);
  await page.unroute("**/actions/move-task");
  // Put the server's order back for the other tests.
  await page.reload();
});

test("a reply can refresh another declared component, keeping only its edited fields", async t => {
  const page = await visit(t);
  await edit(page, 1);
  const done = await page.locator("#done-1").inputValue();
  const other = done === "true" ? "false" : "true";
  await page.locator("#done-1").selectOption(other);
  await page.locator(`${row(1)} [data-dialog-close]`).click();
  const incoming = await page.evaluate(() => {
    const form = document.querySelector('[id="task-order:1"] form');
    const config = JSON.parse(form.dataset.placebo);
    for (const node of document.querySelectorAll('[id="task-order:1"] form')) node.dataset.placebo = JSON.stringify({ ...config, effects: [...config.effects, "task:1"] });
    const copy = document.getElementById("task:1").cloneNode(true);
    copy.querySelector("#title-1").setAttribute("value", "Title from another write");
    copy.querySelector("#feedback-1").textContent = "Refreshed by another action.";
    return copy.innerHTML;
  });
  await page.route("**/actions/move-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.patches.push({ target: "task:1", operation: "refresh-component", html: incoming });
    await route.fulfill({ response, body: JSON.stringify(update) });
  });
  await page.locator(`${row(1)} button[aria-label="Move down task 1"]`).click();
  await applied(page, "task-order:1");
  assert.deepEqual(await page.evaluate(() => window.events.find(e => e.type === "applied").refreshedComponents), ["task:1"]);
  assert.equal(await page.locator("#title-1").inputValue(), "Title from another write");
  assert.equal(await page.locator("#done-1").inputValue(), other);
  assert.equal(await page.locator("#feedback-1").textContent(), "Refreshed by another action.");
  await page.unroute("**/actions/move-task");
  await page.locator(`${row(1)} button[aria-label="Move up task 1"]`).click();
  await applied(page, "task-order:1");
});

test("a conflict shows the other tab's status when this tab only changed the title", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await edit(first, 2);
  const done = await first.locator("#done-2").inputValue();
  const flipped = done === "true" ? "false" : "true";
  await first.locator("#done-2").selectOption(flipped);
  await first.locator("#title-2").press("Enter");
  await applied(first, "task:2");
  await edit(second, 2);
  await submit(second, 2, "Only the title changed here");
  await applied(second, "task:2", "conflict");
  assert.equal(await second.locator("#done-2").inputValue(), flipped);
  assert.equal(await second.locator("#title-2").inputValue(), "Only the title changed here");
  // Saving again keeps the other tab's status instead of reverting it.
  await second.evaluate(() => { window.events = []; });
  await second.locator("#title-2").press("Enter");
  await applied(second, "task:2");
  await first.reload();
  await edit(first, 2);
  assert.equal(await first.locator("#done-2").inputValue(), flipped);
  assert.equal(await first.locator("#title-2").inputValue(), "Only the title changed here");
});

test("a successful reply can navigate within the site, and nowhere else", async t => {
  const page = await visit(t);
  let destination = "//example.com/";
  await page.route("**/actions/save-task", async route => {
    const response = await route.fetch();
    const update = await response.json();
    update.navigate = destination;
    await route.fulfill({ response, body: JSON.stringify(update) });
  });
  await edit(page, 1);
  const title = await page.locator(`${row(1)} .task-title`).textContent();
  await submit(page, 1, "Not applied: the navigation is refused");
  await failure(page, "cross-origin-navigation");
  assert.equal(await page.locator(`${row(1)} .task-title`).textContent(), title);
  assert.equal(await page.locator('[id="task:1"]').getAttribute("data-placebo-stale"), "");
  // The write committed but the page could not show it; reload before saving again.
  destination = "/?from=reply";
  await page.reload();
  await edit(page, 1);
  await submit(page, 1, "Saved, then navigated");
  await page.waitForURL("**/?from=reply");
});

test("moving a task without JavaScript redirects back to the page in its new order", async t => {
  const page = await fixture.page(t, { javaScriptEnabled: false });
  await page.goto(fixture.origin);
  const before = await order(page);
  const first = before[0].split("/")[1];
  const navigation = page.waitForNavigation();
  await page.locator(`${row(first)} button[aria-label="Move down task ${first}"]`).click();
  assert.equal((await navigation).status(), 200);
  // The reply's also_move is moot: the redirected page renders the new order.
  assert.deepEqual(await order(page), [before[1], before[0], ...before.slice(2)]);
  const back = page.waitForNavigation();
  await page.locator(`${row(first)} button[aria-label="Move up task ${first}"]`).click();
  await back;
  assert.deepEqual(await order(page), before);
});

test("an open edit dialog stays modal, with focus, when its row moves", async t => {
  const page = await visit(t);
  await edit(page, 2);
  await page.locator("#title-2").fill("Typing while the row moves");
  const moved = page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.target === "task-order:2"));
  await page.evaluate(() => document.querySelector('[id="task-order:2"] form').requestSubmit());
  await moved;
  assert.deepEqual(await page.evaluate(() => {
    const dialog = document.querySelector('[data-task="2"] dialog');
    return { modal: dialog.matches(":modal"), focused: document.activeElement === document.querySelector("#title-2"),
      value: document.querySelector("#title-2").value };
  }), { modal: true, focused: true, value: "Typing while the row moves" });
  await page.keyboard.press("Escape");
  // Put the order back for the other tests.
  await page.evaluate(() => document.querySelector('[id="task-order:2"] form:last-of-type').requestSubmit());
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied" && e.target === "task-order:2").length === 2);
});
