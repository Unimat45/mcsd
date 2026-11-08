//! Serves the dashboard (SvelteKit static build) embedded in the binary.
//! Debug builds read `web/build` from disk at runtime, so a frontend rebuild
//! does not require recompiling the daemon.

use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};

#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/build/"]
struct Assets;

const PLACEHOLDER: &str = r#"<!doctype html><meta charset="utf-8"><title>mcsd</title>
<body style="font-family:system-ui;background:#0b0f14;color:#e6edf3;padding:3rem">
<h1>mcsd daemon is running</h1>
<p>The dashboard was not built into this binary. Build it with:</p>
<pre style="background:#161b22;padding:1rem;border-radius:8px">cd web &amp;&amp; npm install &amp;&amp; npm run build
cargo build --release</pre>
<p>The API is available under <code>/api</code>.</p></body>"#;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(resp) = asset(if path.is_empty() { "index.html" } else { path }) {
        return resp;
    }
    // SPA fallback: client-side routes like /servers/foo.
    asset("index.html").unwrap_or_else(|| (StatusCode::OK, Html(PLACEHOLDER)).into_response())
}

fn asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(
        (
            [
                (header::CONTENT_TYPE, mime.as_ref().to_string()),
                (header::CACHE_CONTROL, cache.to_string()),
            ],
            file.data,
        )
            .into_response(),
    )
}
