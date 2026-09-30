// The app's small browser behaviors. See "Small browser behaviors" in
// Placebo's docs/interactions.md.
import { behavior } from "/placebo.js";

// A form that saves as soon as one of its controls changes, such as the theme
// on Settings.
behavior("autosave", element => {
  const save = event => event.target.form?.requestSubmit();
  element.addEventListener("change", save);
  return () => element.removeEventListener("change", save);
});

// A kit menu (<details class="dropdown">) closes on a press outside it, which
// then does nothing else, and on Escape. Put the behavior on the <details>.
behavior("dropdown", details => {
  let outside = false;
  const press = event => {
    outside = details.open && !details.contains(event.target);
  };
  const click = event => {
    if (!outside) return;
    outside = false;
    details.open = false;
    if (!event.target.closest?.(".dropdown > summary")) {
      event.preventDefault();
      event.stopPropagation();
    }
  };
  const escape = event => {
    if (event.key !== "Escape" || !details.open) return;
    details.open = false;
    details.querySelector("summary")?.focus();
  };
  addEventListener("pointerdown", press, true);
  addEventListener("click", click, true);
  details.addEventListener("keydown", escape);
  return () => {
    removeEventListener("pointerdown", press, true);
    removeEventListener("click", click, true);
    details.removeEventListener("keydown", escape);
  };
});
