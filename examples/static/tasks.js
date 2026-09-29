import { behavior } from "/placebo.js";

// Native dialogs own modality, Escape handling, and the focus trap, and the
// buttons open and close them with command/commandfor, without JavaScript.
// Placebo supplies element lifetime; this small behavior supplies what is
// application intent: close after a save, and where focus goes.
const commands = "commandForElement" in HTMLButtonElement.prototype;
behavior("dialog", root => {
  const dialog = root.querySelector("dialog");
  const listeners = new AbortController();
  const options = { signal: listeners.signal };
  // Browsers without command support get the same buttons from here.
  if (!commands) root.addEventListener("click", event => {
    const button = event.target.closest("button[commandfor]");
    if (button?.getAttribute("command") === "show-modal") dialog.showModal();
    if (button?.getAttribute("command") === "close") dialog.close();
  }, options);
  dialog.addEventListener("close", () => {
    // A save may have replaced the original trigger's summary fragment.
    if (root.isConnected && !dialog.open &&
        (!document.activeElement || document.activeElement === document.body || root.contains(document.activeElement))) {
      root.querySelector("[data-dialog-open]")?.focus({ preventScroll: true });
    }
  }, options);
  // Close after a save the page now shows in full; stay open while the
  // person is already writing something newer.
  document.addEventListener("placebo:applied", ({ detail }) => {
    if (detail.target === root.dataset.owner && detail.outcome === "applied" && !detail.preservedLocal.length) dialog.close();
  }, options);
  return () => {
    listeners.abort();
    if (dialog.open) dialog.close();
  };
});
