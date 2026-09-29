import assert from "node:assert/strict";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

const fixture = serverFixture("uploads");

async function visit(t, options) {
  const page = await fixture.page(t, options);
  await page.goto(fixture.origin);
  return page;
}
const file = (name, size, mimeType = "image/png") => ({ name, mimeType, buffer: Buffer.alloc(size, 7) });
async function applied(page, outcome = "applied", count = 1) {
  await page.waitForFunction(({ outcome, count }) =>
    window.events.filter(e => e.type === "applied" && e.outcome === outcome).length === count, { outcome, count });
}

test("the runtime sends chosen files as multipart and the handler receives them", async t => {
  const page = await visit(t);
  assert.equal(await page.locator('[id="board:1"] form').getAttribute("enctype"), "multipart/form-data");
  await page.locator("#note").fill("With a cover and two files");
  await page.locator("#cover").setInputFiles(file("cover.png", 1000));
  await page.locator("#files").setInputFiles([file("a.txt", 10, "text/plain"), file("b.txt", 20, "text/plain")]);
  await page.locator("#note").press("Enter");
  await applied(page);
  assert.equal(await page.locator("#saved-cover").textContent(), "Cover: cover.png (1000 bytes, image/png)");
  assert.deepEqual(await page.locator("#saved-files li").allTextContents(), ["a.txt (10 bytes)", "b.txt (20 bytes)"]);
  // After a successful save, the submitted file inputs are empty again.
  assert.equal(await page.locator("#cover").evaluate(input => input.files.length), 0);
});

test("a rejected reply keeps the chosen file in the same input, so saving again sends it", async t => {
  const page = await visit(t);
  await page.locator("#note").fill("x");
  await page.locator("#cover").setInputFiles(file("kept.png", 500));
  await page.evaluate(() => { window.cover = document.querySelector("#cover"); });
  await page.locator("#note").press("Enter");
  await applied(page, "invalid");
  assert.match(await page.locator("#feedback").textContent(), /including kept\.png/);
  assert.deepEqual(await page.evaluate(() => ({ same: window.cover === document.querySelector("#cover"),
    files: Array.from(window.cover.files, f => f.name) })), { same: true, files: ["kept.png"] });
  await page.locator("#note").fill("Fixed the note");
  await page.locator("#note").press("Enter");
  await applied(page);
  assert.match(await page.locator("#saved-cover").textContent(), /kept\.png \(500 bytes/);
});

test("a file over its limit is refused in the browser without sending anything", async t => {
  const page = await visit(t);
  const requests = [];
  page.on("request", request => { if (request.method() === "POST") requests.push(request.url()); });
  await page.locator("#note").fill("Too big a cover");
  await page.locator("#cover").setInputFiles(file("huge.png", 70 * 1024));
  await page.locator("#note").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "ignored" && e.reason === "upload-too-large"));
  assert.equal(requests.length, 0);
  const ignored = await page.evaluate(() => window.events.find(e => e.reason === "upload-too-large"));
  assert.deepEqual([ignored.field, ignored.limit, ignored.size], ["cover", 65536, 71680]);
  assert.match(await page.locator("#cover").evaluate(input => input.validationMessage), /larger than 64 KB/);
  // Choosing a smaller file clears the message and saves.
  await page.locator("#cover").setInputFiles(file("small.png", 100));
  assert.equal(await page.locator("#cover").evaluate(input => input.validationMessage), "");
  await page.locator("#note").press("Enter");
  await applied(page);
  // Several files count together against the field's limit.
  await page.locator("#files").setInputFiles([file("one.bin", 200 * 1024), file("two.bin", 100 * 1024)]);
  await page.locator("#note").press("Enter");
  await page.waitForFunction(() => window.events.filter(e => e.reason === "upload-too-large").length === 2);
  assert.match(await page.locator("#files").evaluate(input => input.validationMessage), /These files are larger than 256 KB/);
});

test("a file the server refuses is diagnosed as too large, and nothing is marked stale", async t => {
  const page = await visit(t);
  await page.evaluate(() => document.querySelector("#cover").removeAttribute("data-placebo-max-bytes"));
  await page.locator("#note").fill("Bypassing the browser check");
  await page.locator("#cover").setInputFiles(file("huge.png", 70 * 1024));
  await page.locator("#note").press("Enter");
  await page.waitForFunction(() => window.events.some(e => e.type === "error" && e.code === "upload-too-large"));
  const detail = await page.evaluate(() => window.events.find(e => e.code === "upload-too-large"));
  assert.equal(detail.status, 413);
  assert.equal(detail.writeState, "not-started");
  assert.match(detail.message, /65536 bytes/);
  assert.equal(await page.locator('[id="board:1"]').getAttribute("data-placebo-stale"), null);
});

test("without JavaScript, files upload natively and an oversized one gets a readable page", async t => {
  const page = await visit(t, { javaScriptEnabled: false });
  await page.locator("#note").fill("Uploaded natively");
  await page.locator("#files").setInputFiles(file("native.txt", 42, "text/plain"));
  let navigation = page.waitForNavigation();
  await page.locator("#note").press("Enter");
  assert.equal((await navigation).status(), 200);
  assert.equal(await page.locator("#saved-note").textContent(), "Uploaded natively");
  assert.ok((await page.locator("#saved-files li").allTextContents()).includes("native.txt (42 bytes)"));
  // A rejection cannot keep a file natively, so the feedback names it.
  await page.locator("#note").fill("x");
  await page.locator("#cover").setInputFiles(file("again.png", 10));
  navigation = page.waitForNavigation();
  await page.locator("#note").press("Enter");
  assert.equal((await navigation).status(), 422);
  assert.match(await page.locator("#feedback").textContent(), /including again\.png/);
  assert.equal(await page.locator("#note").inputValue(), "x");
  await page.goto(fixture.origin);
  await page.locator("#note").fill("Too large natively");
  await page.locator("#cover").setInputFiles(file("huge.png", 70 * 1024));
  navigation = page.waitForNavigation();
  await page.locator("#note").press("Enter");
  assert.equal((await navigation).status(), 413);
  assert.match(await page.locator("p").textContent(), /huge\.png.*larger than 64 KB/);
});
