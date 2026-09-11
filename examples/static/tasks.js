import { behavior } from "/placebo.js";

// Native dialogs own modality, Escape handling, and the focus trap. Placebo
// supplies element lifetime; this small behavior supplies application intent.
behavior("dialog", root => {
  const dialog = root.querySelector("dialog");
  const listeners = new AbortController();
  const options = { signal: listeners.signal };
  root.addEventListener("click", event => {
    if (event.target.closest("[data-dialog-open]")) dialog.showModal();
    if (event.target.closest("[data-dialog-close]")) dialog.close();
  }, options);
  dialog.addEventListener("close", () => {
    // A save may have replaced the original trigger's summary fragment.
    if (root.isConnected && !dialog.open &&
        (!document.activeElement || document.activeElement === document.body || root.contains(document.activeElement))) {
      root.querySelector("[data-dialog-open]")?.focus({ preventScroll: true });
    }
  }, options);
  document.addEventListener("placebo:applied", ({ detail }) => {
    if (detail.target === root.dataset.owner && detail.outcome === "applied" && detail.resetLocal.includes("draft")) dialog.close();
  }, options);
  return () => {
    listeners.abort();
    if (dialog.open) dialog.close();
  };
});
