use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

mod fields;

#[proc_macro]
pub fn fields(input: TokenStream) -> TokenStream {
    fields::expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(FormInput, attributes(serde))]
pub fn form_input(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "FormInput currently requires a concrete input struct without generics",
        ));
    }
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("serde"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("deny_unknown_fields") { Ok(()) }
            else { Err(meta.error("FormInput supports only serde(deny_unknown_fields) on structs; use serde(rename = \"...\") on individual fields")) }
        })?;
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "FormInput requires a struct with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "FormInput requires named fields",
        ));
    };
    if fields.named.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "FormInput requires at least one field",
        ));
    }
    let name = &input.ident;
    let visibility = &input.vis;
    let builder = format_ident!("{name}Fields");
    let states: Vec<_> = (0..fields.named.len())
        .map(|i| format_ident!("__F{i}"))
        .collect();
    let missing = quote!(::placebo::__private::Missing);
    let present = quote!(::placebo::__private::Present);
    let defaults = states.iter().map(|s| quote!(#s = #missing));
    let all_present = states.iter().map(|_| present.clone());
    let mut wire_names = std::collections::HashSet::new();
    let mut setters = Vec::new();

    for (index, field) in fields.named.iter().enumerate() {
        let ident = field.ident.as_ref().unwrap();
        let rust_name = ident.to_string();
        let rust_name = rust_name.trim_start_matches("r#");
        let setter = format_ident!("with_{rust_name}");
        let mut wire_name = rust_name.to_owned();
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("serde"))
        {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    wire_name = meta.value()?.parse::<LitStr>()?.value();
                    Ok(())
                } else if meta.path.is_ident("default") {
                    if meta.input.peek(syn::Token![=]) { let _: LitStr = meta.value()?.parse()?; }
                    Ok(())
                } else {
                    Err(meta.error("FormInput supports only serde(rename = \"...\") and serde(default) on fields; flatten/skip/custom codecs cannot preserve this form contract"))
                }
            })?;
        }
        if wire_name.is_empty() || !wire_names.insert(wire_name.clone()) {
            return Err(syn::Error::new_spanned(
                ident,
                "form field names must be nonempty and unique",
            ));
        }
        let ty = &field.ty;
        let other_states: Vec<_> = states
            .iter()
            .enumerate()
            .filter_map(|(i, state)| (i != index).then_some(state))
            .collect();
        let impl_generics = if other_states.is_empty() {
            quote!()
        } else {
            quote!(<#(#other_states),*>)
        };
        let before = states.iter().enumerate().map(|(i, state)| {
            if i == index {
                missing.clone()
            } else {
                quote!(#state)
            }
        });
        let after = states.iter().enumerate().map(|(i, state)| {
            if i == index {
                present.clone()
            } else {
                quote!(#state)
            }
        });
        setters.push(quote! {
            impl #impl_generics #builder<#(#before),*> {
                pub fn #setter(mut self, control: ::placebo::Control<#ty>) -> #builder<#(#after),*> {
                    self.body.push(control.render_named(#wire_name));
                    #builder { body: self.body, state: ::core::marker::PhantomData }
                }
            }
        });
    }

    Ok(quote! {
        #visibility struct #builder<#(#defaults),*> {
            body: ::placebo::__private::FormBuffer,
            state: ::core::marker::PhantomData<(#(#states,)*)>,
        }

        impl ::placebo::FormInput for #name {
            type Builder = #builder;
            fn fields() -> Self::Builder {
                #builder { body: ::core::default::Default::default(), state: ::core::marker::PhantomData }
            }
        }

        impl<#(#states),*> ::placebo::__private::FormBuilder for #builder<#(#states),*> {
            type Input = #name;
            fn buffer(&mut self) -> &mut ::placebo::__private::FormBuffer { &mut self.body }
        }

        impl<#(#states),*> #builder<#(#states),*> {
            /// Add surrounding markup. Named controls belong in with_* methods.
            pub fn markup(mut self, markup: ::placebo::__private::Markup) -> Self {
                self.body.push(markup);
                self
            }

            pub fn group<Next>(self, class: &str, render: impl FnOnce(Self) -> Next) -> Next
            where Next: ::placebo::__private::FormBuilder<Input = #name> {
                let start = self.body.len();
                let mut next = render(self);
                ::placebo::__private::FormBuilder::buffer(&mut next).wrap_group(start, class);
                next
            }

            pub fn local<Next>(self, key: &str, render: impl FnOnce(Self) -> Next) -> Next
            where Next: ::placebo::__private::FormBuilder<Input = #name> {
                let start = self.body.len();
                let mut next = render(self);
                ::placebo::__private::FormBuilder::buffer(&mut next).wrap_local(start, key);
                next
            }
        }

        #(#setters)*

        impl #builder<#(#all_present),*> {
            pub fn finish(self) -> ::placebo::FormFields<#name> {
                ::placebo::__private::finish(self.body)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_serde_contracts_are_rejected_instead_of_silently_misrendered() {
        for source in [
            "struct Input { #[serde(flatten)] data: String }",
            "struct Input { #[serde(skip)] data: String }",
            "#[serde(rename_all = \"camelCase\")] struct Input { some_data: String }",
            "struct Input { #[serde(rename = \"x\")] a: String, #[serde(rename = \"x\")] b: String }",
        ] {
            assert!(expand(syn::parse_str(source).unwrap()).is_err(), "{source}");
        }
    }
}
