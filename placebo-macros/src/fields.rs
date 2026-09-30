use proc_macro2::{Delimiter, Group, Span, TokenStream, TokenTree};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Expr, Ident, Pat, Token, Type,
    ext::IdentExt,
    parse::{ParseStream, Parser},
};

/// Maud owns markup parsing. We only replace unconditional @field entries and
/// identify control-flow boundaries where a required control cannot live.
pub fn expand(tokens: TokenStream) -> syn::Result<TokenStream> {
    let parser = |input: ParseStream| {
        let input_type: Type = input.parse()?;
        let body = brace(input)?;
        let mut fields = Vec::new();
        let markup = rewrite(body.stream(), &mut fields)?;
        let builder = Ident::new("__placebo_fields", Span::mixed_site());
        let controls = fields
            .iter()
            .map(|(name, value, slot): &(Ident, Option<Expr>, Ident)| {
                let rust_name = name.to_string();
                let rust_name = rust_name.trim_start_matches("r#");
                match value {
                    Some(value) => {
                        let setter = format_ident!("with_{rust_name}", span = name.span());
                        quote! {
                            let mut #builder = #builder.#setter(#value);
                            let #slot = ::placebo::__private::take_markup(&mut #builder);
                        }
                    }
                    None => {
                        let omitter = format_ident!("without_{rust_name}", span = name.span());
                        quote! { let #builder = #builder.#omitter(); }
                    }
                }
            });
        Ok(quote! {{
            let #builder = <#input_type as ::placebo::FormInput>::fields();
            #(#controls)*
            #builder.markup(::placebo::__private::html! { #markup }).finish()
        }})
    };
    parser.parse2(tokens)
}

fn brace(input: ParseStream) -> syn::Result<Group> {
    let group: Group = input.parse()?;
    if group.delimiter() != Delimiter::Brace {
        return Err(syn::Error::new(
            group.span(),
            "expected a markup block in braces",
        ));
    }
    Ok(group)
}

fn reject_fields(tokens: TokenStream) -> syn::Result<()> {
    let mut previous_at = false;
    for token in tokens {
        match &token {
            TokenTree::Ident(ident) if previous_at && (ident == "field" || ident == "omit") => {
                return Err(syn::Error::new(
                    ident.span(),
                    "required @field entries must be unconditional markup; move the field outside this branch, loop, or Rust expression",
                ));
            }
            TokenTree::Group(group) => reject_fields(group.stream())?,
            _ => {}
        }
        previous_at = matches!(token, TokenTree::Punct(punct) if punct.as_char() == '@');
    }
    Ok(())
}

fn rewrite(
    tokens: TokenStream,
    fields: &mut Vec<(Ident, Option<Expr>, Ident)>,
) -> syn::Result<TokenStream> {
    let parser = |input: ParseStream| {
        let mut output = TokenStream::new();
        while !input.is_empty() {
            if input.peek(Token![@]) {
                let at: Token![@] = input.parse()?;
                let directive: Ident = input.call(Ident::parse_any)?;
                if directive == "field" || directive == "omit" {
                    let name: Ident = input.parse()?;
                    if fields.iter().any(|(existing, _, _)| existing == &name) {
                        return Err(syn::Error::new(
                            name.span(),
                            "this form field has already been rendered or omitted",
                        ));
                    }
                    let slot = format_ident!(
                        "__placebo_control_{}",
                        fields.len(),
                        span = Span::mixed_site()
                    );
                    // `@omit name;` leaves an optional field out of the form.
                    let value = if directive == "field" {
                        input.parse::<Token![=]>()?;
                        let value: Expr = input.parse()?;
                        output.extend(quote! { (#slot) });
                        Some(value)
                    } else {
                        None
                    };
                    input.parse::<Token![;]>()?;
                    fields.push((name, value, slot));
                    continue;
                }
                at.to_tokens(&mut output);
                directive.to_tokens(&mut output);
                match directive.to_string().as_str() {
                    "if" | "while" | "match" => {
                        input
                            .call(Expr::parse_without_eager_brace)?
                            .to_tokens(&mut output);
                    }
                    "for" => {
                        input
                            .call(Pat::parse_multi_with_leading_vert)?
                            .to_tokens(&mut output);
                        input.parse::<Token![in]>()?.to_tokens(&mut output);
                        input
                            .call(Expr::parse_without_eager_brace)?
                            .to_tokens(&mut output);
                    }
                    "else" => {
                        if input.peek(Token![if]) {
                            input.parse::<Token![if]>()?.to_tokens(&mut output);
                            input
                                .call(Expr::parse_without_eager_brace)?
                                .to_tokens(&mut output);
                        }
                    }
                    "let" => {
                        return Err(syn::Error::new(
                            directive.span(),
                            "declare shared values with a Rust let before fields!; controls are evaluated before the markup",
                        ));
                    }
                    _ => {
                        return Err(syn::Error::new(
                            directive.span(),
                            "expected @field, @omit, or a Maud control-flow directive",
                        ));
                    }
                }
                let body = brace(input)?;
                reject_fields(body.stream())?;
                body.to_tokens(&mut output);
            } else {
                let token: TokenTree = input.parse()?;
                match token {
                    TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                        let mut next =
                            Group::new(Delimiter::Brace, rewrite(group.stream(), fields)?);
                        next.set_span(group.span());
                        next.to_tokens(&mut output);
                    }
                    TokenTree::Group(group) => {
                        reject_fields(group.stream())?;
                        group.to_tokens(&mut output);
                    }
                    other => other.to_tokens(&mut output),
                }
            }
        }
        Ok(output)
    };
    parser.parse2(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_required_fields_that_may_be_omitted_or_repeated() {
        for body in [
            "@if visible { @field title = control; }",
            "@if { visible } { div { @field title = control; } }",
            "@if visible {} @else if other { @field title = control; }",
            "@if visible {} @else { @field title = control; }",
            "@for item in items { @field title = control; }",
            "@while visible { @field title = control; }",
            "@match item { _ => { @field title = control; } }",
            "(html! { @field title = control; })",
            "@field title = control; div { @field title = control; }",
            "@let title = value; @field title = control;",
            "@if visible { @omit title; }",
            "@omit title; @field title = control;",
        ] {
            assert!(
                expand(format!("Input {{ {body} }}").parse().unwrap()).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn forwards_conditional_markup_and_arbitrary_static_layout() {
        let source = quote! { Input {
            @if visible { p { "Visible" } } @else if other { "Other" } @else { "Hidden" }
            @for item in items { (item) }
            @while false { "Never" }
            @match item { _ => { "Any" } }
            fieldset .wide data-state=(state) {
                @field title = Control::text(value);
            }
        } };
        assert!(expand(source).is_ok());
    }
}
