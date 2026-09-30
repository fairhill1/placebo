// Not compiled: the styles check reads it as text.
fn page(status: &str) -> Markup {
    html! {
        head { style { "p { color: red }" } }
        div .stack style="--stack-space: var(--space-xs)" {
            p style="color: var(--text-muted)" { "Muted" }
            span .badge style=(format!("color: {status}")) { (status) }
        }
        (PreEscaped("<style>p {}</style>"))
        span .badge style=(format!("--badge-bg: {status}")) { (status) }
    }
}
