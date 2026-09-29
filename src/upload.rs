//! File fields. A payload field of type [`Upload`] (or `Option`/`Vec` of one)
//! makes its form `multipart/form-data`. The limit is part of the type, so the
//! browser's check and the server's check come from one place.
use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use maud::{DOCTYPE, html};
use serde::{Deserialize, Deserializer, de::Error as _};
use std::cell::RefCell;

use crate::forms::{FormValue, private};

/// The limit of an [`Upload`] without an explicit one: 10 MiB.
pub const DEFAULT_MAX_BYTES: usize = 10 * 1024 * 1024;

/// A file submitted with a form. `MAX_BYTES` is the most this field accepts;
/// for a `Vec<Upload<N>>` field, the most for all its files together. The
/// runtime refuses a larger file before sending it, and the server rejects one
/// with HTTP 413 before the handler runs.
///
/// ```
/// use placebo::{Control, FormInput, Upload, fields};
/// use serde::Deserialize;
///
/// #[derive(Deserialize, FormInput)]
/// struct Attach {
///     title: String,
///     cover: Option<Upload<{ 2 * 1024 * 1024 }>>,
///     #[serde(default)]
///     pages: Vec<Upload>,
/// }
///
/// let fields = fields! { Attach {
///     @field title = Control::text("");
///     @field cover = Control::file().accept("image/*");
///     @field pages = Control::file();
/// } };
/// ```
///
/// The file name and content type come from the browser; treat them as
/// untrusted text.
#[derive(Clone, Debug)]
pub struct Upload<const MAX_BYTES: usize = DEFAULT_MAX_BYTES> {
    file_name: String,
    content_type: Option<String>,
    bytes: Bytes,
}

impl<const MAX_BYTES: usize> Upload<MAX_BYTES> {
    /// The name the browser gave, without any directory.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }
    pub fn into_bytes(self) -> Bytes {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// A file read from a multipart body, before it is decoded into its field.
pub(crate) struct File {
    pub name: String,
    pub file_name: String,
    pub content_type: Option<String>,
    pub bytes: Bytes,
}

thread_local! {
    /// The files of the body being decoded. Decoding is synchronous, so a
    /// field's token finds its file on this thread.
    static FILES: RefCell<Vec<Option<File>>> = const { RefCell::new(Vec::new()) };
}

const TOKEN: &str = "placebo-upload:";

/// Run `decode` with `files` available to [`Upload`] fields. Returns the
/// form pairs with a token in place of each file.
pub(crate) fn with_files<T>(
    files: Vec<File>,
    decode: impl FnOnce(Vec<(String, String)>) -> T,
) -> T {
    let tokens = files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.name.clone(), format!("{TOKEN}{index}")))
        .collect();
    FILES.with(|slot| *slot.borrow_mut() = files.into_iter().map(Some).collect());
    let decoded = decode(tokens);
    FILES.with(|slot| slot.borrow_mut().clear());
    decoded
}

impl<'de, const MAX_BYTES: usize> Deserialize<'de> for Upload<MAX_BYTES> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let token = String::deserialize(deserializer)?;
        token
            .strip_prefix(TOKEN)
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| FILES.with(|files| files.borrow_mut().get_mut(index)?.take()))
            .map(|file| Upload {
                file_name: file.file_name,
                content_type: file.content_type,
                bytes: file.bytes,
            })
            .ok_or_else(|| {
                D::Error::custom("a file field needs a file in a multipart/form-data body")
            })
    }
}

/// The payload types a file control can submit: [`Upload`], `Option<Upload>`,
/// and `Vec<Upload>` (several files).
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a file field",
    note = "Control::file() needs an Upload, Option<Upload>, or Vec<Upload> field"
)]
pub trait FileValue: FormValue {
    #[doc(hidden)]
    const MAX_BYTES: usize;
    #[doc(hidden)]
    const MULTIPLE: bool;
}

impl<const N: usize> crate::forms::sealed::Field for Upload<N> {}
impl<const N: usize> FormValue for Upload<N> {
    const ABSENT: private::Absent = private::Absent::Required;
    const UPLOAD: Option<usize> = Some(N);
}
impl<const N: usize> FileValue for Upload<N> {
    const MAX_BYTES: usize = N;
    const MULTIPLE: bool = false;
}
impl<const N: usize> crate::forms::sealed::Field for Option<Upload<N>> {}
impl<const N: usize> FormValue for Option<Upload<N>> {
    const ABSENT: private::Absent = private::Absent::None;
    const UPLOAD: Option<usize> = Some(N);
}
impl<const N: usize> FileValue for Option<Upload<N>> {
    const MAX_BYTES: usize = N;
    const MULTIPLE: bool = false;
}
impl<const N: usize> crate::forms::sealed::Field for Vec<Upload<N>> {}
impl<const N: usize> FormValue for Vec<Upload<N>> {
    const ABSENT: private::Absent = private::Absent::Empty;
    const UPLOAD: Option<usize> = Some(N);
}
impl<const N: usize> FileValue for Vec<Upload<N>> {
    const MAX_BYTES: usize = N;
    const MULTIPLE: bool = true;
}

/// Text fields together may take this much besides the files.
const TEXT_BYTES: usize = 1024 * 1024;
/// Room for boundaries and part headers in a whole multipart body.
const FRAMING_BYTES: usize = 256 * 1024;

/// Read a multipart body: text fields as pairs, and the files of the declared
/// file fields within their limits. `fields` maps each file field to its limit.
pub(crate) async fn read(
    headers: &HeaderMap,
    body: Body,
    fields: &[(&'static str, usize)],
) -> Result<(Vec<(String, String)>, Vec<File>), Box<Response>> {
    let boundary = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| multer::parse_boundary(value).ok())
        .ok_or_else(|| malformed("the multipart boundary is missing"))?;
    // The whole body is bounded too, so many small parts cannot grow without
    // limit. The files' limits, the text limit, and the framing add up to it.
    let whole = fields
        .iter()
        .fold(TEXT_BYTES + FRAMING_BYTES, |total, (_, limit)| {
            total.saturating_add(*limit)
        });
    let constraints = multer::Constraints::new()
        .size_limit(multer::SizeLimit::new().whole_stream(whole as u64));
    let mut multipart =
        multer::Multipart::with_constraints(body.into_data_stream(), boundary, constraints);
    let read_error = |error: multer::Error| match error {
        multer::Error::StreamSizeExceeded { .. } => body_too_large(whole),
        error => malformed(&error.to_string()),
    };
    let mut pairs = Vec::new();
    let mut files = Vec::new();
    let mut text_bytes = 0;
    let mut used = vec![0usize; fields.len()];
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(read_error)?
    {
        let name = field.name().unwrap_or_default().to_owned();
        let upload = fields.iter().position(|(field, _)| *field == name);
        let Some(file_name) = field.file_name().map(file_name) else {
            if upload.is_some() {
                return Err(malformed(&format!("field '{name}' expects a file")));
            }
            let mut value = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(read_error)? {
                text_bytes += chunk.len();
                if text_bytes > TEXT_BYTES {
                    return Err(malformed("the text fields are too large"));
                }
                value.extend_from_slice(&chunk);
            }
            let value = String::from_utf8(value)
                .map_err(|_| malformed(&format!("field '{name}' is not UTF-8 text")))?;
            pairs.push((name, value));
            continue;
        };
        let Some(index) = upload else {
            return Err(malformed(&format!("field '{name}' is not a file field")));
        };
        let limit = fields[index].1;
        let content_type = field.content_type().map(ToString::to_string);
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(read_error)? {
            used[index] += chunk.len();
            if used[index] > limit {
                return Err(too_large(&name, &file_name, limit));
            }
            bytes.extend_from_slice(&chunk);
        }
        // A file input with no file chosen submits an empty, unnamed part.
        if file_name.is_empty() && bytes.is_empty() {
            continue;
        }
        files.push(File {
            name,
            file_name,
            content_type,
            bytes: bytes.into(),
        });
    }
    Ok((pairs, files))
}

/// Browsers send only the file's own name, but some older ones sent a path.
fn file_name(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn malformed(reason: &str) -> Box<Response> {
    Box::new(
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Failed to read multipart form body: {reason}"),
        )
            .into_response(),
    )
}

/// A multipart body over its whole limit, refused before the handler ran.
fn body_too_large(limit: usize) -> Box<Response> {
    let mut response = (
        StatusCode::PAYLOAD_TOO_LARGE,
        format!(
            "Failed to read multipart form body: it is larger than {}, the most this form's \
             fields accept together",
            readable_size(limit)
        ),
    )
        .into_response();
    response.headers_mut().insert(
        "x-placebo-upload-limit",
        axum::http::HeaderValue::from(limit),
    );
    Box::new(response)
}

fn readable_size(bytes: usize) -> String {
    match bytes {
        bytes if bytes >= 1024 * 1024 => format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)),
        bytes if bytes >= 1024 => format!("{} KB", bytes / 1024),
        bytes => format!("{bytes} bytes"),
    }
}

/// A file over its field's limit. The runtime checks sizes before sending, so
/// this mostly answers native submissions: say what happened and what to do.
fn too_large(field: &str, file: &str, limit: usize) -> Box<Response> {
    let limit_text = readable_size(limit);
    #[cfg(debug_assertions)]
    eprintln!(
        "[placebo:upload-too-large] field={field} limit={limit} bytes: the request was refused \
         before the handler ran. Raise the field's Upload<MAX_BYTES> if larger files are expected."
    );
    let _ = field;
    let mut response = (
        StatusCode::PAYLOAD_TOO_LARGE,
        [(header::CACHE_CONTROL, "no-store")],
        html! {
            (DOCTYPE)
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { "File too large" }
                }
                body style="font: 1.1rem/1.5 system-ui, sans-serif; max-width: 36rem; margin: 3rem auto; padding: 0 1rem" {
                    h1 { "File too large" }
                    p { "“" (file) "” is larger than " (limit_text) ", the most this form accepts. Nothing was saved. Go back, choose a smaller file, and submit again." }
                }
            }
        },
    )
        .into_response();
    response.headers_mut().insert(
        "x-placebo-upload-limit",
        axum::http::HeaderValue::from(limit),
    );
    Box::new(response)
}
