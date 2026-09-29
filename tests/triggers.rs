//! Read triggers and reads that insert list items.
use maud::html;
use placebo::{FormInput, List, Position, ReadAction, Region, fields};
use serde::Deserialize;

#[derive(Deserialize, FormInput)]
struct NoInput {}

#[allow(dead_code)]
#[derive(Deserialize, FormInput)]
struct More {
    #[serde(default)]
    shown: u32,
}

const TICK: ReadAction<NoInput> = ReadAction::new("tick", "/tick");
const MORE: ReadAction<More> = ReadAction::new("more", "/more");
const ENTRIES: List = List::new("entries");

#[test]
fn triggers_and_declared_lists_are_in_the_form_configuration() {
    let form = TICK
        .bind(Region::new("clock"))
        .on_load()
        .every(1000)
        .form(fields! { NoInput {} })
        .into_string();
    assert!(form.contains("&quot;load&quot;:true"), "{form}");
    assert!(form.contains("&quot;every_ms&quot;:1000"));
    assert!(!form.contains("reveal"));
    let form = MORE
        .bind(Region::new("more"))
        .on_reveal()
        .affects(ENTRIES)
        .form(fields! { More { @field shown = placebo::Control::hidden(10); } })
        .into_string();
    assert!(form.contains("&quot;reveal&quot;:true"));
    assert!(form.contains("&quot;effects&quot;:[&quot;entries&quot;]"));
}

#[test]
fn an_input_without_fields_decodes_an_empty_query() {
    let _: NoInput = serde_html_form::from_str("").unwrap();
}

#[test]
#[should_panic(expected = "poll between every 500 ms and once a day")]
fn polling_faster_than_twice_a_second_is_refused() {
    let _ = TICK.bind(Region::new("clock")).every(100);
}

#[tokio::test]
async fn a_read_reply_inserts_items_into_its_declared_list() {
    use axum::response::IntoResponse;
    let binding = MORE.bind(Region::new("more")).affects(ENTRIES);
    let reply = binding
        .reply(html! { "next" })
        .also_insert(ENTRIES.item(11).mount(html! { "Entry 11" }), Position::End)
        .into_response();
    let body = axum::body::to_bytes(reply.into_body(), 4096).await.unwrap();
    let update: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(update["patches"][0]["operation"], "insert-item");
    assert_eq!(update["patches"][0]["item"], "entries/11");
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "not declared with .affects()")]
fn a_read_reply_inserts_only_into_declared_lists() {
    let _ = MORE
        .bind(Region::new("more"))
        .reply(html! {})
        .also_insert(ENTRIES.item(1).mount(html! {}), Position::End);
}
