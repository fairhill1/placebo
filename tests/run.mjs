// Runs the browser tests. Each test file starts its own example server, so
// files run in parallel, except those that share something: the editors and
// release tests rewrite the same stylesheet, and the dev test runs cargo.
// PLACEBO_BROWSERS="chromium firefox webkit" runs several engines at once;
// PLACEBO_TEST_CONCURRENCY sets how many files run at once per engine.
import { spawn } from "node:child_process";
import { readdirSync } from "node:fs";

const SERIAL = ["dev.test.mjs", "editors.test.mjs", "release.test.mjs"];
const files = readdirSync("tests").filter(name => name.endsWith(".test.mjs")).sort();
const parallel = files.filter(name => !SERIAL.includes(name)).map(name => `tests/${name}`);
const serial = files.filter(name => SERIAL.includes(name)).map(name => `tests/${name}`);
const browsers = (process.env.PLACEBO_BROWSERS ?? process.env.PLACEBO_BROWSER ?? "chromium").split(/[\s,]+/).filter(Boolean);
const concurrency = process.env.PLACEBO_TEST_CONCURRENCY ?? "2";
const several = browsers.length > 1;

// With several engines at once, each one's output is printed when it ends.
function run(browser, args) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, ["--test", ...args], {
      env: { ...process.env, PLACEBO_BROWSER: browser },
      stdio: several ? ["ignore", "pipe", "pipe"] : "inherit",
    });
    let output = "";
    child.stdout?.on("data", chunk => { output += chunk; });
    child.stderr?.on("data", chunk => { output += chunk; });
    child.on("close", code => {
      if (several) process.stdout.write(`\n=== ${browser} ===\n${output}`);
      resolve(code === 0);
    });
  });
}

const results = await Promise.all(browsers.map(browser => run(browser, [`--test-concurrency=${concurrency}`, ...parallel])));
for (const browser of browsers) results.push(await run(browser, ["--test-concurrency=1", ...serial]));
if (results.includes(false)) {
  console.error(`\nBrowser tests failed (${browsers.join(", ")}).`);
  process.exitCode = 1;
}
