# The kit: a layered, token-driven CSS system

Everything in this folder is a **vendorable** design system: plain CSS custom
properties, cascade layers, and class names. No build step, no framework. To
reuse it, copy the whole `kit/` folder. It knows nothing about Rust, Maud, or
Placebo; a Placebo app is just one way to drive it.

This file is the **contract**: the tokens you set, the classes you get, and the
few non-obvious rules. If it isn't documented here, treat it as internal and
subject to change.

---

## Requirements

- **A modern browser.** The kit uses `light-dark()`, OKLCH with relative color
  syntax (`oklch(from …)`), `@starting-style`, `transition-behavior:
  allow-discrete`, and `@layer`, with **no fallbacks**.
  Baseline is roughly 2024+ evergreen browsers. Where `overlay` transitions are
  unsupported, dialogs and popovers still close; only the exit fade is lost.
- **Load `main.css` and nothing else from the kit.** It declares the layer order
  and imports the other files into their layers:
  ```css
  @layer tokens, reset, base, layout, components, overrides;
  ```
  Serve the folder as static files so the `@import`s resolve next to `main.css`.
- **`reset.css` is included.** Don't stack another reset on top.
- **Bring your own font.** `--font-sans` is the system UI stack; the kit loads
  no web fonts.

---

## Adding your app's CSS

**Every app stylesheet enters a layer.** This is the one rule the whole scheme
depends on. Unlayered styles beat *every* layered rule regardless of
specificity, so a single unlayered file silently wins over the kit and the
cascade order stops meaning anything. That includes tokens: an unlayered
`:root { … }` outranks a layered state rule like
`html[data-sidebar="collapsed"] { --sidebar-width: 3.5rem }`, and the state
never applies.

Make your own entry stylesheet and import the kit first:

```css
/* static/app.css: the only stylesheet the page links */
@import url("kit/main.css");
@import url("app/tokens.css") layer(tokens);         /* re-skin: token values only */
@import url("app/components.css") layer(components); /* your components */
@import url("app/overrides.css") layer(overrides);   /* rare, deliberate exceptions */
```

```html
<link rel="stylesheet" href="/static/app.css">
```

Within one layer, specificity then source order still decide. Your components
come after the kit's in `components`, so a same-specificity rule of yours wins.
Anything in `overrides` beats every component whatever its selector; if that
layer grows, a component is missing a variant.

---

## Tokens

### Public: set these to theme
Override in `@layer tokens` on `:root` (or any scope). Every colour is a
`light-dark(light, dark)` pair; set both halves, hand-picked, never computed.

| Token | Purpose |
|---|---|
| `--surface` | Page ground. |
| `--surface-raised` | Cards, modals, inputs: anything standing off the page. |
| `--surface-sunken` | A well: code, badges, notices, ghost-button hover. |
| `--surface-hover` / `--surface-raised-hover` | Pointer-over tints, measured from `--surface` and `--surface-raised` respectively. Use the one matching the ground the element stands on. |
| `--text`, `--text-muted` | Ink and secondary ink. |
| `--border`, `--border-strong` | Resting hairline and control/emphasis border. |
| `--accent-bg`, `--accent-fg`, `--accent-bg-hover`, `--accent-ink` | **The brand, as a set of four.** Fill, the ink on that fill, the hover fill, and the accent used as text on the page (links). Change them together. |
| `--danger-bg`, `--danger-fg`, `--danger-bg-hover` | Destructive actions and error alerts. Keep its chroma above the accent's so danger outranks brand. |
| `--success-bg`, `--success-fg` | Success alerts. |
| `--shadow-1`, `--shadow-2` | Elevation: resting, floating. Neutral black; tint them if the ground gets real chroma. |
| `--scrim` | The one backdrop for every dialog and popover. |
| `--font-sans`, `--font-mono` | Font stacks. |
| `--text-2xs … --text-2xl` | Type scale (`0.65rem … 2.25rem`). |
| `--leading-tight`, `--leading-normal` | Line heights: headings, body. |
| `--weight-normal`, `--weight-medium`, `--weight-bold` | Font weights. |
| `--space-3xs … --space-3xl` | Spacing scale (`0.125rem … 5.5rem`). |
| `--control-size` | **Exact height of every text input, select, and button.** |
| `--icon-sm`, `--icon-md`, `--icon-lg` | Icon sizes. `sm` is in `em` and tracks the surrounding text. |
| `--measure` | Maximum line length for paragraphs (`68ch`). |
| `--wrapper` | Maximum page width for `.wrapper` (`70rem`). |
| `--border-width` | Every border and hairline. |
| `--radius-sm`, `--radius-md`, `--radius-lg`, `--radius-full` | Corner radii. |
| `--focus-ring-size`, `--focus-offset` | Focus ring width and distance from the control. |
| `--ease`, `--duration-fast`, `--duration-base`, `--duration-slow` | Motion scale: colour change, arrive/leave, size change. |
| `--select-chevron` | The select arrow, as a data URI. Scheme-independent because a data URI can't read a custom property. |

### Derived: do not set (computed for you)
| Token | How it's produced |
|---|---|
| `--control-size-sm` | `--control-size × 0.78`. Controls inside controls, or in a row of headings. |
| `--radius-control` | Points at `--radius-lg`. Inputs, selects and buttons share it so a form row rounds as one. Re-point it to another radius step, never to a literal. |
| `--focus-ring` | `--focus-ring-size solid --accent-bg`. |
| `--transition`, `--transition-base`, `--transition-slow` | Each `--duration-*` with `--ease`. Components use these, never a raw duration. |

### Per-instance inputs
Layout primitives and one component read a custom property with a default, so
an instance adjusts itself without a modifier class. Set these inline or on
the element, not in `:root`.

| Property | Read by | Default |
|---|---|---|
| `--stack-space` | `.stack` | `--space-md` |
| `--cluster-space` | `.cluster` | `--space-sm` |
| `--grid-space`, `--grid-min` | `.grid` | `--space-md`, `16rem` |
| `--sidebar-width` | `.sidebar` | `16rem` |
| `--badge-bg` | `.badge` | none (see below) |

---

## Theming

### Light and dark
Set `data-theme` on `<html>`, rendered by the server from the stored
preference, so the right scheme is in the first byte of HTML:

| Attribute | Scheme |
|---|---|
| none, or `data-theme="system"` | follows the OS |
| `data-theme="light"` | light |
| `data-theme="dark"` | dark |

That is the whole switch: three rules in `base.css` set `color-scheme`, and
every `light-dark()` token resolves against it. No component knows themes
exist. **Never add a `@media (prefers-color-scheme)` block or a
`[data-theme="dark"]` token block**; a second copy of the palette drifts the
first time someone adds a token in a hurry.

### Re-skinning
The default palette is a near-neutral cool ground (hue 250) under a warm accent
(hue 42). The identity is that split, so to re-brand, move the four accent
tokens and leave the ground quiet. In light mode the tint lives in the ink and
the surfaces are almost pure paper; in dark mode the surfaces carry the tint and
the ink goes neutral. `tokens.css` explains why, and it's worth keeping if you
pick a new hue.

### Runtime colours
A colour that arrives from the database (a user-picked tag, a tenant theme)
can't have a hand-picked foreground. Pass it as `--badge-bg` and the badge flips
its text to black or white by lightness, in CSS:

```html
<span class="badge" style="--badge-bg: oklch(70% 0.15 145)">Shipped</span>
```

Design-time colours always get a paired foreground token instead. Don't add a
contrast library for either case.

---

## The rules to internalize

- **Controls don't negotiate height.** `base.css` gives every text input,
  select, button and `.btn` an exact `block-size: var(--control-size)`, and
  draws the select arrow itself (`appearance: none`). A row of input + select +
  button lines up with no per-form fixes. Don't "fix" a row with
  `align-items: stretch`.
- **Components never set their own outer spacing.** The layout parent
  (`.stack`, `.cluster`, `.grid`, a flex `gap`) spaces its children. A component
  with a `margin` stops composing.
- **A new value means the scale is incomplete.** Adding a feature adds
  component lines and zero token lines. If a component needs a colour or size
  the scale doesn't have, fix the scale, don't hardcode a literal.
- **Hover only where a pointer can hover.** Every `:hover` sits inside
  `@media (hover: hover)`; on touch, a bare hover rule is a highlight that
  sticks after the tap.
- **Reduced motion is handled once.** The reset cancels every transition and
  animation, including view transitions. Components never check the preference.

---

## Components (class reference)

### Layout
`.wrapper` (centred, max `--wrapper`, inline padding) · `.stack` (vertical
rhythm) · `.cluster` (+ `.cluster-between`, `.cluster-end`) · `.grid`
(auto-fit columns) · `.sidebar` (two columns that stack below 60% content
width; first child is the sidebar) · `.center`

### Buttons
`.btn` (accent fill) + `.btn-ghost` · `.btn-danger` · `.btn-danger-ghost` ·
`.btn-block` (full width) · `.btn-sm` · `.btn-icon` · `.btn-round`
`.card-actions`: a row of same-size buttons.

Use `.btn-danger` for the one destructive answer in a dialog, and
`.btn-danger-ghost` for destructive actions repeated per row.

### Card
`.card` · `.card-link` (the whole card is the link: put the link in the `h2`;
any second link inside needs `position: relative`)

### Form
`.field` wrapping a `label`, the control, and an optional `.field-hint`.
Inputs, selects and textareas are styled by element, with no class needed.

### Table
`.table`. Row hover is measured for a table on a `.card`.

### Badge
`.badge` · runtime colour via `--badge-bg`

### Feedback
`.alert` (danger by default) + `.alert-success` · `.alert-close`
`.notice`: a quiet, neutral statement about the page (an empty list, a
read-only record)

### Modal
`dialog.modal` wrapping `.modal-panel` · `.modal-close`. Put the padding on
`.modal-panel`, not the dialog, so a click on the visible panel never counts as
a backdrop click.

### Icon and text
`.icon` (inline SVG at `--icon-sm`) · `.lede` · `.muted` · `.visually-hidden`

---

## What's pure CSS vs. needs a JS driver

**Pure CSS:** everything except opening a modal. Top-layer entry and exit
animations for every `<dialog>` and `[popover]` live in `base.css`, so a
native `popover` + `popovertarget` needs no script at all.

**Needs a driver:** `dialog.modal` must be opened with `showModal()`, which
gives you the backdrop, focus trap and Escape for free. A dialog shown any
other way (the `open` attribute alone) stays hidden on purpose, because a
non-modal dialog renders inline at the bottom of the page.

---

## Not in scope (yet)

- No styling for checkboxes, radios, range or file inputs beyond the browser's
  own. They are excluded from the control sizing rules, not restyled.
- No page shell (header, footer, sticky layout), tabs, segmented controls,
  toast container, avatar, or skeletons.
- No styles for framework state such as `aria-busy`; that belongs to the app
  or the framework, not the kit.
- No class namespacing. Classes are unprefixed (`.btn`, `.card`); an app that
  defines its own `.btn` merges with the kit's inside `components`.
- No npm package, minified bundle, or single-file build. The `@import`s are
  fetched as separate requests.

---

## Origin

Extracted from peqori's `static/css`: the layer scheme, tokens, reset, base,
layout, and the generic primitives, with app-specific tokens and components
removed. One structural change follows PHARMA: tokens live in their own layer
instead of being unlayered.
