import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

const fixture = serverFixture("controls");

async function visit(t) {
  const page = await fixture.page(t);
  await page.goto(fixture.origin);
  return page;
}
async function applied(page, outcome) {
  await page.waitForFunction(outcome => window.events.some(e =>
    e.type === "applied" && e.target === "profile:1" && e.outcome === outcome), outcome);
}
async function editEverything(page, name) {
  await page.locator("#name").fill(name);
  await page.locator("#nickname").fill("");
  await page.locator("#bio").fill("\nIndented\nsecond line");
  await page.locator("#age").fill("");
  await page.locator("#height").fill("1.72");
  await page.locator("#birthday").fill("1815-12-10");
  await page.locator("#newsletter").uncheck();
  await page.locator("#plan").selectOption("pro");
  await page.getByLabel("Editor").check();
  await page.locator("#topics").selectOption(["web", "ops"]);
  await page.getByLabel("Friday").check();
  await page.getByLabel("Saturday").uncheck();
}
const controlState = page => page.evaluate(() => ({
  name: document.querySelector("#name").value,
  bio: document.querySelector("#bio").value,
  newsletter: document.querySelector("#newsletter").checked,
  role: document.querySelector("[name=role]:checked")?.value ?? null,
  topics: Array.from(document.querySelector("#topics").selectedOptions, o => o.value),
  days: Array.from(document.querySelectorAll("[name=days]:checked"), input => input.value),
}));

test("the runtime submits every control type and the adapter decodes it", async t => {
  const page = await visit(t);
  assert.equal(await page.locator("#bio").inputValue(), "Writes programs.");
  await editEverything(page, "Ada Lovelace");
  await page.getByRole("button", { name: "Save profile" }).click();
  await applied(page, "applied");
  assert.equal(await page.locator("#saved").textContent(), 'SaveProfile { name: "Ada Lovelace", ' +
    'email: "ada@example.com", nickname: None, bio: "\\nIndented\\nsecond line", age: None, height_m: 1.72, ' +
    'newsletter: false, plan: "pro", role: Some(2), topics: ["web", "ops"], days: [5], birthday: Some("1815-12-10") }');
  // The reset draft renders the saved values, including the textarea's leading newline.
  assert.deepEqual(await controlState(page), { name: "Ada Lovelace", bio: "\nIndented\nsecond line",
    newsletter: false, role: "2", topics: ["web", "ops"], days: ["5"] });
});

test("validation keeps checked, selected and multiline drafts in the same nodes", async t => {
  const page = await visit(t);
  const saved = await page.locator("#saved").textContent();
  await editEverything(page, "   ");
  await page.evaluate(() => { window.originalDays = document.querySelector("#days"); });
  await page.getByRole("button", { name: "Save profile" }).click();
  await applied(page, "invalid");
  assert.equal(await page.locator("#feedback").textContent(), "Enter a name.");
  assert.equal(await page.locator("#saved").textContent(), saved);
  assert.equal(await page.evaluate(() => document.querySelector("#days") === window.originalDays), true);
  assert.deepEqual(await controlState(page), { name: "   ", bio: "\nIndented\nsecond line",
    newsletter: false, role: "2", topics: ["web", "ops"], days: ["5"] });
});
