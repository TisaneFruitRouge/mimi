//! The web interface: static files, login links and browser sessions.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use mimi_protocol::WebLoginLink;
use rust_embed::RustEmbed;

use super::Auth;
use super::error::{ApiResult, AppError};
use crate::{AppState, web};

/// The built frontend. Read from disk in debug builds, embedded in release builds.
/// `allow_missing` keeps the daemon compiling before `pnpm build` has run.
#[derive(RustEmbed)]
#[folder = "../../apps/desktop/dist"]
#[allow_missing = true]
struct Assets;

/// Mints a login link. Only native clients (bearer token) may do this, so a browser
/// session can't extend itself.
pub async fn login_link(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<Auth>,
) -> ApiResult<WebLoginLink> {
    if auth != Auth::Bearer {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Login links can only be created by the desktop app or the CLI.",
        ));
    }
    let code = state.login_codes.issue().map_err(AppError::internal)?;
    Ok(Json(WebLoginLink {
        url: format!("http://127.0.0.1:{}/login?code={code}", state.port),
    }))
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    req: Request,
) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| web::allowed_host(h, state.port));
    let code_ok = query
        .get("code")
        .is_some_and(|code| state.login_codes.redeem(code));
    if !host_ok || !code_ok {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            page(
                "This link has expired",
                "Login links work once and only for two minutes. Open Mimi in your browser \
                 again from the desktop app, or run <code>mimi open</code>.",
            ),
        )
            .into_response();
    }
    let token = match web::create_session(&state.db).await {
        Ok(token) => token,
        Err(e) => return AppError::internal(e).into_response(),
    };
    let mut res = Redirect::to("/").into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&web::session_cookie(&token)).expect("cookie is ASCII"),
    );
    res
}

pub async fn logout(State(state): State<Arc<AppState>>, req: Request) -> Response {
    if let Some(token) = session_token(&req) {
        let _ = web::delete_session(&state.db, token).await;
    }
    ([(header::SET_COOKIE, web::cleared_cookie())], Json(())).into_response()
}

pub fn session_token(req: &Request) -> Option<&str> {
    let header = req.headers().get(header::COOKIE)?.to_str().ok()?;
    web::cookie(header, web::SESSION_COOKIE)
}

/// Serves the frontend, falling back to `index.html` for client-side routes.
pub async fn static_files(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "v1" || path.starts_with("v1/") {
        return AppError::not_found("Route").into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if let Some(file) = Assets::get(path).filter(|_| !path.is_empty()) {
        // Vite fingerprints everything under assets/, so it can be cached forever.
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
            file.data,
        )
            .into_response();
    }
    let csp = format!(
        "default-src 'self'; connect-src 'self' ws://127.0.0.1:{port} ws://localhost:{port}; \
         img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'",
        port = state.port
    );
    let headers = [
        (header::CONTENT_TYPE, "text/html; charset=utf-8".to_owned()),
        (header::CACHE_CONTROL, "no-cache".to_owned()),
        (header::CONTENT_SECURITY_POLICY, csp),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        (header::REFERRER_POLICY, "no-referrer".to_owned()),
    ];
    match Assets::get("index.html") {
        Some(index) => (headers, index.data).into_response(),
        None => (
            headers,
            page(
                "The web interface isn't built",
                "Run <code>pnpm build</code> (or <code>pnpm web</code>) in the repository, \
                 then reload this page.",
            ),
        )
            .into_response(),
    }
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Mimi</title><style>body{{font-family:system-ui,sans-serif;max-width:32rem;\
         margin:20vh auto;padding:0 1.5rem;line-height:1.5;color:#222}}\
         @media(prefers-color-scheme:dark){{body{{background:#111;color:#ddd}}}}\
         code{{background:#8882;padding:.1em .3em;border-radius:4px}}</style></head>\
         <body><h1>{title}</h1><p>{body}</p></body></html>"
    )
}
