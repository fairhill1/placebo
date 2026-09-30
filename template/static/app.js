// The app's small browser behaviors. See "Small browser behaviors" in
// Placebo's docs/interactions.md.
import { behavior } from "/placebo.js";

// Opening a <details> puts the cursor in its first input.
behavior("focus-on-open", details => {
  const focus = () => {
    if (details.open) details.querySelector("input:not([type=hidden]), textarea")?.focus();
  };
  details.addEventListener("toggle", focus);
  return () => details.removeEventListener("toggle", focus);
});
