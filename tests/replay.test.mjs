import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Every mutation form carries an idempotency key. A retry after a lost
// response resends the same request, and the server replays its reply.
const fixture = serverFixture("tasks");
const row = id => `[data-task="${id}"]`;

async function visit(t) {
  const page = await fixture.page(t, { feeds: false });
  await page.goto(fixture.origin);
  return page;
}
async function errorCode(page, code) {
  await page.waitForFunction(code => window.events.some(e => e.type === "error" && e.code === code), code);
}
async function addDialog(page, title) {
  await page.locator("#add-task").click();
  await page.locator("#new-title").fill(title);
}

test("a lost response marks the form stale, and adding again replays instead of adding twice", async t => {
  const page = await visit(t);
  const rows = await page.locator(".task-row").count();
  let lose = true;
  const bodies = [];
  const retries = [];
  await page.route("**/actions/add-task", async route => {
    bodies.push(route.request().postData());
    retries.push(route.request().headers()["x-placebo-retry"]);
    const response = await route.fetch();
    if (lose) { lose = false; await route.abort("failed"); } else await route.fulfill({ response });
  });
  await addDialog(page, "Added exactly once");
  await page.locator("#new-title").press("Enter");
  await errorCode(page, "network-error");
  const composer = page.locator('[id="composer:new"]');
  assert.equal(await composer.getAttribute("data-placebo-stale"), "");
  assert.ok(await page.locator('[id="composer:new"] .stale-note').isVisible());
  // Typing more while stale does not change what the retry sends.
  await page.locator("#new-title").fill("Added exactly once, edited");
  await page.locator("#new-title").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.target === "composer:new"));
  assert.equal(bodies.length, 2);
  assert.equal(bodies[1], bodies[0], "the retry resends the first attempt unchanged");
  assert.equal(retries[0], undefined);
  assert.match(retries[1], /^\d+$/, "the retry says how old its first attempt is");
  const events = await page.evaluate(() => window.events);
  const retry = events.find(e => e.type === "scheduled" && e.retry);
  assert.equal(retry.retryOf, events.find(e => e.type === "error").requestId);
  assert.equal(events.find(e => e.type === "applied").replayed, true);
  assert.equal(await composer.getAttribute("data-placebo-stale"), null);
  // The committed task appears once, and the newer draft is still there.
  assert.equal(await page.locator(".task-title", { hasText: /^Added exactly once$/ }).count(), 1);
  assert.equal(await page.locator("#new-title").inputValue(), "Added exactly once, edited");
  await page.reload();
  assert.equal(await page.locator(".task-row").count(), rows + 1);
  assert.equal(await page.locator(".task-title", { hasText: /^Added exactly once$/ }).count(), 1);
});

test("a retry whose first attempt never reached the server runs it once", async t => {
  const page = await visit(t);
  const rows = await page.locator(".task-row").count();
  let lose = true;
  await page.route("**/actions/add-task", async route => {
    if (lose) { lose = false; return route.abort("failed"); }
    return route.continue();
  });
  await addDialog(page, "Sent on the second try");
  await page.locator("#new-title").press("Enter");
  await errorCode(page, "network-error");
  await page.locator("#new-title").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "applied"));
  assert.equal(await page.evaluate(() => window.events.find(e => e.type === "applied").replayed), false);
  await page.reload();
  assert.equal(await page.locator(".task-row").count(), rows + 1);
});

test("a save retried after its response was lost keeps edits typed in between", async t => {
  const page = await visit(t);
  let lose = true;
  await page.route("**/actions/save-task", async route => {
    const response = await route.fetch();
    if (lose) { lose = false; await route.abort("failed"); } else await route.fulfill({ response });
  });
  await page.locator(`${row(2)} [data-dialog-open]`).click();
  await page.locator("#title-2").fill("The first attempt");
  await page.locator("#title-2").press("Enter");
  await errorCode(page, "network-error");
  await page.locator("#title-2").fill("Typed while it was uncertain");
  await page.locator("#title-2").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.target === "task:2"));
  const applied = await page.evaluate(() => window.events.find(e => e.type === "applied"));
  assert.equal(applied.replayed, true);
  assert.deepEqual(applied.preservedLocal, [{ key: "/actions/save-task#title", reason: "edited-since-submission" }]);
  assert.equal(await page.locator(`${row(2)} .task-title`).textContent(), "The first attempt");
  assert.equal(await page.locator("#title-2").inputValue(), "Typed while it was uncertain");
  // Saving again is a new submission with the reply's fresh key.
  await page.locator("#title-2").press("Enter");
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied" && e.target === "task:2").length === 2);
  assert.equal(await page.locator(`${row(2)} .task-title`).textContent(), "Typed while it was uncertain");
});

test("an attempt the server has not finished is diagnosed and stays stale", async t => {
  const page = await visit(t);
  await page.route("**/actions/add-task", route => route.fulfill({
    status: 409, contentType: "text/html",
    headers: { "x-placebo-replay": "pending", "x-placebo-action": "add-task" }, body: "Still saving",
  }));
  await addDialog(page, "Still running elsewhere");
  await page.locator("#new-title").press("Enter");
  await errorCode(page, "replay-pending");
  const detail = await page.evaluate(() => window.events.find(e => e.code === "replay-pending"));
  assert.equal(detail.writeState, "unknown");
  assert.match(detail.hint, /submit again/);
  assert.equal(await page.locator('[id="composer:new"]').getAttribute("data-placebo-stale"), "");
});

test("each new submission sends its own key, not the one in the markup", async t => {
  // Markup rendered once for several pages, such as a pushed row, carries the
  // same key everywhere. Sending it would replay one page's reply to another.
  const page = await visit(t);
  const bodies = [];
  await page.route("**/actions/add-task", route => { bodies.push(route.request().postData()); return route.continue(); });
  for (const [index, title] of ["First of two", "Second of two"].entries()) {
    await addDialog(page, title);
    const rendered = await page.locator('[id="composer:new"] input[name="placebo-key"]').inputValue();
    await page.locator("#new-title").press("Enter");
    await page.waitForFunction(count => window.events.filter(e => e.type === "applied" && e.target === "composer:new").length === count, index + 1);
    const sent = new URLSearchParams(bodies.at(-1)).get("placebo-key");
    assert.match(sent, /^[0-9a-f]{32}$/);
    assert.notEqual(sent, rendered);
  }
  assert.notEqual(new URLSearchParams(bodies[0]).get("placebo-key"), new URLSearchParams(bodies[1]).get("placebo-key"));
});

test("a retry older than the server remembers is diagnosed and stays stale", async t => {
  const page = await visit(t);
  await page.route("**/actions/add-task", route => route.fulfill({
    status: 409, contentType: "text/html",
    headers: { "x-placebo-replay": "unknown", "x-placebo-action": "add-task" }, body: "Your changes may already be saved",
  }));
  await addDialog(page, "Sent long ago");
  await page.locator("#new-title").press("Enter");
  await errorCode(page, "replay-unknown");
  const detail = await page.evaluate(() => window.events.find(e => e.code === "replay-unknown"));
  assert.equal(detail.writeState, "unknown");
  assert.match(detail.hint, /Reload/);
  assert.equal(await page.locator('[id="composer:new"]').getAttribute("data-placebo-stale"), "");
});
