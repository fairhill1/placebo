import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { test } from "node:test";
import { serverFixture } from "./fixture.mjs";

// A read form reads the page it is on at the form's query, and the page
// morphs in. The search example: one Books search, and a component that adds
// books.
const fixture = serverFixture("search");

async function visit(t, { path = "/", mockTransport = false, ...options } = {}) {
  const page = await fixture.page(t, options);
  if (mockTransport) {
    await page.addInitScript(() => {
      // The page as served, for the mock's replies.
      document.addEventListener("DOMContentLoaded", () => { window.served = document.documentElement.outerHTML; });
      const nativeFetch = window.fetch;
      window.requests = [];
      window.fetch = (input, options = {}) => {
        if (!new Headers(options.headers).has("X-Placebo-Refresh")) return nativeFetch(input, options);
        const url = new URL(input, location.href);
        // Deliberately ignore AbortSignal. Correctness must not depend on
        // the network honoring cancellation.
        return new Promise(resolve => window.requests.push({ q: url.searchParams.get("q"), resolve }));
      };
      // Answer a pending read with the page at its query, whose results show
      // the query itself.
      window.deliver = (q, { status = 200, type = "text/html", body = null } = {}) => {
        // The latest one: typing may have sent the same query before Enter did.
        const request = window.requests.findLast(r => r.q === q && !r.done);
        if (!request) throw new Error(`No pending mock request for ${q}`);
        request.done = true;
        const page = new DOMParser().parseFromString(window.served, "text/html");
        page.getElementById("books-query").setAttribute("value", q);
        page.getElementById("book-results").innerHTML = `<p>${q}</p>`;
        request.resolve(new Response(body ?? `<!DOCTYPE html>${page.documentElement.outerHTML}`,
          { status, headers: { "Content-Type": type } }));
      };
    });
  }
  await page.goto(fixture.origin + path);
  return page;
}

async function submit(page, q) {
  await page.locator("#books-query").fill(q);
  await page.locator("#books-query").press("Enter");
}

async function requested(page, q) {
  await page.waitForFunction(q => window.requests.some(r => r.q === q), q);
}

async function errorCode(page, code) {
  await page.waitForFunction(code => window.events.some(e => e.type === "error" && e.code === code), code);
}

const settle = page => page.evaluate(() => new Promise(resolve => setTimeout(resolve, 50)));
const applied = page => page.evaluate(() => window.events.filter(e => e.type === "applied").length);

test("server HTML works before JavaScript and native form navigation remains valid", async t => {
  const page = await visit(t, { javaScriptEnabled: false });
  assert.equal(await page.locator("#book-results li").count(), 4);
  await submit(page, "rust");
  // A read form has no action: the browser loads this page at its query.
  await page.waitForURL(/\/\?q=rust&delay_ms=0$/);
  assert.equal(await page.locator("#book-results li").count(), 2);
  assert.equal(await page.locator("#books-query").inputValue(), "rust");
  // The address is the query, so reload and Back work without JavaScript too.
  await page.reload();
  assert.equal(await page.locator("#book-results li").count(), 2);
  await submit(page, "philosophy");
  await page.waitForURL(/q=philosophy/);
  assert.equal(await page.locator("#book-results li").count(), 1);
  await page.goBack();
  await page.waitForURL(/q=rust/);
  assert.equal(await page.locator("#books-query").inputValue(), "rust");
  assert.equal(await page.locator("#book-results li").count(), 2);
});

test("a search morphs the page, keeping the input node, focus, selection, and an unrelated draft", async t => {
  const page = await visit(t);
  await page.locator("#draft").fill("Keep this draft.");
  await page.locator("#books-query").fill("rust");
  await page.evaluate(() => {
    window.originalInput = document.querySelector("#books-query");
    window.originalInput.setSelectionRange(1, 3);
    window.originalResults = document.querySelector("#book-results");
    window.originalAdd = document.getElementById("add-book:1");
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "applied"));
  assert.equal(await page.locator("#book-results li").count(), 2);
  assert.equal(await page.locator("#draft").inputValue(), "Keep this draft.");
  assert.deepEqual(await page.evaluate(() => ({
    same: document.querySelector("#books-query") === window.originalInput,
    focused: document.activeElement === window.originalInput,
    selection: [window.originalInput.selectionStart, window.originalInput.selectionEnd],
    // The search box now shows what the page rendered for its query.
    rendered: window.originalInput.defaultValue,
    results: document.querySelector("#book-results") === window.originalResults,
    component: document.getElementById("add-book:1") === window.originalAdd,
    errors: window.events.filter(e => e.type === "error"),
  })), { same: true, focused: true, selection: [1, 3], rendered: "rust", results: true, component: true, errors: [] });
  const [scheduled, request] = await page.evaluate(() => ["scheduled", "request"].map(type => window.events.find(e => e.type === type)));
  assert.equal(scheduled.method, "GET");
  assert.equal(request.path, "/");
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
  await settle(page);
  assert.equal(await page.locator("#book-results").textContent(), "new");
  assert.equal(await applied(page), 1);
  assert.ok(await page.evaluate(() => window.events.some(e => e.type === "discarded" && e.reason === "superseded")));
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
  await settle(page);
  assert.equal(await page.locator("#book-results li").count(), 4);
  await requested(page, "new");
  await page.evaluate(() => window.deliver("new"));
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "new");
});

test("two read forms can show a field of the same name", async t => {
  const page = await visit(t);
  // A second search box for the same query, such as one in a header.
  await page.evaluate(() => {
    const form = document.querySelector("#books-query").form.cloneNode(true);
    form.querySelectorAll("[id]").forEach(node => node.removeAttribute("id"));
    document.querySelector("main").prepend(form);
  });
  await page.locator("#books-query").fill("rust");
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 2);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 0);
  assert.equal(await page.locator("#books-query").inputValue(), "rust");
});

test("two read forms share one latest-wins ordering", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "first");
  await requested(page, "first");
  // A second form reading the same page, such as a "load more" form.
  await page.evaluate(() => {
    const form = document.createElement("form");
    form.method = "get";
    form.dataset.placebo = JSON.stringify({ version: 6, read: true });
    form.innerHTML = '<input type="hidden" name="q" value="second"><input type="hidden" name="delay_ms" value="0">';
    document.body.append(form);
    form.requestSubmit();
  });
  await requested(page, "second");
  await page.evaluate(() => { window.deliver("second"); window.deliver("first"); });
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "second");
  await settle(page);
  assert.equal(await page.locator("#book-results").textContent(), "second");
  assert.equal(await applied(page), 1);
  assert.match(page.url(), /\?q=second&delay_ms=0$/);
});

test("unmounting the form cancels its read and clears busy state", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "gone");
  await requested(page, "gone");
  await page.evaluate(() => {
    window.form = document.querySelector("#books-query").form;
    window.busy = window.form.getAttribute("aria-busy");
    window.form.remove();
  });
  await page.waitForFunction(() => window.events.some(e => e.type === "discarded" && e.reason === "unmounted"));
  await page.evaluate(() => window.deliver("gone"));
  await settle(page);
  assert.equal(await page.locator("#book-results li").count(), 4);
  assert.deepEqual(await page.evaluate(() => [window.busy, window.form.getAttribute("aria-busy")]), ["true", null]);
  assert.equal(await applied(page), 0);
});

test("a read form given an action is refused before sending", async t => {
  const page = await visit(t, { mockTransport: true });
  await page.evaluate(() => document.querySelector("#books-query").form.setAttribute("action", "/elsewhere"));
  await submit(page, "rust");
  await errorCode(page, "unsupported-method");
  assert.equal(await page.evaluate(() => window.requests.length), 0);
  assert.equal(new URL(page.url()).search, "");
  assert.equal(await page.locator("details.trace").getAttribute("open"), "");
});

test("HTTP failures report an error and a later search recovers", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "failed");
  await requested(page, "failed");
  await page.evaluate(() => window.deliver("failed", { status: 500 }));
  await errorCode(page, "http-error");
  assert.equal(await page.locator("#book-results li").count(), 4);
  assert.equal(await page.evaluate(() => document.querySelector("#books-query").form.getAttribute("aria-busy")), null);
  await submit(page, "recovered");
  await requested(page, "recovered");
  await page.evaluate(() => window.deliver("recovered"));
  await page.waitForFunction(() => document.querySelector("#book-results").textContent === "recovered");
});

test("a reply that is not HTML is reported and changes nothing", async t => {
  const page = await visit(t, { mockTransport: true });
  await submit(page, "json");
  await requested(page, "json");
  await page.evaluate(() => window.deliver("json", { type: "application/json", body: '{"html":"<p>json</p>"}' }));
  await errorCode(page, "invalid-content-type");
  assert.equal(await page.locator("#book-results li").count(), 4);
  assert.equal(await applied(page), 0);
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
  await settle(page);
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

test("the address follows the query by replacing its entry, and reload restores it", async t => {
  const page = await visit(t);
  const entries = await page.evaluate(() => history.length);
  await page.locator("#books-query").fill("rust");
  await page.waitForURL(/\?q=rust&delay_ms=0$/);
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 2);
  await page.locator("#books-query").pressSequentially(" in");
  await page.waitForURL(/\?q=rust\+in&delay_ms=0$/);
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 1);
  // Other controls of the form are part of the query too.
  await page.locator("#books-delay").selectOption("800");
  await page.waitForURL(/\?q=rust\+in&delay_ms=800$/);
  await page.waitForFunction(() => window.events.filter(e => e.type === "applied").length >= 3);
  assert.equal(await page.evaluate(() => history.length), entries);
  // The page renders the same query, so a reload or bookmark shows it too.
  await page.reload();
  assert.equal(await page.locator("#books-query").inputValue(), "rust in");
  assert.equal(await page.locator("#books-delay").inputValue(), "800");
  assert.equal(await page.locator("#book-results li").count(), 1);
});

// The tests from here on add books to the shared server state.
test("adding a book after a search shows it in that search's results and keeps focus on the button", async t => {
  const page = await visit(t);
  await page.locator("#books-query").fill("design");
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 2);
  const reads = await page.evaluate(() => window.events.filter(e => e.type === "request" && e.method === "GET").length);
  await page.locator("#new-book").fill("Design Patterns");
  // Submit from the keyboard: the button keeps focus while the page morphs.
  await page.locator('[id="add-book:1"] button').focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.querySelectorAll("#book-results li").length === 3);
  const reply = await page.evaluate(() => window.events.find(e => e.type === "applied" && e.target === "add-book:1"));
  assert.equal(reply.page, "whole");
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "request" && e.method === "GET").length), reads);
  assert.equal(await page.locator("#new-book").inputValue(), "");
  assert.equal(await page.locator("#add-feedback").textContent(), "Added.");
  assert.equal(await page.locator("#books-query").inputValue(), "design");
  assert.ok(await page.evaluate(() => document.activeElement.matches('[id="add-book:1"] button')));
});

test("adding a book on a filtered page shows it from the reply's page, without reading again", async t => {
  const page = await visit(t, { path: "/?q=design" });
  const results = await page.evaluate(() => (window.results = document.querySelector("#book-results"), window.results.querySelectorAll("li").length));
  await page.locator("#new-book").fill("Designing for the web");
  await page.locator("#new-book").press("Enter");
  await page.waitForFunction(count => document.querySelectorAll("#book-results li").length === count + 1, results);
  const reply = await page.evaluate(() => window.events.find(e => e.type === "applied" && e.target === "add-book:1"));
  assert.equal(reply.page, "whole");
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "request" && e.method === "GET").length), 0);
  assert.ok(await page.evaluate(() => document.querySelector("#book-results") === window.results));
  assert.ok((await page.locator("#book-results").textContent()).includes("Designing for the web"));
});

// The save reaches the server and writes, then the search moves the page to
// another query while the save's page, for the old address, is on its way.
async function saveThenSearch(page, title, q) {
  let written, release;
  const write = new Promise(resolve => { written = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  await page.route("**/actions/add-book", async route => {
    const response = await route.fetch();
    written();
    await gate;
    await route.fulfill({ response });
  });
  await page.evaluate(() => document.addEventListener("placebo:applied", ({ detail }) => {
    if (detail.target === "add-book:1") window.feedback = document.getElementById("add-feedback").textContent;
  }));
  await page.locator("#new-book").fill(title);
  await page.locator("#new-book").press("Enter");
  await write;
  await page.locator("#books-query").fill(q);
  await page.waitForURL(new RegExp(`\\?q=${q}&delay_ms=0$`));
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.method === "GET"));
  assert.deepEqual(await page.locator("#book-results li span:first-child").allTextContents(), [title]);
  release();
  await page.waitForFunction(() => window.events.some(e => e.type === "applied" && e.source === "moved"));
}

test("a save answered after a search shows in its component only, then the page is read again at the new query", async t => {
  const page = await visit(t);
  await saveThenSearch(page, "Moved along", "moved");
  const reply = await page.evaluate(() => window.events.find(e => e.type === "applied" && e.target === "add-book:1"));
  assert.equal(reply.page, "moved");
  assert.equal(reply.outcome, "applied");
  // The component showed its reply; the rest shows the new query's page.
  assert.equal(await page.evaluate(() => window.feedback), "Added.");
  assert.equal(await page.locator("#new-book").inputValue(), "");
  assert.deepEqual(await page.locator("#book-results li span:first-child").allTextContents(), ["Moved along"]);
  assert.match(page.url(), /\?q=moved&delay_ms=0$/);
  assert.equal(await page.evaluate(() => window.events.filter(e => e.type === "error").length), 0);
});

test("a save answered after a search keeps its reply's feedback after the page is read again", async t => {
  const page = await visit(t);
  await saveThenSearch(page, "Kept feedback", "kept");
  assert.equal(await page.locator("#add-feedback").textContent(), "Added.");
});

test("adding a book without JavaScript redirects back to the filtered page, which shows it", async t => {
  const page = await visit(t, { path: "/?q=native", javaScriptEnabled: false });
  assert.equal(await page.locator("#book-results li").count(), 0);
  await page.locator("#new-book").fill("A native book");
  const navigation = page.waitForNavigation();
  await page.locator("#new-book").press("Enter");
  assert.equal((await navigation).status(), 200);
  // The redirect renders the current results.
  assert.equal(new URL(page.url()).search, "?q=native");
  assert.deepEqual(await page.locator("#book-results li span:first-child").allTextContents(), ["A native book"]);
});
