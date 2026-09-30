# The kit: Tailwind v4 and Basecoat

Pages are styled with [Tailwind CSS](https://tailwindcss.com) v4 utilities and
[Basecoat](https://basecoatui.com)'s components, the shadcn/ui design as plain
CSS classes, with Basecoat's theme as it ships. This folder holds only
`basecoat/`: Basecoat's CSS (basecoat-css 1.0.2, MIT), vendored unchanged.
Its JavaScript is not included.

This file is the **contract**: how the styles are built, the classes you get,
and the rules. Basecoat's site shows each component's markup; where this file
and the site differ (JavaScript), this file is right for a Placebo app.

---

## How the stylesheet is built

Tailwind generates the utilities a page uses by scanning the app's sources, so
the stylesheet is compiled, not written by hand:

| File | What it is |
|---|---|
| `styles/app.css` | The app's stylesheet: the input. Committed. |
| `.placebo/kit/` | This folder, copied from the Placebo the app builds against. Ignored by git. |
| `static/app.css` | Tailwind's output, which pages link. Ignored by git. |

```css
/* styles/app.css */
@import "tailwindcss";
@theme {
  /* No default palette: colours come from Basecoat's variables, which follow
     dark mode and a re-skin. Basecoat's overlays and rings use these two. */
  --color-*: initial;
  --color-black: #000;
  --color-white: #fff;
}
@import "../.placebo/kit/basecoat/basecoat.css";
@theme {
  /* Basecoat's stacks without Geist */
  --font-sans: ui-sans-serif, system-ui, sans-serif, …;
  --font-mono: ui-monospace, SFMono-Regular, Menlo, …;
}
```

- `placebo dev` runs Tailwind in watch mode beside the app: a class added to a
  view, or a rule to `styles/app.css`, rebuilds `static/app.css`, and the
  browser reloads.
- `placebo css` builds once; `placebo css --minify` before a release build.
  Deploy `static/app.css` with the binary.
- Both copy the kit into `.placebo/kit` first, so `cargo update -p placebo`
  updates Basecoat with the rest of Placebo.
- Tailwind is its standalone CLI (no Node), pinned by Placebo and downloaded
  once into `~/.cache/placebo/`. `PLACEBO_TAILWINDCSS=/path/to/tailwindcss`
  uses another binary.
- Tailwind finds classes in any file it scans, so a class must appear whole in
  the source: `."text-sm"` or `const ROW: &str = "flex gap-2"`, never
  `format!("text-{size}")`.

---

## The theme

Basecoat's, with two cuts in `styles/app.css`: Tailwind's default palette
and the Geist font, which Basecoat names but doesn't ship, so pages use the
system's font. Light is the default; the `dark` class on `<html>`,
rendered by the server from the stored choice, switches to dark, and
Tailwind's `dark:` variant follows it.

Colours are Basecoat's (shadcn's) variables, each with a Tailwind colour
utility (`bg-primary`, `text-muted-foreground`, `border-border`...). They are
the only colours: `styles/app.css` removes Tailwind's default palette but
`black` and `white`, so `bg-blue-500` generates nothing and the console
reports it as an unknown class. The variables are
`--background`/`--foreground`, `--card`, `--popover`, `--primary`,
`--secondary`, `--muted`, `--accent` (each with a `-foreground`),
`--destructive`, `--border`, `--input`, `--ring`, `--sidebar-*`,
`--chart-1 … 5`; and `--radius`, which `rounded-sm … rounded-xl` step from.
Spacing, type, weights, shadows, and breakpoints are Tailwind's defaults
(`p-4`, `text-sm`, `font-medium`, `shadow-sm`, `md:`).

To re-skin, override the variables in `styles/app.css`, light in `:root` and
dark in `.dark`, inside `@layer base`, as on
[basecoatui.com](https://basecoatui.com/installation). A new scale value (a
colour, a width) is a Tailwind theme variable in `@theme`, such as
`--color-brand` for `bg-brand`, after the person approves it.

---

## The rules

- **Components first, then utilities.** Use Basecoat's component for what it
  covers (a button is `.btn`, never a stack of utilities); lay out and space
  with utilities (`flex gap-2`, `grid md:grid-cols-3`, `text-muted-foreground`).
- **Values come from the theme.** No arbitrary values in classes (`p-[13px]`,
  `bg-[#fff]`, `grid-cols-[auto_1fr]`); an arbitrary *variant* such as
  `[&>svg]:size-4` or `aria-[current=page]:bg-muted` is fine. The styles test
  fails on an arbitrary value in a view or an `@apply`, and on a raw colour,
  size, or duration in `styles/app.css`.
- **A pattern used twice is a Rust `const` of classes** (`NAV_LINK` in the
  starter) or a function returning its markup. Don't copy a long class list
  between views.
- **Every rule is in a Tailwind layer** (`theme`, `base`, `components`,
  `utilities`) or is one of Tailwind's at-rules (`@theme`, `@utility`,
  `@custom-variant`...). An unlayered rule beats every utility, so `md:` and
  `hover:` stop working on what it styles. Nothing is `!important`.
- **A utility overrides a component.** Utilities come after components, so
  `."w-full"` on a `.btn` wins without a fight.
- **No inline styles**, but custom properties: a theme variable written out
  (`style="--gap: var(--spacing)"`) or a value from data
  (`style=(format!("--tag: {}", tag.colour))`), read by a class such as
  `bg-(--tag)`. No `<style>` elements.
- **Hover only where a pointer can hover.** Tailwind v4's `hover:` already
  applies only under `@media (hover: hover)`.
- **Headings are unstyled.** Tailwind's reset makes `h1` plain text; give a
  page heading its size (`text-2xl font-semibold tracking-tight`). Inside a
  `.card > header`, Basecoat styles the `h2`.

---

## Components

Basecoat's markup is on its site, per component. Variants and sizes are data
attributes: `button .btn data-variant="outline" data-size="sm"`.

### Pure CSS: use freely
| Component | Markup |
|---|---|
| Button | `.btn`, `data-variant` = `secondary`, `outline`, `ghost`, `link`, `destructive`; `data-size` = `xs`, `sm`, `lg`, `icon`, `icon-sm`... `.button-group` joins them. |
| Badge | `.badge`, same variants. |
| Card | `.card` holding `header` (an `h2`, a `p`), `section`, `footer`; `data-size="sm"`. |
| Alert | `.alert` holding an icon, an `h2`, and a `section`; `data-variant="destructive"`. |
| Form | `.field` wrapping a `label`, the control, and a `p` hint. Inputs, selects, and textareas are styled inside a `.field` or with `.input`, `.select`, `.textarea`. `.fieldset` groups fields. A checkbox with `role="switch"` is a switch. |
| Table | `.table` in a `.table-container`. |
| Avatar | `.avatar` holding an `img` or a `span` of initials; `data-size`. |
| Others | `.accordion` (of `details`), `.breadcrumb`, `.kbd`, `.progress`, `.skeleton`, `.empty`, `.item`, `[data-tooltip]`. |

### With Placebo's drivers
| Component | How |
|---|---|
| Dialog | `dialog.dialog` holding one element with `header`, `section`, `footer`. Open it with `showModal()`: a `command="show-modal"` button, or `Component::dialog`. |
| Dropdown menu | `details.dropdown-menu` with a `summary.btn` and a `div data-popover` holding `div role="menu"` of `role="menuitem"` buttons or links. The starter's `dropdown` behavior closes it on a press outside or Escape. `data-side="top"` opens it upward. |

### Not available
Basecoat's JavaScript is not loaded, so its tabs, custom select, combobox,
command palette, JS popover, toast, drawer, and collapsible sidebar are out.
Use a native `select`, radios styled with utilities for a small choice (the
starter's theme picker), links for tabs, and
a `dialog` for a drawer. Ask the person before adding a script.

### Icons
`icon!()` renders an SVG the size of its text; Basecoat sizes it inside a
button, alert, or menu item. Size one elsewhere with a utility on its parent,
such as `[&>svg]:size-5`. Label an icon-only button with
`span .sr-only { "Delete" }`.
