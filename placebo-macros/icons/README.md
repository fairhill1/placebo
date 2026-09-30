# Lucide icons

`lucide.json` is `icon-nodes.json` from the `lucide-static` npm package,
version 1.49.0, unmodified: every icon's name with the SVG elements it draws.
`icon!` reads it while compiling, so an app's binary holds only the icons it
names. `LICENSE` is Lucide's (ISC, with Feather's MIT notice).

To update, replace `lucide.json` with the file from a newer `lucide-static`
(`npm pack lucide-static`) and change the version above.
