//! Read forms and polling.
use placebo::{FormInput, Read, fields};
use serde::Deserialize;

#[derive(Deserialize, FormInput)]
struct NoInput {}

#[allow(dead_code)]
#[derive(Deserialize, FormInput)]
struct More {
    #[serde(default)]
    shown: u32,
}

#[test]
fn triggers_are_in_the_form_configuration() {
    let form = Read::new()
        .on_reveal()
        .form(fields! { More { @field shown = placebo::Control::hidden(10); } })
        .into_string();
    assert!(form.contains("&quot;reveal&quot;:true"), "{form}");
    assert!(!form.contains("input_delay_ms"));
    assert!(form.contains(r#"<input type="hidden" name="shown" value="10">"#));
}

#[test]
fn an_input_without_fields_decodes_an_empty_query() {
    let _: NoInput = serde_html_form::from_str("").unwrap();
    let form = Read::new().form(fields! { NoInput {} }).into_string();
    assert!(form.starts_with("<form method=\"get\""));
}

#[test]
#[should_panic(expected = "input delay exceeds one minute")]
fn an_input_delay_over_a_minute_is_refused() {
    let _ = Read::new().on_input(60_001);
}
