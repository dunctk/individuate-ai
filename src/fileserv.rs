use axum::response::Response as AxumResponse;
use axum::{
    body::Body,
    extract::State,
    http::{header, Request, Response, StatusCode, Uri},
    response::IntoResponse,
};
use leptos::LeptosOptions;
use tower::ServiceExt;
use tower_http::services::ServeDir;

pub async fn file_and_error_handler(
    uri: Uri,
    State(options): State<LeptosOptions>,
    req: Request<Body>,
) -> AxumResponse {
    let root = options.site_root.clone();
    let res = get_static_file(uri.clone(), &root).await.unwrap();

    if res.status() == StatusCode::OK {
        res.into_response()
    } else if let Some(fallback) = fallback_css(&uri).await {
        fallback.into_response()
    } else {
        let handler = leptos_axum::render_app_to_stream(options, crate::app::App);
        handler(req).await.into_response()
    }
}

async fn get_static_file(uri: Uri, root: &str) -> Result<Response<Body>, (StatusCode, String)> {
    let req = Request::builder()
        .uri(uri.clone())
        .body(Body::empty())
        .unwrap();
    // `ServeDir` implements `tower::Service`
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
    Some(response)
}
