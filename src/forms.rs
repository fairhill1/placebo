//! Input-derived form builders. Controls render their own generated names;
//! completion requires every declared field, including fields with serde defaults.
use maud::{Markup, PreEscaped, html};
use serde::de::DeserializeOwned;
use std::{fmt::Display, marker::PhantomData};

#[doc = include_str!("../docs/typed-forms.md")]
pub trait FormInput: DeserializeOwned + Sized {
    type Builder: private::FormBuilder<Input = Self>;
    fn fields() -> Self::Builder;
}

/// A completed form for exactly one payload type. Constructed by finish() on
/// an input-derived builder after every field has been emitted once.
pub struct FormFields<I> {
    markup: Markup,
    input: PhantomData<fn() -> I>,
}

impl<I> FormFields<I> {
    pub(crate) fn into_markup(self) -> Markup {
        self.markup
    }
}

mod sealed {
    pub trait Value {}
}

/// Primitive values whose Display encoding matches their form deserialization.
/// Checkboxes, optional fields, collections, and custom codecs need separate
/// submission rules and are not part of this first typed-control API.
pub trait FieldValue: sealed::Value + Display + DeserializeOwned {}
macro_rules! values {
    ($($ty:ty),* $(,)?) => { $(impl sealed::Value for $ty {} impl FieldValue for $ty {})* };
}
values!(
    String, bool, u8, u16, u32, u64, usize, i8, i16, i32, i64, isize
);

enum Kind {
    Hidden(String),
    Text(String, &'static str),
    Select(Vec<(String, String, bool)>),
}

/// A control with a payload value type. The builder supplies its field name;
/// there is no name override or disabled option that could remove it from submission.
pub struct Control<T> {
    kind: Kind,
    id: Option<String>,
    class: Option<String>,
    described_by: Option<String>,
    autocomplete: Option<String>,
    placeholder: Option<String>,
    value: PhantomData<fn() -> T>,
}

impl<T> Control<T> {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            id: None,
            class: None,
            described_by: None,
            autocomplete: None,
            placeholder: None,
            value: PhantomData,
        }
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = Some(id.into());
        self
    }
    pub fn class(mut self, class: &str) -> Self {
        self.class = Some(class.into());
        self
    }
    pub fn described_by(mut self, id: &str) -> Self {
        self.described_by = Some(id.into());
        self
    }
    pub fn autocomplete(mut self, value: &str) -> Self {
        self.autocomplete = Some(value.into());
        self
    }
    pub fn placeholder(mut self, value: &str) -> Self {
        self.placeholder = Some(value.into());
        self
    }

    #[doc(hidden)]
    pub fn render_named(self, name: &str) -> Markup {
        match self.kind {
            Kind::Hidden(value) => html! { input type="hidden" name=(name) value=(value); },
            Kind::Text(value, kind) => html! {
                input type=(kind) name=(name) value=(value) id=[self.id] class=[self.class]
                    aria-describedby=[self.described_by] autocomplete=[self.autocomplete] placeholder=[self.placeholder];
            },
            Kind::Select(options) => html! {
                select name=(name) id=[self.id] class=[self.class] aria-describedby=[self.described_by] {
                    @for (value, label, selected) in options {
                        option value=(value) selected[selected] { (label) }
                    }
                }
            },
        }
    }
}

impl<T: FieldValue> Control<T> {
    pub fn hidden(value: T) -> Self {
        Self::new(Kind::Hidden(value.to_string()))
    }

    pub fn select<L: Into<String>>(selected: T, options: impl IntoIterator<Item = (T, L)>) -> Self
    where
        T: PartialEq,
    {
        let options: Vec<_> = options
            .into_iter()
            .map(|(value, label)| {
                let is_selected = value == selected;
                (value.to_string(), label.into(), is_selected)
            })
            .collect();
        assert!(
            options.iter().any(|(_, _, selected)| *selected),
            "a select needs an option matching its initial value"
        );
        Self::new(Kind::Select(options))
    }
}

impl Control<String> {
    pub fn text(value: impl Into<String>) -> Self {
        Self::new(Kind::Text(value.into(), "text"))
    }
    pub fn search(value: impl Into<String>) -> Self {
        Self::new(Kind::Text(value.into(), "search"))
    }
}

#[doc(hidden)]
pub mod private {
    use super::*;
    pub use maud::{Markup, html};
    pub struct Missing;
    pub struct Present;

    #[derive(Default)]
    pub struct FormBuffer {
        html: String,
    }

    impl FormBuffer {
        pub fn push(&mut self, markup: Markup) {
            self.html.push_str(&markup.into_string());
        }
        pub fn len(&self) -> usize {
            self.html.len()
        }
        pub fn is_empty(&self) -> bool {
            self.html.is_empty()
        }
        pub fn wrap_group(&mut self, start: usize, class: &str) {
            let content = PreEscaped(self.html.split_off(start));
            self.push(html! { div class=(class) { (content) } });
        }
        pub fn wrap_local(&mut self, start: usize, key: &str) {
            assert!(!key.is_empty(), "local state needs a key");
            let content = PreEscaped(self.html.split_off(start));
            self.push(html! { div data-placebo-local=(key) { (content) } });
        }
    }

    pub trait FormBuilder {
        type Input: FormInput;
        fn buffer(&mut self) -> &mut FormBuffer;
    }

    pub fn take_markup(builder: &mut impl FormBuilder) -> Markup {
        PreEscaped(std::mem::take(&mut builder.buffer().html))
    }

    pub fn finish<I: FormInput>(body: FormBuffer) -> FormFields<I> {
        FormFields {
            markup: PreEscaped(body.html),
            input: PhantomData,
        }
    }
}
