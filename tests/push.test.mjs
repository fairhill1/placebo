import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Two tabs on the task list follow the same feed. A change in one tells the
// other, which reads its page again and morphs it in.
const fixture = serverFixture("tasks");
const row = id => `[data-task="${id}"]`;
const order = page => page.$$eval("#tasks > .task-row", rows => rows.map(row => row.id));

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
async function pushed(page, predicate) {
  await page.waitForFunction(predicate => window.events.some(e => e.type === "applied" && e.source === "push" &&
    new Function("e", `return ${predicate}`)(e)), predicate);
}
async function edit(page, id) {
  await page.locator(`${row(id)} [data-dialog-open]`).click();
  await page.locator(`${row(id)} dialog`).waitFor({ state: "visible" });
}
async function save(page, id, title) {
  const before = await page.evaluate(id => window.events.filter(e => e.type === "applied" && e.target === `task:${id}`).length, id);
  await page.locator(`#title-${id}`).fill(title);
  await page.locator(`#title-${id}`).press("Enter");
  await page.waitForFunction(({ id, before }) =>
    window.events.filter(e => e.type === "applied" && e.target === `task:${id}`).length > before, { id, before });
}
async function move(page, id, direction) {
  const before = await page.evaluate(() => window.events.filter(e => e.type === "applied" && e.action === "move-task").length);
  await page.locator(`button[aria-label="Move ${direction} task ${id}"]`).click();
  await page.waitForFunction(before =>
    window.events.filter(e => e.type === "applied" && e.action === "move-task").length > before, before);
}

test("a save in one tab updates the row, the count, and the closed editor in the other", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await edit(first, 1);
  const done = await first.locator("#done-1").inputValue();
  await first.locator("#done-1").selectOption(done === "true" ? "false" : "true");
  await save(first, 1, "Saved in the first tab");
  await pushed(second, `e.refreshedComponents.includes("task:1")`);
  assert.equal(await second.locator(`${row(1)} .task-title`).textContent(), "Saved in the first tab");
  assert.equal(await second.locator("#task-count").innerHTML(), await first.locator("#task-count").innerHTML());
  // The second tab's editor has the new version, so its save is not a conflict.
  await edit(second, 1);
  assert.equal(await second.locator("#title-1").inputValue(), "Saved in the first tab");
  await save(second, 1, "Then saved in the second tab");
  assert.equal(await second.evaluate(() => window.events.findLast(e => e.type === "applied" && e.target === "task:1").outcome), "applied");
  await first.waitForFunction(() => document.querySelector('[data-task="1"] .task-title').textContent === "Then saved in the second tab");
});

test("the tab that saved skips its own signal and keeps its reply's feedback", async t => {
  const page = await visit(t);
  await edit(page, 2);
  await save(page, 2, "Saved without an echo");
  await page.waitForFunction(() => window.events.some(e => e.type === "push" && e.phase === "push-own"));
  // Give a wrongly started refresh time to land.
  await page.waitForTimeout(300);
  assert.match(await page.locator("#feedback-2").textContent(), /^Saved\./);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "applied" && e.source === "push").length), 0);
  assert.equal(await page.locator(`${row(2)} .task-title`).textContent(), "Saved without an echo");
});

test("a refresh keeps a draft open in the other tab's editor", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await edit(second, 2);
  await second.locator("#title-2").fill("A draft in the second tab");
  const doneBefore = await second.locator("#done-2").inputValue();
  await edit(first, 2);
  await first.locator("#done-2").selectOption(doneBefore === "true" ? "false" : "true");
  await save(first, 2, "Renamed in the first tab");
  await second.waitForFunction(() => document.querySelector('[data-task="2"] .task-title').textContent === "Renamed in the first tab");
  // Edited: kept. Untouched: the other tab's value. The dialog stays open.
  assert.equal(await second.locator("#title-2").inputValue(), "A draft in the second tab");
  assert.equal(await second.locator("#done-2").inputValue(), doneBefore === "true" ? "false" : "true");
  assert.ok(await second.locator(`${row(2)} dialog`).evaluate(dialog => dialog.open));
  const event = await second.evaluate(() => window.events.findLast(e => e.type === "applied" && e.source === "push"));
  assert.equal(event.feed, "tasks-live");
});

test("a draft kept through a refresh saves as a conflict, not over the other tab's change", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await edit(second, 2);
  await second.locator("#title-2").fill("A draft in the second tab");
  await edit(first, 2);
  await save(first, 2, "Renamed in the first tab");
  await second.waitForFunction(() => document.querySelector('[data-task="2"] .task-title').textContent === "Renamed in the first tab");
  // The editor kept the version its draft started from, so saving the draft
  // is checked against it.
  await save(second, 2, "A draft in the second tab");
  assert.equal(await second.evaluate(() => window.events.findLast(e => e.type === "applied" && e.target === "task:2").outcome), "conflict");
  assert.equal(await second.locator("#title-2").inputValue(), "A draft in the second tab");
  assert.equal(await first.locator(`${row(2)} .task-title`).textContent(), "Renamed in the first tab");
  // The conflict brought the current version, so saving again goes through.
  await save(second, 2, "A draft in the second tab");
  assert.equal(await second.evaluate(() => window.events.findLast(e => e.type === "applied" && e.target === "task:2").outcome), "applied");
  await first.waitForFunction(() => document.querySelector('[data-task="2"] .task-title').textContent === "A draft in the second tab");
});

test("adding, moving, and deleting in one tab change the other tab's list", async t => {
  const first = await visit(t);
  const second = await visit(t);
  await first.locator("#add-task").click();
  await first.locator("#new-title").fill("Shown in the other tab");
  await first.locator("#new-title").press("Enter");
  const added = second.locator(".task-row", { hasText: "Shown in the other tab" });
  await added.waitFor();
  const id = await added.getAttribute("data-task");
  await first.locator('[id="composer:new"]').waitFor({ state: "hidden" });
  assert.deepEqual(await order(second), await order(first));
  await move(first, id, "up");
  await second.waitForFunction(id => {
    const rows = Array.from(document.querySelectorAll("#tasks > .task-row"), row => row.id);
    return rows.indexOf(`tasks/${id}`) === rows.length - 2;
  }, id);
  await first.locator(`${row(id)} [data-dialog-open]`).click();
  await first.locator(`${row(id)} button.danger`).click();
  await second.locator(row(id)).waitFor({ state: "detached" });
  assert.equal(await first.locator(row(id)).count(), 0);
  assert.equal(await second.locator("#task-count").innerHTML(), await first.locator("#task-count").innerHTML());
});

test("a page whose feed position is from another server process reads itself when it connects", async t => {
  const other = await visit(t);
  await edit(other, 2);
  await save(other, 2, "Changed before connecting");
  const page = await fixture.page(t);
  // Serve the page as another server process rendered it, before that save.
  let served;
  await page.route(`${fixture.origin}/`, async route => {
    const response = await route.fetch();
    served = (await response.text()).replace(/after=[0-9a-f]+-\d+/, "after=gone-1")
      .replace(">Changed before connecting<", ">Stale<");
    await route.fulfill({ response, body: served });
  }, { times: 1 });
  await page.goto(fixture.origin);
  assert.ok(served.includes(">Stale<") && served.includes("after=gone-1"));
  await pushed(page, "true");
  assert.equal(await page.locator(`${row(2)} .task-title`).textContent(), "Changed before connecting");
});

test("a dropped stream is diagnosed, reconnects, and shows a change made meanwhile", async t => {
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
  // Changed while this page is disconnected.
  const other = await visit(t);
  await edit(other, 1);
  await save(other, 1, "Saved during the outage");
  drop = false;
  await page.waitForFunction(() => window.events.some(e => e.type === "push" && e.phase === "push-reconnected"));
  await page.waitForFunction(() => document.querySelector('[data-task="1"] .task-title').textContent === "Saved during the outage");
});

test("a missing feed route and a signal from another protocol version are reported", async t => {
  const page = await fixture.page(t);
  await page.route("**/live/tasks*", route => route.fulfill({ status: 404, body: "Not found" }));
  await page.goto(fixture.origin);
  await page.waitForFunction(() => window.events.some(e => e.code === "push-closed"));
  assert.match(await page.evaluate(() => window.events.find(e => e.code === "push-closed").hint), /FEED\.route\(\)/);
  await page.unroute("**/live/tasks*");
  await page.route("**/live/tasks*", route => route.fulfill({ status: 200, contentType: "text/event-stream",
    body: `event: changed\nid: x-1\ndata: ${JSON.stringify({ version: 5, feed: "tasks-live" })}\nretry: 60000\n\n` }));
  await page.evaluate(() => { const feed = document.getElementById("tasks-live"); feed.replaceWith(feed.cloneNode()); });
  await page.waitForFunction(() => window.events.some(e => e.code === "version-mismatch" && e.feed === "tasks-live"));
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "applied" && e.source === "push").length), 0);
});

test("signals that arrive while the page is being read cause one more read, not one each", async t => {
  const page = await visit(t);
  let reads = 0, release;
  const held = new Promise(resolve => { release = resolve; });
  // Hold the first read of the page until every signal has arrived.
  await page.route(`${fixture.origin}/`, async route => {
    if (!route.request().headers()["x-placebo-refresh"]) return route.fallback();
    reads += 1;
    if (reads === 1) await held;
    await route.continue();
  });
  const other = await visit(t);
  const signals = () => page.evaluate(() => window.events.filter(e => e.type === "push" && e.phase === "push-changed").length);
  const before = await signals();
  // Four changes that leave the order as it was.
  for (const direction of ["down", "up", "down", "up"]) await move(other, 1, direction);
  await page.waitForFunction(count => window.events.filter(e => e.type === "push" && e.phase === "push-changed").length >= count, before + 4);
  release();
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied" && e.source === "push").length >= 2);
  await page.waitForTimeout(200);
  assert.equal(reads, 2);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "applied" && e.source === "push").length), 2);
});

test("a refresh read before the page's own newer reply is discarded", async t => {
  const page = await visit(t);
  let reads = 0, release, fetched;
  const held = new Promise(resolve => { release = resolve; });
  const readEarly = new Promise(resolve => { fetched = resolve; });
  // The first read of the page is rendered now, and delivered only after
  // this page has shown its own, newer reply.
  await page.route(`${fixture.origin}/`, async route => {
    if (!route.request().headers()["x-placebo-refresh"]) return route.fallback();
    reads += 1;
    if (reads > 1) return route.continue();
    const response = await route.fetch();
    fetched();
    await held;
    await route.fulfill({ response });
  });
  const other = await visit(t);
  // The other tab moves task 2 down, and this page, not yet refreshed, moves
  // it back up: the older read shows the order in between.
  await move(other, 2, "down");
  await readEarly;
  await move(page, 2, "up");
  release();
  await page.waitForFunction(() => window.events.some(e => e.type === "discarded" && e.reason === "older-page"));
  const discarded = await page.evaluate(() => window.events.find(e => e.type === "discarded" && e.reason === "older-page"));
  assert.equal(discarded.feed, "tasks-live");
  // The signal from this page's own move needs no read: its reply's page shows it.
  await page.waitForFunction(() => window.events.some(e => e.type === "push" && e.phase === "push-own"));
  const rows = await order(page);
  assert.ok(rows.indexOf("tasks/2") < rows.indexOf("tasks/3"), rows.join(", "));
});
