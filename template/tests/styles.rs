#[test]
fn styles_stay_in_the_theme() {
    placebo::styles::Check::new()
        .app_css("styles/app.css")
        .views("src")
        .run();
}
