// The app's small browser behaviors. See "Small browser behaviors" in
// Placebo's docs/interactions.md.
import { behavior } from "/placebo.js";

// A <details> row that adds a record: opening it puts the cursor in its
// input, and Escape closes it again.
behavior("add-row", details => {
  const input = () => details.querySelector("input:not([type=hidden])");
  const opened = () => { if (details.open) input()?.focus(); };
  const escape = event => {
    if (event.key !== "Escape") return;
    details.open = false;
    details.querySelector("summary")?.focus();
  };
  details.addEventListener("toggle", opened);
  details.addEventListener("keydown", escape);
  return () => {
    details.removeEventListener("toggle", opened);
    details.removeEventListener("keydown", escape);
  };
});
