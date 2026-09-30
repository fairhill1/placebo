#[test]
fn styles_stay_in_the_kit() {
    placebo::styles::Check::new()
        .app_css("static/app.css")
        .app_css("static/components.css")
        .views("src")
        .run();
}
