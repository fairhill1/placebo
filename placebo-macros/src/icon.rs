//! `icon!("name")`: a Lucide icon as inline SVG, looked up while compiling so
//! a misspelled name fails the build and the binary holds only named icons.
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::{Map, Value};
use std::sync::OnceLock;
use syn::LitStr;

const LUCIDE: &str = include_str!("../icons/lucide.json");

fn icons() -> &'static Map<String, Value> {
    static ICONS: OnceLock<Map<String, Value>> = OnceLock::new();
    ICONS.get_or_init(|| serde_json::from_str(LUCIDE).expect("icons/lucide.json is an object"))
}

pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let name: LitStr = syn::parse2(input)?;
    let svg = svg(&name.value()).map_err(|message| syn::Error::new(name.span(), message))?;
    Ok(quote! { ::placebo::__private::PreEscaped(#svg) })
}

/// The icon's SVG, the size of the text around it (Basecoat sizes it inside a
/// button, alert, or menu), coloured by its text, and hidden from screen
/// readers: the text beside it, or a visually hidden label, names it.
fn svg(name: &str) -> Result<String, String> {
    let Some(Value::Array(elements)) = icons().get(name) else {
        return Err(unknown(name));
    };
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1em\" height=\"1em\" \
         viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" \
         stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\">",
    );
    for element in elements {
        let (Some(tag), Some(Value::Object(attributes))) = (element[0].as_str(), element.get(1))
        else {
            return Err(format!(
                "icons/lucide.json has a malformed element in `{name}`"
            ));
        };
        svg.push('<');
        svg.push_str(tag);
        for (attribute, value) in attributes {
            let value = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            svg.push_str(&format!(
                " {attribute}=\"{}\"",
                value.replace('"', "&quot;")
            ));
        }
        svg.push_str("/>");
    }
    svg.push_str("</svg>");
    Ok(svg)
}

fn unknown(name: &str) -> String {
    let mut near: Vec<(usize, &str)> = icons()
        .keys()
        .map(|known| (distance(name, known), known.as_str()))
        .filter(|(distance, known)| *distance <= 2 || known.contains(name))
        .collect();
    near.sort();
    let near: Vec<String> = near
        .iter()
        .take(5)
        .map(|(_, known)| format!("`{known}`"))
        .collect();
    let hint = if near.is_empty() {
        String::new()
    } else {
        format!(" Did you mean {}?", near.join(", "))
    };
    format!(
        "Lucide has no icon `{name}`.{hint} Names are listed at https://lucide.dev/icons \
         (Lucide 1.49.0)."
    )
}

/// Levenshtein distance, for suggestions.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous + usize::from(ca != *cb);
            previous = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(previous + 1);
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_renders_its_elements_inside_a_text_sized_svg() {
        let check = svg("check").unwrap();
        assert!(
            check.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1em\""),
            "{check}"
        );
        assert!(check.contains("aria-hidden=\"true\""));
        assert!(
            check.ends_with("<path d=\"M20 6 9 17l-5-5\"/></svg>"),
            "{check}"
        );
    }

    #[test]
    fn an_unknown_name_suggests_near_ones() {
        let error = svg("chek").unwrap_err();
        assert!(error.contains("Lucide has no icon `chek`"), "{error}");
        assert!(error.contains("`check`"), "{error}");
    }
}
