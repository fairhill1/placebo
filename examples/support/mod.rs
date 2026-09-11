use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

pub fn asset(name: &str, mime: &'static str, embedded: &'static str) -> Response {
    #[cfg(all(feature = "dev", debug_assertions))]
    {
        let _ = embedded;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/static")
            .join(name);
        match std::fs::read(path) {
            Ok(bytes) => (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                bytes,
            )
                .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read development asset.",
            )
                .into_response(),
        }
    }
    #[cfg(not(all(feature = "dev", debug_assertions)))]
    {
        let _ = (name, StatusCode::OK);
        ([(header::CONTENT_TYPE, mime)], embedded).into_response()
    }
}

#[cfg(all(feature = "dev", debug_assertions))]
pub fn reload() -> placebo::dev::DevReload {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    placebo::dev::watch([root.join("examples/static"), root.join("client")])
        .expect("watch development assets")
}
