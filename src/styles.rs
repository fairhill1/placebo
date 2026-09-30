//! Keep pages inside the theme. Pages compose Basecoat's components and
//! Tailwind's utilities, whose values come from the theme; an agent that
//! reaches for an arbitrary value (`p-[13px]`), a raw colour in CSS, or an
//! inline style grows a second design system beside it. Maud accepts any
//! class or attribute, so nothing stops that at compile time. [`Check`], in
//! the app's `cargo test`, keeps the app's stylesheet on the theme and in
//! Tailwind's layers, and styles out of the views. The runtime reports a
//! class no stylesheet defines, such as a utility Tailwind could not
//! generate, as `[placebo:unknown-class]`.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Tailwind's cascade layers.
const LAYERS: &str = "theme base components utilities";
/// The at-rules a Tailwind stylesheet writes outside a layer.
const TAILWIND_AT_RULES: &str = "@import @theme @custom-variant @source @plugin @utility";
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
/// Sizes that step off the theme's type scale.
const FONT_SIZE_KEYWORDS: &str =
    "xx-small x-small small medium large x-large xx-large xxx-large smaller larger";
const APPROVE: &str = "a component Basecoat lacks goes in the app's styles/app.css, in \
    `@layer components`, built with @apply from the theme's utilities";

/// Checks, in the app's `cargo test`, that its styles stay inside the theme.
/// An agent can write any file through the shell, past a Claude Code rule
/// that asks before CSS edits; this test sees it.
///
/// ```no_run
/// #[test]
/// fn styles_stay_in_the_theme() {
///     placebo::styles::Check::new()
///         .app_css("styles/app.css")
///         .views("src")
///         .run();
/// }
/// ```
///
/// Paths are relative to the package, where `cargo test` runs. `run` panics
/// listing every problem with its file, line, and what to do instead.
#[derive(Default)]
pub struct Check {
    app_css: Vec<PathBuf>,
    views: Vec<PathBuf>,
}

impl Check {
    pub fn new() -> Self {
        Self::default()
    }

    /// One of the app's own stylesheets, such as `styles/app.css`; call it
    /// once per file. Every rule sits in one of Tailwind's layers or is one
    /// of its at-rules (`@theme`, `@utility`...), nothing is `!important`,
    /// and no `@apply` uses an arbitrary value such as `p-[13px]`.
    /// Outside `@theme` and `@layer theme`, colours, font families, line
    /// heights, letter spacing, and durations are theme variables; margins,
    /// padding, gaps, font sizes and weights, and radii are theme variables
    /// or 0, not scaled by a multiplier but the spacing unit's, as in
    /// `--spacing(4)`; a font size is never a keyword such as `larger`; and no
    /// length is in px but 1px. Other lengths, such as a sidebar's width, are
    /// free.
    pub fn app_css(mut self, path: impl Into<PathBuf>) -> Self {
        self.app_css.push(path.into());
        self
    }

    /// A directory of Rust sources, such as `src`, that must use no arbitrary
    /// value in a class (`p-[13px]`, `bg-[#fff]`; an arbitrary variant such
    /// as `[&>svg]:size-4` is fine), write no `<style>` element, and no
    /// `style=` attribute but one that sets custom properties: to theme
    /// variables when written out, `style="--gap: var(--spacing)"`, or to
    /// data, `style=(format!("--tag: {}", tag.colour))`.
    pub fn views(mut self, dir: impl Into<PathBuf>) -> Self {
        self.views.push(dir.into());
        self
    }

    /// Panics listing every problem, so one run shows them all.
    pub fn run(self) {
        let mut problems = Vec::new();
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
            "Styles left the theme. Pages compose Basecoat's components and Tailwind's \
             utilities; {APPROVE}.\n\n{}\n",
            problems.join("\n")
        );
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
        let at_rule =
            (TAILWIND_AT_RULES.split(' ')).any(|name| text.split_whitespace().next() == Some(name));
        if piece.within.is_empty() && !at_rule && !layer(&text, LAYERS) {
            problems.push(format!(
                "[placebo:unlayered] {place} is outside Tailwind's layers, so it beats every \
                 component and utility. Put it in `@layer components` (or theme, base, utilities)."
            ));
        }
        if let Some(classes) = text.strip_prefix("@apply ")
            && let Some(class) = (classes.split_whitespace()).find(|class| arbitrary(class, true))
        {
            problems.push(arbitrary_problem(&place, class));
        }
        let declaration = !piece.block && !piece.within.is_empty() && !text.starts_with('@');
        let Some((property, value)) = text.split_once(':').filter(|_| declaration) else {
            continue;
        };
        if value.replace(' ', "").contains("!important") {
            problems.push(format!(
                "[placebo:important] {place} is !important. Tailwind's layers order the \
                 cascade; use a later layer, or a utility, instead."
            ));
        }
        let tokens = (piece.within.iter())
            .any(|prelude| layer(prelude, "theme") || prelude.trim() == "@theme");
        if let Some(raw) =
            raw_value(&property.trim().to_ascii_lowercase(), value).filter(|_| !tokens)
        {
            problems.push(format!(
                "[placebo:raw-value] {place} has the raw value {raw}. Use the theme: \
                 `@apply p-4 text-muted-foreground`, or var(--color-muted-foreground) and \
                 --spacing(4). A value the theme lacks is a new variable in `@theme`, after \
                 the person approves it: the theme is what keeps every page consistent."
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
    // Text in strings is not a value, and a multiple of the spacing unit
    // is on the scale: --spacing(4), calc(var(--spacing) * 4).
    let value = on_spacing_scale(value);
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
    // Type follows the scale: leading and tracking are theme variables, and
    // so is a size, never a keyword such as `larger`.
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
                    // a length does: calc(var(--text-sm) * 1.3).
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

/// The value with each multiple of Tailwind's spacing unit, `--spacing(4)`
/// or `var(--spacing) * 4`, replaced by the unit alone.
fn on_spacing_scale(value: &str) -> String {
    let mut value = value.to_owned();
    for (start, call) in [("--spacing(", true), ("var(--spacing) * ", false)] {
        let mut from = 0;
        while let Some(at) = value[from..].find(start).map(|at| from + at) {
            let rest = &value[at + start.len()..];
            // A call runs to its `)`; a factor is a number.
            let end = if call {
                rest.find(')').map_or(rest.len(), |end| end + 1)
            } else {
                let sign = usize::from(rest.starts_with('-'));
                (rest[sign..].find(|c: char| !c.is_ascii_digit() && c != '.'))
                    .map_or(rest.len(), |end| sign + end)
            };

            value.replace_range(at..at + start.len() + end, "var(--spacing)");
            from = at + "var(--spacing)".len();
        }
    }
    value
}

/// Whether a class uses an arbitrary value, such as `p-[13px]` or
/// `hover:bg-[#fff]/50`, or with `property`, an arbitrary property such as
/// `[mask-type:alpha]`. Its variants may be arbitrary, as in `[&>svg]:size-4`
/// or `aria-[current=page]:bg-muted`: they select, not style.
fn arbitrary(class: &str, property: bool) -> bool {
    let (mut depth, mut start) = (0, 0);
    for (at, c) in class.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ':' if depth == 0 => start = at + 1,
            _ => {}
        }
    }
    let utility = class[start..].trim_start_matches('-');
    let name = utility.find("-[").is_some_and(|at| {
        (utility[..at].bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    (name && utility.starts_with(|c: char| c.is_ascii_lowercase()))
        || (property && utility.starts_with('['))
}

fn arbitrary_problem(place: &str, class: &str) -> String {
    format!(
        "[placebo:arbitrary-value] {place} uses the arbitrary value `{class}`. Use the theme's \
         scale, such as p-3 or text-sm; a value the theme lacks is a new variable in `@theme`, \
         after the person approves it."
    )
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
/// `style=` attribute other than a literal that sets custom properties to theme
/// variables.
fn view_problems(file: &str, source: &str, problems: &mut Vec<String>) {
    for (number, line) in source.lines().enumerate() {
        // Classes are in strings: `."p-[3px]"`, `class="..."`, `.class("...")`.
        // Only `name-[value]` counts here: `[...]` alone is as likely a log line.
        let class = (line.split('"').skip(1).step_by(2))
            .flat_map(str::split_whitespace)
            .find(|class| arbitrary(class, false));
        if let Some(class) = class {
            let place = format!("{file}:{}: `{}`", number + 1, line.trim());
            problems.push(arbitrary_problem(&place, class));
        }
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
                    "[placebo:style-element] {place} writes a <style> element. Use Basecoat's \
                     components and Tailwind's utilities; {APPROVE}."
                ));
            } else if after.starts_with('=')
                && !literal.is_some_and(|style| custom_properties(style, true))
                && !from_data.is_some_and(|style| custom_properties(style, false))
            {
                problems.push(format!(
                    "[placebo:inline-style] {place} sets an inline style. A view may only set \
                     custom properties: to a theme variable when written out, such as \
                     style=\"--gap: var(--spacing)\", or to a value from data, such as \
                     style=(format!(\"--tag: {{}}\", tag.colour)). Use Tailwind's utilities \
                     otherwise."
                ));
            }
        }
    }
}

/// Whether a `style` value only sets custom properties, such as
/// `--gap: var(--spacing)`: a way to adjust one instance.
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
    fn app_rules_sit_in_tailwind_layers_without_important() {
        let problems = css(
            "@import \"tailwindcss\";\n.card { gap: 0; }\n@media (width > 40rem) {}\n@layer extra {}\n\
             @layer components { .card { gap: var(--spacing) !important; } }\n\
             @theme { --color-brand: #c05; }\n@utility tab-4 { tab-size: 4; }\n@custom-variant x (&:hover);\n\
             @layer components { .tag { @apply p-[3px] [&>svg]:size-4; } .tab { @apply aria-[current=page]:bg-muted; } }",
        );
        assert_eq!(problems.len(), 5, "{problems:#?}");
        assert!(problems[0].starts_with("[placebo:unlayered] app.css:2: `.card`"));
        assert!(problems[1].starts_with("[placebo:unlayered] app.css:3: `@media (width > 40rem)`"));
        assert!(problems[2].starts_with("[placebo:unlayered] app.css:4: `@layer extra`"));
        assert!(problems[3].starts_with("[placebo:important] app.css:5"));
        assert!(
            problems[4].starts_with("[placebo:arbitrary-value] app.css:9"),
            "{}",
            problems[4]
        );
        assert!(problems[4].contains("`p-[3px]`"));
    }

    #[test]
    fn arbitrary_values_are_found_but_arbitrary_variants_allowed() {
        for class in [
            "p-[13px]",
            "hover:bg-[#fff]/50",
            "md:w-[calc(100%-2rem)]",
            "-mt-[2px]",
            "[mask-type:alpha]",
        ] {
            assert!(arbitrary(class, true), "{class}");
        }
        for class in [
            "p-3",
            "[&>svg]:size-4",
            "aria-[current=page]:bg-muted",
            "has-[:checked]:ring",
            "w-(--sidebar-width)",
        ] {
            assert!(!arbitrary(class, true), "{class}");
        }
        assert!(!arbitrary("[placebo:unknown-class]", false));
        let problems = views(
            "p .\"p-[3px]\" {}\ndiv class=\"flex gap-[7px]\" {}\neprintln!(\"[warn] a[href]\");\nspan .\"[&>svg]:size-4\" {}",
        );
        assert_eq!(problems.len(), 2, "{problems:#?}");
        assert!(problems[0].starts_with("[placebo:arbitrary-value] view.rs:1:"));
        assert!(problems[1].contains("`gap-[7px]`"));
    }

    #[test]
    fn values_come_from_the_theme_outside_it() {
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
            raw("padding", "calc(var(--text-sm) * 1.3)"),
            Some("1.3".into())
        );
        assert_eq!(raw("padding", "--spacing(4) 13px"), Some("13px".into()));
        assert_eq!(raw("line-height", "1.4"), Some("1.4".into()));
        assert_eq!(raw("letter-spacing", "0.06em"), Some("0.06em".into()));
        assert_eq!(raw("font-size", "larger"), Some("larger".into()));
        assert_eq!(
            raw("font-family", "\"Red Hat\", serif"),
            Some("\"Red Hat\", serif".into())
        );
        assert_eq!(raw("transition", "opacity 180ms"), Some("180ms".into()));
        for (property, value) in [
            ("color", "var(--color-foreground)"),
            ("background", "oklch(from var(--primary) l c h / 0.5)"),
            ("border", "1px solid transparent"),
            ("padding", "0 var(--spacing)"),
            ("margin-inline", "auto"),
            ("margin", "calc(var(--spacing) * -1)"),
            ("letter-spacing", "var(--tracking-wide)"),
            ("line-height", "var(--leading-tight)"),
            ("--sidebar-width", "17rem"),
            ("grid-template-columns", "minmax(12rem, 1fr) 8rem"),
            ("gap", "--spacing(1) --spacing(4)"),
            ("font-family", "var(--font-mono)"),
            ("font-family", "inherit"),
            ("transition", "color var(--default-transition-duration)"),
            ("padding", "--spacing(4) --spacing(2.5)"),
            ("margin", "calc(var(--spacing) * 4)"),
            ("font-size", "var(--text-sm)"),
        ] {
            assert_eq!(raw(property, value), None, "{property}: {value}");
        }
        assert_eq!(
            css(
                "@layer theme { :root { --brand: #c05; --gap: 3px; } }\n@theme { --color-brand: oklch(0.6 0.2 30); }"
            ),
            Vec::<String>::new()
        );

        assert!(
            css("@layer components { .x { color: red } }")[0]
                .starts_with("[placebo:raw-value] app.css:1")
        );
    }

    #[test]
    fn views_set_no_styles_but_custom_properties() {
        assert!(views("p .\"flex gap-(--gap)\" style=\"--gap: var(--spacing)\" {}\nlet style = 1;\ndata-style=\"a\"\n\
                       span .badge style=(format!(\"--tag: {}\", tag.colour)) {}").is_empty());
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
    fn inline_styles_may_only_set_custom_properties_to_theme_variables() {
        let tokens = |style| custom_properties(style, true);
        assert!(tokens("--gap: var(--spacing)"));
        assert!(tokens(
            " --gap: var(--spacing); --width:var(--container-sm); "
        ));
        assert!(!tokens("color: var(--color-foreground)"));
        assert!(!tokens("--width: 9rem"));
        assert!(!tokens("--tag: var(--a, red)"));
        assert!(!tokens("--a: var(--b) var(--c)"));
        assert!(custom_properties("--tag: {}", false));
        assert!(!custom_properties("--tag: {}; color: {}", false));
    }
}
