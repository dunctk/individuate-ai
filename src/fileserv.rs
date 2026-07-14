use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;
use tower_http::services::ServeDir;

pub async fn static_file_handler(uri: Uri) -> Response {
    let site_root = std::env::var("LEPTOS_SITE_ROOT").unwrap_or_else(|_| "target/site".to_string());
    let res = get_static_file(uri.clone(), &site_root).await.unwrap();
    if res.status() == StatusCode::OK {
        return res;
    }
    if let Some(fallback) = fallback_css(&uri).await {
        return fallback.into_response();
    }
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

async fn get_static_file(uri: Uri, root: &str) -> Result<Response<Body>, (StatusCode, String)> {
    let req = axum::http::Request::builder()
        .uri(uri.clone())
        .body(Body::empty())
        .unwrap();
    match ServeDir::new(root).oneshot(req).await {
        Ok(res) => Ok(res.into_response()),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", err),
        )),
    }
}

async fn fallback_css(uri: &Uri) -> Option<Response<Body>> {
    if uri.path() != "/pkg/individuateai.css" {
        return None;
    }
    let css = tokio::fs::read("style/output.css").await.ok()?;
    let mut response = Response::new(Body::from(css));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/css; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    Some(response)
}
