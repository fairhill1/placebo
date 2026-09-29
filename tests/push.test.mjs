import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Two tabs on the task list follow the same feed and update each other.
const fixture = serverFixture("tasks");
const row = id => `[data-task="${id}"]`;

async function visit(t, options) {
  const page = await fixture.page(t, options);
  await page.goto(fixture.origin);
  await connected(page);
  return page;
}
async function connected(page, count = 1) {
  await page.waitForFunction(count => window.events.filter(e => e.type === "push" &&
    ["push-connected", "push-reconnected"].includes(e.phase)).length >= count, count);
}
async function pushed(page, predicate, source = "push") {
  await page.waitForFunction(({ predicate, source }) => window.events.some(e => e.type === "applied" && e.source === source &&
    new Function("e", `return ${predicate}`)(e)), { predicate, source });
}
async function edit(page, id) {
  await page.locator(`${row(id)} [data-dialog-open]`).click();
  await page.locator(`${row(id)} dialog`).waitFor({ state: "visible" });
}
async function save(page, id, title) {
  await page.locator(`#title-${id}`).fill(title);
  await page.locator(`#title-${id}`).press("Enter");
  await page.waitForFunction(id => window.events.some(e => e.type === "applied" && e.target === `task:${id}`), id);
}

test("a save in one tab updates the row, the count, and the editor in the other", async t => {
  const first = await visit(t);
  const second = await visit(t);
  const version = await second.locator('[id="task:1"]').getAttribute("data-placebo-revision");
  await edit(first, 1);
  const done = await first.locator("#done-1").inputValue();
  await first.locator("#done-1").selectOption(done === "true" ? "false" : "true");
  await save(first, 1, "Saved in the first tab");
  await pushed(second, `e.refreshedComponents.includes("task:1")`);
  assert.equal(await second.locator(`${row(1)} .task-title`).textContent(), "Saved in the first tab");
  assert.equal(await second.locator("#task-count").innerHTML(), await first.locator("#task-count").innerHTML());
  assert.equal(await second.locator('[id="task:1"]').getAttribute("data-placebo-revision"), String(Number(version) + 1));
  // The second tab's editor has the new version, so its save is not a conflict.
  await edit(second, 1);
  assert.equal(await second.locator("#title-1").inputValue(), "Saved in the first tab");
  assert.match(await second.locator("#feedback-1").textContent(), /another tab/);
  await save(second, 1, "Then saved in the second tab");
  assert.equal(await second.evaluate(() => window.events.find(e => e.type === "applied" && e.target === "task:1").outcome), "applied");
  await pushed(first, `e.refreshedComponents.includes("task:1")`);
  assert.equal(await first.locator(`${row(1)} .task-title`).textContent(), "Then saved in the second tab");
});

test("a pushed refresh keeps a draft open in the other tab's editor", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await edit(second, 2);
  await second.locator("#title-2").fill("A draft in the second tab");
  const doneBefore = await second.locator("#done-2").inputValue();
  await edit(first, 2);
  await first.locator("#done-2").selectOption(doneBefore === "true" ? "false" : "true");
  await save(first, 2, "Renamed in the first tab");
  await pushed(second, `e.refreshedComponents.includes("task:2")`);
  // Edited: kept. Untouched: the other tab's value. The dialog stays open.
  assert.equal(await second.locator("#title-2").inputValue(), "A draft in the second tab");
  assert.equal(await second.locator("#done-2").inputValue(), doneBefore === "true" ? "false" : "true");
  assert.ok(await second.locator(`${row(2)} dialog`).evaluate(dialog => dialog.open));
  assert.equal(await second.locator(`${row(2)} .task-title`).textContent(), "Renamed in the first tab");
});

test("adding, moving, and deleting in one tab change the other tab's list", async t => {
  const first = await visit(t);
  const second = await visit(t);
  const order = page => page.$$eval("#tasks > [data-placebo-item]", items => items.map(item => item.id));
  await first.locator("#add-task").click();
  await first.locator("#new-title").fill("Pushed to the other tab");
  await first.locator("#new-title").press("Enter");
  await second.waitForFunction(() => Array.from(document.querySelectorAll(".task-title")).some(n => n.textContent === "Pushed to the other tab"));
  const id = await second.locator(".task-row", { hasText: "Pushed to the other tab" }).getAttribute("data-task");
  assert.deepEqual(await order(second), await order(first));
  // The adding tab got the insert twice, from its reply and the push. The
  // second finds it there, or, from a reply after the push, leaves it to it.
  await first.waitForFunction(() => window.events.some(e => e.type === "applied" && (e.existingItems?.length || e.supersededItems?.length)));
  await first.keyboard.press("Escape");
  await first.locator(`${row(id)} button[aria-label="Move up task ${id}"]`).click();
  await second.waitForFunction(id => {
    const items = Array.from(document.querySelectorAll("#tasks > [data-placebo-item]"), item => item.id);
    return items.indexOf(`tasks/${id}`) === items.length - 2;
  }, id);
  await first.locator(`${row(id)} [data-dialog-open]`).click();
  await first.locator(`${row(id)} button.danger`).click();
  await second.locator(row(id)).waitFor({ state: "detached" });
  assert.equal(await second.locator("#task-count").innerHTML(), await first.locator("#task-count").innerHTML());
});

test("a stale snapshot from a push is skipped and traced, not applied", async t => {
  const page = await visit(t);
  await page.evaluate(() => {
    const count = document.querySelector("#task-count");
    count.dataset.placeboRevision = "1000000";
    window.shown = count.innerHTML;
  });
  const other = await visit(t);
  await edit(other, 3);
  const done = await other.locator("#done-3").inputValue();
  await other.locator("#done-3").selectOption(done === "true" ? "false" : "true");
  await save(other, 3, "Changes the count");
  await pushed(page, `e.skippedRegions.includes("task-count")`);
  assert.ok(await page.evaluate(() => document.querySelector("#task-count").innerHTML === window.shown));
  const event = await page.evaluate(() => window.events.find(e => e.source === "push" && e.skippedRegions.includes("task-count")));
  assert.equal(event.skippedSnapshots[0].reason, "not-newer");
  assert.equal(event.feed, "tasks-live");
});

test("a dropped stream is diagnosed, reconnects, and replays what it missed", async t => {
  const page = await visit(t);
  let drop = true;
  // The next connection ends at once, as a proxy or a restart would.
  await page.route("**/live/tasks*", route => drop
    ? route.fulfill({ status: 200, contentType: "text/event-stream", body: "retry: 200\n\n" })
    : route.fallback());
  await page.evaluate(() => { const feed = document.getElementById("tasks-live"); feed.replaceWith(feed.cloneNode()); });
  await page.waitForFunction(() => window.events.some(e => e.code === "push-disconnected"));
  const warning = await page.evaluate(() => window.events.find(e => e.code === "push-disconnected"));
  assert.equal(warning.feed, "tasks-live");
  assert.match(warning.hint, /reconnects by itself/);
  // Published while this page is disconnected.
  const other = await visit(t);
  await edit(other, 1);
  await save(other, 1, "Saved during the outage");
  drop = false;
  await page.waitForFunction(() => window.events.some(e => e.type === "push" && e.phase === "push-reconnected"));
  await page.waitForFunction(() => document.querySelector('[data-task="1"] .task-title').textContent === "Saved during the outage");
});

test("a page whose position is gone resyncs by reading itself again", async t => {
  const page = await fixture.page(t);
  // Serve the page with a feed position from another server instance.
  await page.route(`${fixture.origin}/`, async route => {
    const response = await route.fetch();
    const body = (await response.text()).replace(/after=[0-9a-f]+-\d+/, "after=gone-1");
    await route.fulfill({ response, body });
  }, { times: 1 });
  await page.goto(fixture.origin);
  const other = await visit(t);
  await edit(other, 2);
  await save(other, 2, "Changed before the resync");
  await page.evaluate(() => {
    // Show an older title, as if the page were rendered before that save.
    document.querySelector('[data-task="2"] .task-title').textContent = "Stale";
    document.getElementById("task-summary:2").dataset.placeboRevision = "0";
  });
  const resyncs = () => page.evaluate(() => window.events.filter(e => e.type === "applied" && e.source === "resync").length);
  const before = await resyncs();
  // A new subscription from the same stale position.
  await page.locator("#tasks-live").evaluate(node => node.replaceWith(node.cloneNode()));
  await page.waitForFunction(before => window.events.filter(e => e.type === "applied" && e.source === "resync").length > before, before);
  assert.equal(await page.locator(`${row(2)} .task-title`).textContent(), "Changed before the resync");
  assert.ok(await page.evaluate(() => window.events.some(e => e.type === "push" && e.phase === "push-resync")));
});

test("a feed route that is missing is reported, and a push for an undeclared target is rejected", async t => {
  const page = await fixture.page(t);
  await page.route("**/live/tasks*", route => route.fulfill({ status: 404, body: "Not found" }));
  await page.goto(fixture.origin);
  await page.waitForFunction(() => window.events.some(e => e.code === "push-closed"));
  assert.match(await page.evaluate(() => window.events.find(e => e.code === "push-closed").hint), /FEED\.route\(\)/);
  await page.unroute("**/live/tasks*");
  await page.route("**/live/tasks*", route => route.fulfill({ status: 200, contentType: "text/event-stream",
    body: `event: update\nid: x-1\ndata: ${JSON.stringify({ version: 5, feed: "tasks-live", patches: [{ target: "add-task", operation: "replace-children", revision: "9", html: "" }] })}\nretry: 60000\n\n` }));
  await page.evaluate(() => { const feed = document.getElementById("tasks-live"); feed.replaceWith(feed.cloneNode()); });
  await page.waitForFunction(() => window.events.some(e => e.code === "undeclared-push"));
});

test("a slow reply does not bring back an item a push removed meanwhile", async t => {
  const first = await visit(t);
  const second = await visit(t);
  // Hold the adding tab's reply until the other tab has deleted the task.
  let release;
  const held = new Promise(resolve => { release = resolve; });
  await first.route("**/actions/add-task", async route => {
    const response = await route.fetch();
    await held;
    await route.fulfill({ response });
  }, { times: 1 });
  await first.locator("#add-task").click();
  await first.locator("#new-title").fill("Deleted before the reply");
  await first.locator("#new-title").press("Enter");
  const added = second.locator(".task-row", { hasText: "Deleted before the reply" });
  await added.waitFor();
  const id = await added.getAttribute("data-task");
  await first.locator(row(id)).waitFor();
  await second.locator(`${row(id)} [data-dialog-open]`).click();
  await second.locator(`${row(id)} button.danger`).click();
  await first.locator(row(id)).waitFor({ state: "detached" });
  release();
  await first.waitForFunction(() => window.events.some(e => e.type === "applied" && e.action === "add-task"));
  const event = await first.evaluate(() => window.events.find(e => e.type === "applied" && e.action === "add-task"));
  assert.deepEqual(event.supersededItems, [`tasks/${id}`]);
  assert.equal(await first.locator(row(id)).count(), 0);
});

test("a resync keeps an item pushed while it read the page", async t => {
  const page = await fixture.page(t);
  // The page's feed position is gone, so it resyncs on connecting. Its read
  // of the page is answered only after another tab adds a task. (Routes run
  // newest first: the first load gets the stale position.)
  let release;
  const held = new Promise(resolve => { release = resolve; });
  await page.route(`${fixture.origin}/`, async route => {
    const response = await route.fetch();
    await held;
    await route.fulfill({ response });
  }, { times: 1 });
  await page.route(`${fixture.origin}/`, async route => {
    const response = await route.fetch();
    const body = (await response.text()).replace(/after=[0-9a-f]+-\d+/, "after=gone-1");
    await route.fulfill({ response, body });
  }, { times: 1 });
  await page.goto(fixture.origin);
  await page.waitForFunction(() => window.events.some(e => e.type === "push" && e.phase === "push-resync"));
  const other = await visit(t);
  await other.locator("#add-task").click();
  await other.locator("#new-title").fill("Pushed during the resync");
  await other.locator("#new-title").press("Enter");
  const added = page.locator(".task-row", { hasText: "Pushed during the resync" });
  await added.waitFor();
  release();
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.source === "resync"));
  assert.equal(await added.count(), 1);
});
