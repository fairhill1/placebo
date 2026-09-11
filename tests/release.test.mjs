import assert from "node:assert/strict";
import { test } from "node:test";
import { spawn } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { once } from "node:events";

test("release with the dev feature exposes no reload endpoint and serves embedded assets", {
  skip: !process.env.PLACEBO_TEST_RELEASE,
}, async () => {
  const server = spawn(resolve("target/release/examples/editors"), [], {
    env: { ...process.env, PLACEBO_ADDR: "127.0.0.1:0" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  try {
    const origin = await new Promise((accept, reject) => {
      let output = "";
      const timeout = setTimeout(() => reject(new Error("Release example did not start.")), 10000);
      server.stdout.on("data", chunk => {
        output += chunk;
        const match = output.match(/http:\/\/127\.0\.0\.1:\d+/);
        if (match) { clearTimeout(timeout); accept(match[0]); }
      });
      server.once("error", error => { clearTimeout(timeout); reject(error); });
      server.once("exit", code => { clearTimeout(timeout); reject(new Error(`Release example exited: ${code}`)); });
    });
    const html = await (await fetch(origin)).text();
    assert.ok(!html.includes("data-event-stream"));
    assert.ok(!html.includes("tower-livereload"));
    assert.equal((await fetch(`${origin}/_tower-livereload/event-stream`)).status, 404);
    const css = await (await fetch(`${origin}/editors.css`)).text();
    const path = "examples/static/editors.css";
    const source = await readFile(path, "utf8");
    try {
      await writeFile(path, `${source}\n:root { --release-probe: should-not-appear; }\n`);
      assert.equal(await (await fetch(`${origin}/editors.css`)).text(), css);
    } finally { await writeFile(path, source); }
  } finally {
    if (server.exitCode === null) {
      const exited = once(server, "exit");
      server.kill();
      await exited;
    }
  }
});
