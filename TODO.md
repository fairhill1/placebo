# Later

## Keep agents inside the kit

The kit works when pages only compose its classes. nfi-film used the kit
unchanged plus 137 lines of its own CSS; peqori, where CSS was written per
feature, reached 10,400 lines. These would make straying hard:

1. **Kit components as Rust functions**, the way Lumen's `src/kit.rs` did it
   (807 lines). A misspelled function does not compile; a misspelled class
   renders unstyled, in silence.
2. **A development console check** that reports every class in the page that
   no loaded stylesheet defines, as `[placebo:unknown-class]` with the element.
3. **Approval before new CSS.** An agent should not be able to roll its own
   styles without the person saying yes:
   - A Claude Code `ask` permission rule in the app's `.claude/settings.json`
     for edits and writes to `*.css`, so every stylesheet change asks first.
   - An agent can also write files through the shell, so back the rule with a
     test: the vendored kit files match the kit byte for byte, the app's own
     stylesheet stays under a line budget, and `style=` attributes or `<style>`
     elements in views fail it.
   - A rule in the README's rules section: pages compose kit classes only; a
     new visual pattern goes into the kit, after approval.
