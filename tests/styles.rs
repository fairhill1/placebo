//! The styles test fails on every way a page can leave the kit. The fixture
//! in tests/styles plants one problem of each kind next to what it allows.
use placebo::styles::Check;
use std::panic;

#[test]
fn the_kit_placebo_ships_passes() {
    Check::new().kit("kit").run();
}

#[test]
fn every_planted_problem_is_reported() {
    let check = Check::new()
        .kit("tests/styles/kit")
        .app_css("tests/styles/app.css")
        .views("tests/styles/views");
    let report = *panic::catch_unwind(|| check.run())
        .expect_err("the check fails")
        .downcast::<String>()
        .expect("a message");
    let expected = [
        "[placebo:kit-changed] tests/styles/kit/main.css is missing or differs",
        "[placebo:kit-changed] tests/styles/kit/tokens.css is missing or differs",
        "[placebo:kit-changed] tests/styles/kit/reset.css is missing or differs",
        "[placebo:kit-changed] tests/styles/kit/base.css is missing or differs",
        "[placebo:kit-changed] tests/styles/kit/layout.css is missing or differs",
        "[placebo:kit-changed] tests/styles/kit/components.css is missing or differs",
        "[placebo:kit-changed] tests/styles/kit/extra.css is not part of the kit",
        "[placebo:raw-value] tests/styles/app.css:16: `color: #333` has the raw value #333",
        "[placebo:raw-value] tests/styles/app.css:17: `background: rgb(250 240 230)` has the raw value rgb(250 240 230)",
        "[placebo:raw-value] tests/styles/app.css:18: `border: 1px solid White` has the raw value White",
        "[placebo:raw-value] tests/styles/app.css:19: `padding: 0.3rem var(--space-sm)` has the raw value 0.3rem",
        "[placebo:raw-value] tests/styles/app.css:20: `box-shadow: 0 2px 4px var(--scrim)` has the raw value 2px",
        "[placebo:important] tests/styles/app.css:21: `font-weight: var(--weight-bold) !important`",
        "[placebo:raw-value] tests/styles/app.css:22: `padding: calc(var(--space-md) * 1.3)` has the raw value 1.3",
        "[placebo:raw-value] tests/styles/app.css:23: `line-height: 1.4` has the raw value 1.4",
        "[placebo:raw-value] tests/styles/app.css:24: `letter-spacing: 0.06em` has the raw value 0.06em",
        "[placebo:raw-value] tests/styles/app.css:25: `font-size: larger` has the raw value larger",
        "[placebo:raw-value] tests/styles/app.css:26: `font-family: Georgia, serif` has the raw value Georgia, serif",
        "[placebo:raw-value] tests/styles/app.css:27: `transition: opacity 180ms` has the raw value 180ms",
        "[placebo:unlayered] tests/styles/app.css:30: `.stray` is outside the kit's layers",
        "[placebo:style-element] tests/styles/views/page.rs:4:",
        "[placebo:inline-style] tests/styles/views/page.rs:6:",
        "[placebo:inline-style] tests/styles/views/page.rs:7:",
        "[placebo:style-element] tests/styles/views/page.rs:9:",
    ];
    for problem in expected {
        assert!(report.contains(problem), "Missing {problem} in:\n{report}");
    }
    assert_eq!(
        report.matches("[placebo:").count(),
        expected.len(),
        "{report}"
    );
}
