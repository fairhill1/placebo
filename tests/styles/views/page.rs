// Not compiled: the styles check reads it as text.
fn page(status: &str) -> Markup {
    html! {
        head { style { "p { color: red }" } }
        div ."flex gap-(--gap)" style="--gap: var(--spacing)" {
            p style="color: var(--color-muted-foreground)" { "Muted" }
            span .badge style=(format!("color: {status}")) { (status) }
        }
        (PreEscaped("<style>p {}</style>"))
        span .badge style=(format!("--tag: {status}")) { (status) }
        p ."text-[13px]" ."[&>svg]:size-4" { "Small" }
    }
}
