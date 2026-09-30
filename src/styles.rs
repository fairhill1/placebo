//! Keep pages inside the CSS kit. The kit works when pages compose its
//! classes; an agent that invents a class and writes CSS for it, or reaches
//! for an inline style, grows a second design system beside it. Maud accepts
//! any class or attribute, so nothing stops that at compile time. [`Check`],
//! in the app's `cargo test`, keeps a vendored kit unchanged, the app's
//! stylesheet on tokens and in its layers, and styles out of the views. The
//! runtime reports a class no stylesheet defines as `[placebo:unknown-class]`.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The kit this version of Placebo ships.
pub(crate) const KIT: [(&str, &str); 6] = [
    ("main.css", include_str!("../kit/main.css")),
    ("tokens.css", include_str!("../kit/tokens.css")),
    ("reset.css", include_str!("../kit/reset.css")),
    ("base.css", include_str!("../kit/base.css")),
    ("layout.css", include_str!("../kit/layout.css")),
    ("components.css", include_str!("../kit/components.css")),
];
/// The kit's cascade layers, as `main.css` declares them.
const LAYERS: &str = "tokens reset base layout components overrides";
const COLOUR_FUNCTIONS: &str =
    "rgb rgba hsl hsla hwb lab lch oklab oklch color color-mix light-dark";
/// Properties that take a colour, in part: `color`, `background`, `border-top`...
const COLOUR_PROPERTIES: &str = "color background border outline shadow fill stroke decoration";
/// Every CSS named colour. `transparent` and `currentColor` name none.
const NAMED_COLOURS: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black \
    blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral \
    cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen \
    darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon \
    darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink \
    deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro \
    ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory \
    khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan \
    lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen \
    lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen \
    magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen \
    mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream \
    mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid \
    palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum \
    powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown \
    seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen \
    steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen";
/// Sizes that step off the kit's type scale.
const FONT_SIZE_KEYWORDS: &str =
    "xx-small x-small small medium large x-large xx-large xxx-large smaller larger";
const APPROVE: &str = "a component the kit lacks goes in the app's own components.css, \
    built from the kit's tokens";

/// Checks, in the app's `cargo test`, that its styles stay inside the kit.
/// An agent can write any file through the shell, past a Claude Code rule
/// that asks before CSS edits; this test sees it.
///
/// ```no_run
/// #[test]
/// fn styles_stay_in_the_kit() {
///     placebo::styles::Check::new()
///         .app_css("static/app.css")
///         .app_css("static/components.css")
///         .views("src")
///         .run();
/// }
/// ```
///
/// Paths are relative to the package, where `cargo test` runs. `run` panics
/// listing every problem with its file, line, and what to do instead.
#[derive(Default)]
pub struct Check {
    kit: Option<PathBuf>,
    app_css: Vec<PathBuf>,
    views: Vec<PathBuf>,
}

impl Check {
    pub fn new() -> Self {
        Self::default()
    }

    /// For an app that vendors a copy of the kit rather than serving it with
    /// [`crate::kit`]: the copy, which must match the kit this Placebo ships
    /// byte for byte. The kit's README has apps re-skin it by setting tokens
    /// in `@layer tokens` in their own stylesheet, never by editing the copy.
    pub fn kit(mut self, dir: impl Into<PathBuf>) -> Self {
        self.kit = Some(dir.into());
        self
    }

    /// One of the app's own stylesheets, such as `static/app.css` or the
    /// `static/components.css` where the components the kit lacks live; call
    /// it once per file. Every rule sits in one of the kit's layers (a file
    /// imported into a layer still writes its `@layer` block) and nothing is
    /// `!important`.
    /// Outside `@layer tokens`, colours, font families, line heights, letter
    /// spacing, and durations are tokens; margins, padding, gaps, font sizes
    /// and weights, and radii are tokens or 0, not scaled by a multiplier; a
    /// font size is never a keyword such as `larger`; and no length is in px
    /// but 1px. Other lengths, such as a sidebar's width, are free.
    pub fn app_css(mut self, path: impl Into<PathBuf>) -> Self {
        self.app_css.push(path.into());
        self
    }

    /// A directory of Rust sources, such as `src`, that must write no `<style>`
    /// element, and no `style=` attribute but one that sets custom properties:
    /// to tokens when written out, `style="--stack-space: var(--space-xs)"`,
    /// or to data, `style=(format!("--badge-bg: {}", tag.colour))`.
    pub fn views(mut self, dir: impl Into<PathBuf>) -> Self {
        self.views.push(dir.into());
        self
    }

    /// Panics listing every problem, so one run shows them all.
    pub fn run(self) {
        let mut problems = Vec::new();
        if let Some(dir) = &self.kit {
            kit_problems(dir, &mut problems);
        }
        for path in &self.app_css {
            match fs::read_to_string(path) {
                Ok(css) => css_problems(&path.display().to_string(), &css, &mut problems),
                Err(error) => problems.push(format!("{}: {error}", path.display())),
            }
        }
        for dir in &self.views {
            view_files(dir, &mut problems);
        }
        assert!(
            problems.is_empty(),
            "Styles left the kit's scale. Pages compose the kit's classes; {APPROVE}.\n\n{}\n",
            problems.join("\n")
        );
    }
}

fn kit_problems(dir: &Path, problems: &mut Vec<String>) {
    let next = "Copy Placebo's kit/ folder again, and re-skin with tokens in `@layer tokens` in \
                the app's stylesheet";
    for (name, shipped) in KIT {
        let path = dir.join(name);
        if fs::read_to_string(&path).ok().as_deref() != Some(shipped) {
            problems.push(format!(
                "[placebo:kit-changed] {} is missing or differs from the kit this Placebo ships. \
                 {next}; {APPROVE}.",
                path.display()
            ));
        }
    }
    let files = fs::read_dir(dir).into_iter().flatten().flatten();
    for path in files.map(|file| file.path()) {
        if path.extension().is_some_and(|ext| ext == "css")
            && !KIT.iter().any(|(name, _)| path.ends_with(name))
        {
            let path = path.display();
            problems.push(format!(
                "[placebo:kit-changed] {path} is not part of the kit. Remove it; {APPROVE}."
            ));
        }
    }
}

fn css_problems(file: &str, css: &str, problems: &mut Vec<String>) {
    let css = blank_comments(css);
    let layer = |prelude: &str, names: &str| {
        (names.split(' ')).any(|name| prelude.split_whitespace().eq(["@layer", name]))
    };
    for piece in pieces(&css) {
        let text = piece.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = css[..piece.at].matches('\n').count() + 1;
        let place = format!("{file}:{line}: `{text}`");
        if piece.within.is_empty() && !text.starts_with("@import") && !layer(&text, LAYERS) {
            problems.push(format!(
                "[placebo:unlayered] {place} is outside the kit's layers, so it beats every kit \
                 rule. Put it in `@layer components` (or tokens, layout, overrides)."
            ));
        }
        let declaration = !piece.block && !piece.within.is_empty() && !text.starts_with('@');
        let Some((property, value)) = text.split_once(':').filter(|_| declaration) else {
            continue;
        };
        if value.replace(' ', "").contains("!important") {
            problems.push(format!(
                "[placebo:important] {place} is !important. The kit's layers order the cascade; \
                 use a later layer instead."
            ));
        }
        let tokens = piece.within.iter().any(|prelude| layer(prelude, "tokens"));
        if let Some(raw) =
            raw_value(&property.trim().to_ascii_lowercase(), value).filter(|_| !tokens)
        {
            problems.push(format!(
                "[placebo:raw-value] {place} has the raw value {raw}. Use a token, such as \
                 var(--space-md) or var(--text-muted). A value the scale lacks is a new \
                 token in `@layer tokens`, after the person approves it: the scale is what \
                 keeps every page consistent."
            ));
        }
    }
}

/// The first value in a declaration that should be a token; see [`Check::app_css`].
fn raw_value(property: &str, value: &str) -> Option<String> {
    // A family is a token: --font-sans or --font-mono.
    if property == "font-family"
        && !value.contains("var(")
        && !["inherit", "initial", "unset", "revert"].contains(&value.trim())
    {
        return Some(value.trim().to_owned());
    }
    // Text in strings is not a value.
    let value = value
        .split(['"', '\''])
        .step_by(2)
        .collect::<Vec<_>>()
        .join(" ");
    for name in COLOUR_FUNCTIONS.split(' ') {
        for (at, _) in value.match_indices(&format!("{name}(")) {
            let end = value[at..]
                .find(')')
                .map_or(value.len(), |end| at + end + 1);
            let call = &value[at..end];
            if !value[..at].ends_with(|c: char| c.is_alphanumeric() || c == '-')
                && !call.contains("var(")
            {
                return Some(call.to_owned());
            }
        }
    }
    let colour = COLOUR_PROPERTIES
        .split(' ')
        .any(|part| property.contains(part));
    let spacing = property.starts_with("margin")
        || property.starts_with("padding")
        || property.ends_with("gap")
        || property == "font-size"
        || (property.starts_with("border") && property.ends_with("radius"));
    // Type follows the scale: leading and tracking are tokens, and so is a
    // size, never a keyword such as `larger`.
    let typography = property == "line-height" || property == "letter-spacing";
    let timing = property.starts_with("transition") || property.starts_with("animation");
    for word in value.split(|c: char| c.is_whitespace() || ",()/*+".contains(c)) {
        let number = word.trim_start_matches('-');
        let digits = number
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(number.len());
        let (amount, unit) = (number[..digits].parse().unwrap_or(0.0), &number[digits..]);
        let raw = match word.strip_prefix('#') {
            Some(hex) => {
                [3, 4, 6, 8].contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit())
            }
            None if amount != 0.0 => {
                (unit == "px" && amount != 1.0)
                    // A multiplier moves a value off the scale as surely as
                    // a length does: calc(var(--space-md) * 1.3).
                    || (spacing && (!unit.is_empty() || amount != 1.0))
                    || typography
                    || (timing && (unit == "ms" || unit == "s"))
                    || property == "font-weight"
            }
            None if property == "font-size"
                && FONT_SIZE_KEYWORDS.split(' ').any(|keyword| word == keyword) =>
            {
                true
            }
            None => {
                colour
                    && NAMED_COLOURS
                        .split_whitespace()
                        .any(|name| word.eq_ignore_ascii_case(name))
            }
        };
        if raw {
            return Some(word.to_owned());
        }
    }
    None
}

fn view_files(dir: &Path, problems: &mut Vec<String>) {
    let Ok(files) = fs::read_dir(dir) else {
        return problems.push(format!("{}: no views to check here.", dir.display()));
    };
    let mut paths: Vec<_> = files.flatten().map(|file| file.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            view_files(&path, problems);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = fs::read_to_string(&path).unwrap_or_default();
            view_problems(&path.display().to_string(), &source, problems);
        }
    }
}

/// A `<style>` element (Maud's `style {`, or `<style` in a string), or a
/// `style=` attribute other than a literal that sets custom properties to tokens.
fn view_problems(file: &str, source: &str, problems: &mut Vec<String>) {
    for (number, line) in source.lines().enumerate() {
        for (at, _) in line.match_indices("style") {
            let (before, after) = (&line[..at], &line[at + "style".len()..]);
            if before.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-') {
                continue;
            }
            let place = format!("{file}:{}: `{}`", number + 1, line.trim());
            let literal = after
                .strip_prefix("=\"")
                .and_then(|value| value.split('"').next());
            let from_data = after
                .strip_prefix("=(")
                .and_then(|value| value.split('"').nth(1));
            if before.ends_with('<') || after.trim_start().starts_with('{') {
                problems.push(format!(
                    "[placebo:style-element] {place} writes a <style> element. Use the kit's \
                     classes; {APPROVE}."
                ));
            } else if after.starts_with('=')
                && !literal.is_some_and(|style| custom_properties(style, true))
                && !from_data.is_some_and(|style| custom_properties(style, false))
            {
                problems.push(format!(
                    "[placebo:inline-style] {place} sets an inline style. A view may only set \
                     custom properties: to a token when written out, such as \
                     style=\"--stack-space: var(--space-xs)\", or to a value from data, such as \
                     style=(format!(\"--badge-bg: {{}}\", tag.colour)). Use the kit's classes \
                     otherwise."
                ));
            }
        }
    }
}

/// Whether a `style` value only sets custom properties, such as
/// `--stack-space: var(--space-xs)`: the kit's way to adjust one instance.
/// A value written out must be a token; one filled in from data, such as a
/// user's tag colour, may be anything.
fn custom_properties(style: &str, tokens: bool) -> bool {
    let custom = |name: &str| {
        name.len() > 2
            && name.starts_with("--")
            && (name.bytes()).all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    };
    let mut settings = style
        .split(';')
        .map(str::trim)
        .filter(|setting| !setting.is_empty());
    settings.all(|setting| {
        setting.split_once(':').is_some_and(|(name, value)| {
            let token = value
                .trim()
                .strip_prefix("var(")
                .and_then(|value| value.strip_suffix(')'));
            custom(name.trim()) && (!tokens || token.is_some_and(custom))
        })
    })
}

/// A piece of a stylesheet: a block's prelude (a selector or an @-rule), or a
/// declaration or statement. `at` is where it starts, and `within` holds the
/// preludes of the blocks around it, outermost first.
struct Piece<'a> {
    text: &'a str,
    at: usize,
    block: bool,
    within: Vec<&'a str>,
}

/// Replace comments with spaces, keeping every line break where it was.
fn blank_comments(css: &str) -> String {
    let mut css = css.to_owned();
    while let Some(start) = css.find("/*") {
        let end = css[start + 2..]
            .find("*/")
            .map_or(css.len(), |end| start + end + 4);
        let blank: String = (css[start..end].chars())
            .map(|c| if c == '\n' { c } else { ' ' })
            .collect();
        css.replace_range(start..end, &blank);
    }
    css
}

/// Split a stylesheet, its comments blanked, into pieces. A string is
/// skipped whole, so a `;` or `{` in one splits nothing.
fn pieces(css: &str) -> Vec<Piece<'_>> {
    let (bytes, mut pieces, mut within, mut start, mut at) =
        (css.as_bytes(), Vec::new(), Vec::new(), 0, 0);
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'"' | b'\'') => {
                at += 1;
                while at < bytes.len() && bytes[at] != quote {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
            }
            end @ (b'{' | b';' | b'}') => {
                let text = css[start..at].trim();
                if end == b'{' || !text.is_empty() {
                    let offset = start + css[start..at].len() - css[start..at].trim_start().len();
                    let (block, within) = (end == b'{', within.clone());
                    pieces.push(Piece {
                        text,
                        at: offset,
                        block,
                        within,
                    });
                }
                match end {
                    b'{' => within.push(text),
                    b'}' => drop(within.pop()),
                    _ => {}
                }
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn css(css: &str) -> Vec<String> {
        let mut problems = Vec::new();
        css_problems("app.css", css, &mut problems);
        problems
    }

    fn views(source: &str) -> Vec<String> {
        let mut problems = Vec::new();
        view_problems("view.rs", source, &mut problems);
        problems
    }

    #[test]
    fn the_layers_are_the_ones_the_kit_declares() {
        let order = format!("@layer {};", LAYERS.replace(' ', ", "));
        assert!(KIT[0].1.contains(&order), "{order}");
    }

    #[test]
    fn pieces_skip_comments_and_strings() {
        let sheet = blank_comments(
            "/* .note { a; } */\n@import url(\"a;b.css\");\n@layer components {\n  .card { content: \"}\"; }\n}",
        );
        let pieces: Vec<_> = pieces(&sheet)
            .into_iter()
            .map(|piece| {
                (
                    piece.block,
                    piece.text,
                    sheet[..piece.at].matches('\n').count(),
                    piece.within,
                )
            })
            .collect();
        assert_eq!(
            pieces,
            [
                (false, "@import url(\"a;b.css\")", 1, vec![]),
                (true, "@layer components", 2, vec![]),
                (true, ".card", 3, vec!["@layer components"]),
                (
                    false,
                    "content: \"}\"",
                    3,
                    vec!["@layer components", ".card"]
                ),
            ]
        );
    }

    #[test]
    fn app_rules_sit_in_the_kit_layers_without_important() {
        let problems = css(
            "@import url(\"kit/main.css\");\n.card { gap: 0; }\n@media (width > 40rem) {}\n@layer extra {}\n\
             @layer components { .card { gap: var(--space-sm) !important; } }",
        );
        assert_eq!(problems.len(), 4, "{problems:#?}");
        assert!(problems[0].starts_with("[placebo:unlayered] app.css:2: `.card`"));
        assert!(problems[1].starts_with("[placebo:unlayered] app.css:3: `@media (width > 40rem)`"));
        assert!(problems[2].starts_with("[placebo:unlayered] app.css:4: `@layer extra`"));
        assert!(problems[3].starts_with("[placebo:important] app.css:5"));
    }

    #[test]
    fn values_are_tokens_outside_the_tokens_layer() {
        let raw = |property, value| raw_value(property, value);
        assert_eq!(raw("color", "#fff"), Some("#fff".into()));
        assert_eq!(
            raw("box-shadow", "0 1px 2px rgb(0 0 0 / 0.1)"),
            Some("rgb(0 0 0 / 0.1)".into())
        );
        assert_eq!(raw("border", "1px solid White"), Some("White".into()));
        assert_eq!(raw("margin-block-start", "0.2em"), Some("0.2em".into()));
        assert_eq!(raw("font-size", "calc(1rem + 2px)"), Some("1rem".into()));
        assert_eq!(raw("font-weight", "600"), Some("600".into()));
        assert_eq!(raw("inset", "-3px"), Some("-3px".into()));
        assert_eq!(raw("border-top-left-radius", "50%"), Some("50%".into()));
        assert_eq!(
            raw("padding", "calc(var(--space-md) * 1.3)"),
            Some("1.3".into())
        );
        assert_eq!(raw("line-height", "1.4"), Some("1.4".into()));
        assert_eq!(raw("letter-spacing", "0.06em"), Some("0.06em".into()));
        assert_eq!(raw("font-size", "larger"), Some("larger".into()));
        assert_eq!(
            raw("font-family", "\"Red Hat\", serif"),
            Some("\"Red Hat\", serif".into())
        );
        assert_eq!(raw("transition", "opacity 180ms"), Some("180ms".into()));
        for (property, value) in [
            ("color", "var(--text)"),
            ("background", "oklch(from var(--accent-bg) l c h / 0.5)"),
            ("border", "1px solid transparent"),
            ("padding", "0 var(--space-sm)"),
            ("margin-inline", "auto"),
            ("margin", "calc(var(--space-sm) * -1)"),
            ("letter-spacing", "var(--tracking-wide)"),
            ("line-height", "var(--leading-tight)"),
            ("--sidebar-width", "17rem"),
            ("grid-template-columns", "minmax(12rem, 1fr) 8rem"),
            ("gap", "var(--space-2xs) var(--space-md)"),
            ("font-family", "var(--font-mono)"),
            ("font-family", "inherit"),
            ("transition", "color var(--transition)"),
        ] {
            assert_eq!(raw(property, value), None, "{property}: {value}");
        }
        assert_eq!(
            css("@layer tokens { :root { --brand: #c05; --gap: 3px; } }"),
            Vec::<String>::new()
        );
        assert!(
            css("@layer components { .x { color: red } }")[0]
                .starts_with("[placebo:raw-value] app.css:1")
        );
    }

    #[test]
    fn views_set_no_styles_but_tokens() {
        assert!(views("p .stack style=\"--stack-space: var(--space-xs)\" {}\nlet style = 1;\ndata-style=\"a\"\n\
                       span .badge style=(format!(\"--badge-bg: {}\", tag.colour)) {}").is_empty());
        let problems = views(
            "style { \"p {}\" }\n\"<style>p{}</style>\"\np style=\"color: red\" {}\np style=(format!(\"color: {b}\")) {}",
        );
        let codes: Vec<_> = problems
            .iter()
            .map(|problem| &problem[..problem.find(']').unwrap() + 1])
            .collect();
        assert_eq!(
            codes,
            [
                "[placebo:style-element]",
                "[placebo:style-element]",
                "[placebo:inline-style]",
                "[placebo:inline-style]"
            ]
        );
        assert!(problems[3].contains("view.rs:4:"), "{}", problems[3]);
    }

    #[test]
    fn inline_styles_may_only_set_custom_properties_to_tokens() {
        let tokens = |style| custom_properties(style, true);
        assert!(tokens("--stack-space: var(--space-xs)"));
        assert!(tokens(
            " --grid-min: var(--wrapper); --grid-space:var(--space-lg); "
        ));
        assert!(!tokens("color: var(--text)"));
        assert!(!tokens("--grid-min: 9rem"));
        assert!(!tokens("--badge-bg: var(--a, red)"));
        assert!(!tokens("--a: var(--b) var(--c)"));
        assert!(custom_properties("--badge-bg: {}", false));
        assert!(!custom_properties("--badge-bg: {}; color: {}", false));
    }
}
