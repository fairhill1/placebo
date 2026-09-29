import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";
const fixture = serverFixture("search");

async function visit(t, { javaScriptEnabled = true, mockTransport = false } = {}) {
  const context = await fixture.browser.newContext({ javaScriptEnabled });
  t.after(() => context.close());
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  t.after(() => assert.deepEqual(errors, [], "no uncaught browser exceptions"));
  if (javaScriptEnabled) {
    await page.addInitScript(({ mockTransport }) => {
      window.events = [];
      for (const type of ["scheduled", "request", "applied", "discarded", "error"]) {
        document.addEventListener(`placebo:${type}`, ({ detail }) => window.events.push({ type, ...detail }));
      }
      if (mockTransport) {
        const nativeFetch = window.fetch;
        window.requests = [];
        window.fetch = (input, options) => {
          const url = new URL(input, location.href);
          if (!url.pathname.startsWith("/search/")) return nativeFetch(input, options);
          // Deliberately ignore AbortSignal. Correctness must not depend on
          // the network honoring cancellation.
          return new Promise(resolve => window.requests.push({
            q: url.searchParams.get("q"),
            path: url.pathname,
            resolve,
          }));
        };
        window.deliver = (q, overrides = {}, status = 200) => {
          const request = window.requests.find(r => r.q === q && !r.done);
          if (!request) throw new Error(`No pending mock request for ${q}`);
          request.done = true;
          const books = request.path.endsWith("books");
          request.resolve(new Response(JSON.stringify({
            version: 4,
            action: books ? "search-books" : "search-places",
            target: books ? "book-results" : "place-results",
            operation: "replace-children",
            html: `<p>${q}</p>`,
            ...overrides,
          }), { status, headers: { "Content-Type": "application/vnd.placebo.update+json",
            "X-Placebo-Action": books ? "search-books" : "search-places" } }));
        };
      }
    }, { mockTransport });
  }
  await page.goto(fixture.origin);
  if (javaScriptEnabled) await page.waitForFunction(() => document.querySelector('script[src="/placebo.js"]'));
  return page;
}

async function submit(page, q, scope = "books") {
  await page.locator(`#${scope}-query`).fill(q);
  await page.locator(`#${scope}-query`).press("Enter");
}

async function requested(page, q) {
  await page.waitForFunction(q => window.requests.some(r => r.q === q), q);
}

async function errorCode(page, code) {
  await page.waitForFunction(code => window.events.some(e => e.type === "error" && e.code === code), code);
}

test("server HTML works before JavaScript and native form navigation remains valid", async t => {
  const page = await visit(t, { javaScriptEnabled: false });
  assert.equal(await page.locator("#book-results li").count(), 4);
  await submit(page, "rust");
  await page.waitForURL("**/search/books?**");
  assert.equal(await page.locator("#book-results li").count(), 2);
  assert.equal(await page.locator("#books-query").inputValue(), "rust");
});

test("real fragments preserve the input node, focus, selection, and unrelated local state", async t => {
  const page = await visit(t);
  await page.locator("#draft").fill("Keep this draft.");
  await page.locator("#books-query").fill("rust");
  await page.evaluate(() => {
    window.originalInput = document.querySelector("#books-query");
    window.originalInput.setSelectionRange(1, 3);
    window.originalPlaces = document.querySelector("#place-results").innerHTML;
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "applied"));
  assert.equal(await page.locator("#book-results li").count(), 2);
  assert.equal(await page.locator("#draft").inputValue(), "Keep this draft.");
  assert.deepEqual(await page.evaluate(() => ({
    same: document.querySelector("#books-query") === window.originalInput,
    focused: document.activeElement === window.originalInput,
    selection: [window.originalInput.selectionStart, window.originalInput.selectionEnd],
    placesUnchanged: document.querySelector("#place-results").innerHTML === window.originalPlaces,
    errors: window.events.filter(e => e.type === "error"),
  })), { same: true, focused: true, selection: [1, 3], placesUnchanged: true, errors: [] });
});

test("an older response cannot overwrite a newer result even when abort is ignored", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "old");
  await requested(page, "old");
  await submit(page, "new");
  await requested(page, "new");
  await page.evaluate(() => window.deliver("new"));
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "new");
  await page.evaluate(() => window.deliver("old"));
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
  assert.equal(await page.locator("#book-results").textContent(), "new");
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "applied").length), 1);
});

test("new input invalidates the old response immediately, before debounce finishes", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "old");
  await requested(page, "old");
  await page.evaluate(() => {
    const input = document.querySelector("#books-query");
    input.value = "new";
    input.dispatchEvent(new InputEvent("input", { bubbles: true }));
    window.deliver("old");
  });
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
  assert.equal(await page.locator("#book-results li").count(), 4);
  await requested(page, "new");
  await page.evaluate(() => window.deliver("new"));
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "new");
});

test("independent mounted regions do not cancel each other", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "rust");
  await requested(page, "rust");
  await submit(page, "oslo", "places");
  await requested(page, "oslo");
  await page.evaluate(() => { window.deliver("oslo"); window.deliver("rust"); });
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied").length === 2);
  assert.equal(await page.locator("#book-results").textContent(), "rust");
  assert.equal(await page.locator("#place-results").textContent(), "oslo");
});

test("two forms aimed at one region share the same request ordering", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "first");
  await requested(page, "first");
  await page.evaluate(() => {
    const form = document.querySelector("#books-query").form.cloneNode(true);
    form.querySelectorAll("[id]").forEach(node => node.removeAttribute("id"));
    form.elements.q.value = "second";
    document.body.append(form);
    form.requestSubmit();
  });
  await requested(page, "second");
  await page.evaluate(() => { window.deliver("second"); window.deliver("first"); });
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "second");
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "applied").length), 1);
});

test("remounting an identical id cannot receive the previous instance's response", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "old-instance");
  await requested(page, "old-instance");
  await page.evaluate(() => {
    const target = document.querySelector("#book-results");
    const replacement = target.cloneNode(false);
    replacement.textContent = "new instance";
    target.replaceWith(replacement);
    window.deliver("old-instance");
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "discarded" && e.reason === "unmounted"));
  assert.equal(await page.locator("#book-results").textContent(), "new instance");
});

test("unmounting the source invalidates pending work and clears busy state", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "gone");
  await requested(page, "gone");
  await page.evaluate(() => document.querySelector("#books-query").form.remove());
  await page.waitForFunction(() => window.events.some(e => e.reason === "unmounted"));
  await page.evaluate(() => window.deliver("gone"));
  assert.equal(await page.locator("#book-results li").count(), 4);
  assert.equal(await page.locator("#book-results").getAttribute("aria-busy"), null);
});

for (const [name, change, code] of [
  ["missing", () => document.querySelector("#book-results").remove(), "missing-target"],
  ["duplicate", () => document.body.append(document.querySelector("#book-results").cloneNode(true)), "duplicate-target"],
]) {
  test(`${name} targets produce a diagnostic before a request is sent`, async t => {
    const page = await visit(t, { mockTransport: true });
    await page.evaluate(change);
    await submit(page, "rust");
    await errorCode(page, code);
    assert.equal(await page.evaluate(() => window.requests.length), 0);
    assert.equal(await page.locator("details.trace").getAttribute("open"), "");
  });
}

for (const [name, overrides, code] of [
  ["version", { version: 99 }, "version-mismatch"],
  ["action", { action: "another-action" }, "response-mismatch"],
  ["target", { target: "place-results" }, "response-mismatch"],
  ["operation", { operation: "unknown" }, "invalid-update"],
]) {
  test(`a mismatched response ${name} leaves existing DOM intact`, async t => {
    const page = await visit(t, { mockTransport: true });
    await submit(page, "bad");
    await requested(page, "bad");
    await page.evaluate(({ overrides }) => window.deliver("bad", overrides), { overrides });
    await errorCode(page, code);
    assert.equal(await page.locator("#book-results li").count(), 4);
    assert.equal(await page.locator("#book-results").getAttribute("aria-busy"), null);
  });
}

test("HTTP failures report an error and a subsequent request can recover", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "failed");
  await requested(page, "failed");
  await page.evaluate(() => window.deliver("failed", {}, 500));
  await errorCode(page, "http-error");
  assert.equal(await page.locator("#book-results li").count(), 4);
  await submit(page, "recovered");
  await requested(page, "recovered");
  await page.evaluate(() => window.deliver("recovered"));
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "recovered");
});

test("untrusted search text is displayed literally and cannot execute markup", async t => {
  const page = await visit(t);
  const attack = '<img src=x onerror="window.injected=true">';
  await submit(page, attack);
  await page.waitForFunction(() => window.events.some(e => e.type === "applied"));
  assert.equal(await page.locator("#book-results img").count(), 0);
  assert.ok((await page.locator("#book-results").textContent()).includes(attack));
  assert.equal(await page.evaluate(() => window.injected), undefined);
});

test("the demo fits a narrow screen without horizontal overflow", async t => {
  const page = await visit(t);
  if (process.env.PLACEBO_SCREENSHOTS) {
    await mkdir("test-results", { recursive: true });
    await page.screenshot({ path: "test-results/desktop.png", fullPage: true });
  }
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  if (process.env.PLACEBO_SCREENSHOTS) {
    await page.screenshot({ path: "test-results/mobile.png", fullPage: true });
  }
});

test("IME composition cancels old work and sends only committed text", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "old");
  await requested(page, "old");
  await page.evaluate(() => {
    const input = document.querySelector("#books-query");
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    input.value = "に";
    input.dispatchEvent(new InputEvent("input", { bubbles: true, isComposing: true }));
    window.deliver("old");
  });
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)));
  assert.equal(await page.locator("#book-results li").count(), 4);
  assert.equal(await page.evaluate(() => window.requests.length), 1);
  await page.evaluate(() => {
    const input = document.querySelector("#books-query");
    input.value = "日本";
    input.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
  });
  await requested(page, "日本");
  await page.evaluate(() => window.deliver("日本"));
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "日本");
});

test("the Books search follows history: one entry per query, Back and reload restore it", async t => {
  const page = await visit(t);
  const books = () => page.locator("#book-results li").count();
  const entries = await page.evaluate(() => history.length);
  await page.locator("#books-query").fill("rust");
  await page.waitForURL(/\?q=rust&/);
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 2);
  // Further keystrokes in the same field replace the entry the first one made.
  await page.locator("#books-query").pressSequentially(" in");
  await page.waitForURL(/\?q=rust\+in&/);
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 1);
  assert.equal(await page.evaluate(() => history.length), entries + 1);
  await page.goBack();
  await page.waitForFunction(() => document.querySelector("#books-query").value === "" &&
    document.querySelectorAll("#book-results li").length === 4);
  await page.goForward();
  await page.waitForFunction(() => document.querySelector("#books-query").value === "rust in" &&
    document.querySelectorAll("#book-results li").length === 1);
  // The page renders the same query, so a reload or bookmark shows it too.
  await page.reload();
  assert.equal(await page.locator("#books-query").inputValue(), "rust in");
  assert.equal(await books(), 1);
  // The Places search does not follow history.
  await page.locator("#places-query").fill("oslo");
  await page.waitForFunction(() => document.querySelectorAll("#place-results li").length === 1);
  assert.match(page.url(), /\?q=rust\+in&/);
});

// Runs last: it adds a book to the shared server state.
test("adding a book reruns the Books search with its current filter and keeps focus on the button", async t => {
  const page = await visit(t);
  await page.locator("#books-query").fill("design");
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 2);
  await page.locator("#new-book").fill("Design Patterns");
  // Submit from the keyboard: the button keeps focus while its node is replaced.
  await page.locator('[id="add-book:1"] button').focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 3);
  const applied = await page.evaluate(() => window.events.find(e => e.type === "applied" && e.target === "add-book:1"));
  assert.deepEqual(applied.refetched, ["book-results"]);
  assert.equal(await page.locator("#new-book").inputValue(), "");
  assert.equal(await page.locator("#add-feedback").textContent(), "Added.");
  assert.ok(await page.evaluate(() => document.activeElement.matches('[id="add-book:1"] button')));
});
