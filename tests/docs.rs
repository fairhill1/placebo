//! The README embeds the quickstart and rules so readers (and coding agents)
//! get them without following links. These tests keep the copies identical.
const README: &str = include_str!("../README.md");

#[test]
fn readme_quickstart_is_the_quickstart_example() {
    let example = include_str!("../examples/quickstart.rs");
    assert!(
        README.contains(&format!("```rust\n{example}```")),
        "Copy examples/quickstart.rs into the README quickstart block verbatim."
    );
}

#[test]
fn readme_rules_are_the_crate_rules() {
    let rules = include_str!("../docs/rules.md");
    let marker = README.find("<!-- rules:start").expect("rules start marker");
    let start = marker + README[marker..].find("-->\n").expect("closed start marker") + 4;
    let end = README.find("<!-- rules:end -->").expect("rules end marker");
    assert_eq!(
        &README[start..end],
        rules,
        "Copy docs/rules.md into the README verbatim."
    );
}
