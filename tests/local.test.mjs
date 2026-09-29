import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// Local UI state with native features: dialogs opened by command/commandfor,
// a popover, and a details disclosure.
const fixture = serverFixture("tasks");
const row = id => `[data-task="${id}"]`;

async function visit(t, options = {}) {
  const page = await fixture.page(t, { feeds: false, ...options });
  await page.goto(fixture.origin);
  return page;
}

test("the editor dialog opens and closes without JavaScript, and a rejected save opens it again", async t => {
  const page = await visit(t, { javaScriptEnabled: false });
  await page.locator(`${row(3)} [data-dialog-open]`).click();
  const dialog = page.locator('[id="task:3"]');
  assert.ok(await dialog.evaluate(node => node.open && node.matches(":modal")));
  await page.locator(`${row(3)} [data-dialog-close]`).first().click();
  assert.equal(await dialog.evaluate(node => node.open), false);
  await page.locator(`${row(3)} [data-dialog-open]`).click();
  await page.locator("#title-3").fill("x");
  const navigation = page.waitForNavigation();
  await page.locator("#title-3").press("Enter");
  assert.equal((await navigation).status(), 422);
  // The whole page again, with the editor open and the draft and feedback in it.
  assert.ok(await dialog.evaluate(node => node.open));
  assert.equal(await page.locator("#title-3").inputValue(), "x");
  assert.match(await page.locator("#feedback-3").textContent(), /3 and 80/);
});

test("a reply keeps a closed details and an open popover as the person left them", async t => {
  const page = await visit(t);
  await page.locator(`${row(1)} [data-dialog-open]`).click();
  await page.locator('[id="advanced-1"] > summary').click();
  assert.equal(await page.locator('[id="advanced-1"]').evaluate(node => node.open), false);
  await page.locator(`${row(1)} .help`).click();
  assert.ok(await page.locator('[id="title-help-1"]').evaluate(node => node.matches(":popover-open")));
  await page.evaluate(() => {
    document.querySelector("#title-1").value = "x";
    document.querySelector("#title-1").form.requestSubmit();
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.outcome === "invalid"));
  assert.equal(await page.locator('[id="advanced-1"]').evaluate(node => node.open), false);
  assert.ok(await page.locator('[id="title-help-1"]').evaluate(node => node.matches(":popover-open")));
});

test("a button whose command target is missing or the wrong kind is reported", async t => {
  const page = await visit(t);
  const logs = [];
  page.on("console", message => { if (message.type() === "error") logs.push(message.text()); });
  await page.evaluate(() => {
    document.body.insertAdjacentHTML("beforeend",
      '<button id="broken" command="show-modal" commandfor="task:999">Open</button>' +
      '<div id="plain">Not a dialog</div><button id="wrong" command="show-modal" commandfor="plain">Open</button>' +
      '<button id="typo" popovertarget="nowhere">Help</button>');
  });
  await page.waitForFunction(() => window.events.filter(e => ["missing-command-target", "invalid-command"].includes(e.code)).length === 3);
  const codes = await page.evaluate(() => window.events.filter(e => e.type === "error").map(e => [e.element, e.code]));
  assert.deepEqual(codes.sort(), [["button#broken", "missing-command-target"], ["button#typo", "missing-command-target"],
    ["button#wrong", "invalid-command"]]);
  assert.ok(logs.some(line => line.includes("[placebo:missing-command-target]") && line.includes('commandfor="task:999"')));
  // Each is reported once, and the page's own buttons resolve.
  await page.evaluate(() => document.body.append(document.createElement("p")));
  await page.waitForTimeout(50);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 3);
});
