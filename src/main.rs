use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, Key, PrivateCookieJar, SameSite};
use base64::Engine;
use dashmap::DashMap;
use individuateai::agent::{
    self, agent_runtime, cookie_key, draft_stream_handler, graph_handler, has_auth_cookie,
    is_supported_tts_voice, stream_handler, AuthSession, MemoryEdit, RelationshipProfile, User,
    DEFAULT_TTS_VOICE,
};
use individuateai::fileserv;
use individuateai::templates;
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEEPGRAM_EU_API_BASE: &str = "https://api.eu.deepgram.com";
const DEEPGRAM_MIP_OPT_OUT: &str = "true";

fn deepgram_eu_endpoint(path: &str) -> String {
    format!("{DEEPGRAM_EU_API_BASE}{path}")
}

#[derive(Clone)]
struct AppState {
    key: Key,
    templates: Arc<Environment<'static>>,
    rate_limiter: RateLimiter,
    speech_rate_limiter: RateLimiter,
}

impl axum::extract::FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.key.clone()
    }
}

#[derive(Clone)]
struct RateLimiter {
    attempts: Arc<DashMap<String, Vec<Instant>>>,
    max_attempts: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(max_attempts: usize, window_secs: u64) -> Self {
        Self {
            attempts: Arc::new(DashMap::new()),
            max_attempts,
            window: Duration::from_secs(window_secs),
        }
    }

    fn check(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut entry = self
            .attempts
            .entry(key.to_string())
            .or_insert_with(Vec::new);
        entry.retain(|t| now.duration_since(*t) < self.window);
        if entry.len() >= self.max_attempts {
            return false;
        }
        entry.push(now);
        true
    }
}

fn configured_tts_voice() -> String {
    std::env::var("DEEPGRAM_TTS_MODEL")
        .ok()
        .filter(|voice| is_supported_tts_voice(voice))
        .unwrap_or_else(|| DEFAULT_TTS_VOICE.to_string())
}

async fn resolve_user_tts_voice(user_id: &str) -> String {
    let stored = match agent_runtime().await {
        Ok(runtime) => runtime
            .get_tts_voice(user_id.to_string())
            .await
            .ok()
            .flatten(),
        Err(_) => None,
    };
    stored
        .filter(|voice| is_supported_tts_voice(voice))
        .unwrap_or_else(configured_tts_voice)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();
    let key = cookie_key();
    let env = Arc::new(templates::create_env());

    let state = AppState {
        key: key.clone(),
        templates: env.clone(),
        rate_limiter: RateLimiter::new(10, 60), // 10 attempts per 60s window
        speech_rate_limiter: RateLimiter::new(90, 60),
    };

    let rate_limited_routes = Router::new()
        .route("/api/recovery/login", post(recovery_login_handler))
        .route("/api/recovery/rotate/start", post(recovery_rotate_start))
        .route(
            "/api/recovery/rotate/confirm",
            post(recovery_rotate_confirm),
        )
        .route("/api/passkey/register/start", post(passkey_register_start))
        .route(
            "/api/passkey/register/complete",
            post(passkey_register_complete),
        )
        .route("/api/passkey/revoke", post(passkey_revoke_handler))
        .route("/api/passkey/login/start", post(passkey_login_start))
        .route("/api/passkey/login/complete", post(passkey_login_complete))
        .route("/api/passkey/sync/start", post(passkey_sync_start))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_auth,
        ));

    let app = Router::new()
        // Pages
        .route("/", get(landing_page))
        .route("/privacy-and-security", get(privacy_security_page))
        .route("/chat", get(home_page))
        .route("/login", get(login_page))
        .route("/recovery", get(recovery_page))
        .route("/signup", get(signup_page))
        .route("/mind-map", get(mind_map_page))
        .route("/social-graph", get(social_graph_page))
        // Fragments
        .route("/fragments/sidebar", get(sidebar_fragment))
        .route("/fragments/chat/:session_id", get(chat_fragment))
        .route("/fragments/profile-drawer", get(profile_drawer_fragment))
        // API (non-rate-limited)
        .route("/api/logout", get(logout_handler))
        .route("/api/whoami", get(whoami_handler))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/:id/history", get(chat_history))
        .route("/api/profiles", get(list_profiles))
        .route("/api/profiles/:slug", post(save_profile))
        .route(
            "/api/settings/voice",
            get(get_voice_setting).post(save_voice_setting),
        )
        .route("/api/social-graph", get(get_social_graph))
        .route("/api/episodes", get(get_episodes))
        .route("/api/memory-status", get(memory_status))
        .route(
            "/api/memories/:kind/:id",
            get(get_editable_memory).post(update_editable_memory),
        )
        .route("/api/deepgram/token", post(deepgram_token_handler))
        .route("/api/transcribe", post(transcribe_handler))
        .route("/api/speak", post(speak_handler))
        .route("/api/chat", post(chat_handler))
        // SSE streams
        .route("/api/agent-stream", get(stream_handler))
        .route("/api/draft-stream", get(draft_stream_handler))
        .route("/api/graph/:user_id", get(graph_handler))
        // Static
        .route("/pkg/*path", get(fileserv::static_file_handler))
        .route("/icons/*path", get(fileserv::static_file_handler))
        .route("/passkey.js", get(passkey_js_handler))
        .route("/:filename", get(static_asset_handler))
        // Rate-limited auth routes
        .merge(rate_limited_routes)
        // Auth middleware
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_guard))
        .with_state(state)
        .layer(middleware::from_fn(add_app_search_headers));

    let port = std::env::var("PORT").unwrap_or_else(|_| "3008".to_string());
    let addr = format!("0.0.0.0:{}", port);
    println!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

// --- Auth Middleware ---

async fn add_app_search_headers(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        header::HeaderName::from_static("x-robots-tag"),
        HeaderValue::from_static("noindex, nofollow, noarchive"),
    );
    response
}

async fn auth_guard(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().trim_end_matches('/');
    let protected = path == "/chat"
        || path == "/mind-map"
        || path == "/social-graph"
        || path.starts_with("/fragments")
        || path.starts_with("/api/sessions")
        || path.starts_with("/api/profiles")
        || path.starts_with("/api/chat");
    let is_api = path.starts_with("/api/")
        && !path.contains("/login")
        && !path.contains("/signup")
        && !path.contains("/passkey/login")
        && !path.contains("/passkey/register")
        && !path.contains("recovery/login");

    if (protected || is_api) && !has_auth_cookie(req.headers(), &state.key) {
        if path.starts_with("/api/") {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response();
        }
        return Redirect::temporary("/login").into_response();
    }
    next.run(req).await
}

// --- Rate Limiting Middleware ---

async fn rate_limit_auth(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    if !state.rate_limiter.check(&ip) {
        tracing::warn!("Rate limit hit for IP: {}", ip);
        let is_api = req.uri().path().starts_with("/api/");
        if is_api {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"error": "Too many requests. Please try again later."})),
            )
                .into_response();
        }
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests. Please try again later.",
        )
            .into_response();
    }
    next.run(req).await
}

// --- Cookie helpers ---

fn set_auth_cookie(_key: &Key, session: &AuthSession, is_secure: bool) -> Cookie<'static> {
    let value = serde_json::to_string(session).expect("auth session serializes");
    Cookie::build((agent::AUTH_COOKIE_NAME, value))
        .path("/")
        .secure(is_secure)
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(365))
        .build()
}

fn remove_auth_cookie(_key: &Key, is_secure: bool) -> Cookie<'static> {
    let mut c = Cookie::build((agent::AUTH_COOKIE_NAME, ""))
        .path("/")
        .secure(is_secure)
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(0))
        .build();
    c.make_removal();
    c
}

fn extract_auth_session(jar: &PrivateCookieJar) -> Option<AuthSession> {
    jar.get(agent::AUTH_COOKIE_NAME)
        .and_then(|c| serde_json::from_str::<AuthSession>(c.value()).ok())
        .filter(|session| session.dek.len() == individuateai::security::DEK_LEN)
}

fn cookie_is_secure(headers: &HeaderMap) -> bool {
    agent::cookie_is_secure(headers)
}

async fn get_authed_user(headers: &HeaderMap, key: &Key) -> Option<User> {
    let jar = PrivateCookieJar::from_headers(headers, key.clone());
    let session = extract_auth_session(&jar)?;
    let runtime = agent_runtime().await.ok()?;
    runtime.cache_dek(&session.user_id, session.dek).ok()?;
    runtime.migrate_user_content(&session.user_id).await.ok()?;
    runtime.get_user(session.user_id).await.ok()
}

// --- Page Handlers ---

async fn landing_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_landing(&state.templates);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

async fn home_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return Redirect::temporary("/login").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let mut messages = Vec::new();
    let session_id = if let Some(id) = params.get("session").filter(|id| !id.trim().is_empty()) {
        match runtime
            .get_session_history(user.id.clone(), id.to_string())
            .await
        {
            Ok(history) => {
                messages = history;
                id.as_str()
            }
            Err(_) => "",
        }
    } else {
        ""
    };
    let html = templates::render_home(&state.templates, &user, session_id, &messages);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn login_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_login(&state.templates);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

async fn privacy_security_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_privacy_security(&state.templates);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

async fn recovery_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_recovery(&state.templates);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

async fn signup_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_signup(&state.templates);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

async fn mind_map_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return Redirect::temporary("/login").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let graph = match runtime.get_mind_map_payload(user.id.clone()).await {
        Ok(graph) => graph,
        Err(error) => {
            tracing::error!("Could not load mind map: {error}");
            serde_json::json!({
                "nodes": [],
                "edges": [],
                "people": [],
                "episodes": [],
                "cross_edges": [],
                "load_error": "The mind map could not load. Refresh to try again."
            })
        }
    };
    let html = templates::render_mind_map(&state.templates, &graph, &user.id);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn social_graph_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return Redirect::temporary("/login").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let graph = runtime
        .get_social_graph(user.id.clone())
        .await
        .unwrap_or_default();
    let html = templates::render_social_graph(&state.templates, &graph, &user.id);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

// --- Fragment Handlers ---

async fn sidebar_fragment(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let sessions = runtime
        .list_sessions(user.id.clone())
        .await
        .unwrap_or_default();
    let html = templates::render_sidebar(&state.templates, &sessions, &user);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn chat_fragment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let messages = runtime
        .get_session_history(user.id, session_id)
        .await
        .unwrap_or_default();
    let html = templates::render_chat_messages(&state.templates, &messages);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn profile_drawer_fragment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let profiles = runtime
        .get_relationship_profiles(user.id.clone())
        .await
        .unwrap_or_default();
    let selected_slug = params
        .get("slug")
        .map(|s| s.as_str())
        .or_else(|| profiles.first().map(|profile| profile.slug.as_str()))
        .unwrap_or("");
    let selected_voice = runtime
        .get_tts_voice(user.id)
        .await
        .ok()
        .flatten()
        .filter(|voice| is_supported_tts_voice(voice))
        .unwrap_or_else(configured_tts_voice);
    let html = templates::render_profile_drawer(
        &state.templates,
        &profiles,
        selected_slug,
        &selected_voice,
    );
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

// --- API Handlers ---

async fn logout_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let jar = PrivateCookieJar::from_headers(&headers, state.key.clone());
    if let Some(session) = extract_auth_session(&jar) {
        if let Ok(runtime) = agent_runtime().await {
            runtime.forget_dek(&session.user_id);
        }
    }
    let is_secure = cookie_is_secure(&headers);
    let cookie = remove_auth_cookie(&state.key, is_secure);
    let mut jar = cookie::CookieJar::new();
    jar.private_mut(&state.key).add(cookie);
    let mut resp = Redirect::to("/login").into_response();
    if let Some(h) = jar.delta().last() {
        resp.headers_mut().insert(
            header::SET_COOKIE,
            header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
        );
    }
    resp
}

#[derive(Deserialize)]
struct RecoveryLoginPayload {
    email: String,
    recovery_key: String,
}

async fn recovery_login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RecoveryLoginPayload>,
) -> Response {
    let runtime = match agent_runtime().await {
        Ok(runtime) => runtime,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };
    match runtime
        .login_with_recovery(payload.email, payload.recovery_key)
        .await
    {
        Ok((user, dek)) => {
            let session = AuthSession {
                user_id: user.id,
                dek,
            };
            let cookie = set_auth_cookie(&state.key, &session, cookie_is_secure(&headers));
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut response = Json(serde_json::json!({"redirect": "/chat"})).into_response();
            if let Some(header) = jar.delta().last() {
                response.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&header.encoded().to_string()).unwrap(),
                );
            }
            response
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid recovery credentials"})),
        )
            .into_response(),
    }
}

async fn whoami_handler(State(state): State<AppState>, headers: HeaderMap) -> Json<Option<User>> {
    let user = get_authed_user(&headers, &state.key).await;
    Json(user)
}

#[derive(Deserialize)]
struct CreateSessionPayload {
    title: Option<String>,
}

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateSessionPayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let title = payload.title.unwrap_or_else(|| "New Session".to_string());
    let runtime = agent_runtime().await.unwrap();
    match runtime.create_new_session(user.id, title).await {
        Ok(session) => Json(session).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_sessions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.list_sessions(user.id).await {
        Ok(sessions) => Json(sessions).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn chat_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_session_history(user.id, id).await {
        Ok(messages) => Json(messages).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_profiles(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_relationship_profiles(user.id).await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_social_graph(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_social_graph(user.id).await {
        Ok(graph) => Json(graph).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_episodes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_episodes_with_links(user.id).await {
        Ok(episodes) => Json(episodes).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_editable_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = match agent_runtime().await {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!("Could not open memory store: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Could not load memory"})),
            )
                .into_response();
        }
    };
    match runtime.get_editable_memory(user.id, kind, id).await {
        Ok(Some(memory)) => Json(memory).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Memory not found"})),
        )
            .into_response(),
        Err(error) => {
            tracing::error!("Could not load editable memory: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Could not load memory"})),
            )
                .into_response()
        }
    }
}

async fn update_editable_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kind, id)): Path<(String, String)>,
    Json(payload): Json<MemoryEdit>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = match agent_runtime().await {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!("Could not open memory store: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Could not save memory"})),
            )
                .into_response();
        }
    };
    match runtime
        .update_editable_memory(user.id, kind, id, payload)
        .await
    {
        Ok(Some(memory)) => Json(memory).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Memory not found"})),
        )
            .into_response(),
        Err(error) => {
            let message = error.to_string();
            let validation_error = message.starts_with("Memory ")
                || message.starts_with("Unsupported memory category");
            if !validation_error {
                tracing::error!("Could not save editable memory: {error}");
            }
            (
                if validation_error {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                },
                Json(serde_json::json!({
                    "error": if validation_error { message } else { "Could not save memory".to_string() }
                })),
            )
                .into_response()
        }
    }
}

async fn memory_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_memory_status(user.id).await {
        Ok(status) => Json(status).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn transcribe_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if get_authed_user(&headers, &state.key).await.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Unauthorized"})),
        )
            .into_response();
    }

    const MAX_AUDIO_BYTES: usize = 10 * 1024 * 1024;
    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Audio is empty"})),
        )
            .into_response();
    }
    if body.len() > MAX_AUDIO_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Recording is too large"})),
        )
            .into_response();
    }

    let api_key = match std::env::var("DEEPGRAM_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::error!("DEEPGRAM_API_KEY is not configured");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Voice transcription is unavailable"})),
            )
                .into_response();
        }
    };

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("audio/"))
        .unwrap_or("audio/webm");

    let response = match reqwest::Client::new()
        .post(deepgram_eu_endpoint("/v1/listen"))
        .query(&[
            ("model", "nova-3"),
            ("smart_format", "true"),
            ("punctuate", "true"),
            ("mip_opt_out", DEEPGRAM_MIP_OPT_OUT),
        ])
        .header("Authorization", format!("Token {api_key}"))
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!("Deepgram request failed: {error}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Voice transcription failed"})),
            )
                .into_response();
        }
    };

    if !response.status().is_success() {
        tracing::warn!("Deepgram returned status {}", response.status());
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "Voice transcription failed"})),
        )
            .into_response();
    }

    let payload: serde_json::Value = match response.json().await {
        Ok(payload) => payload,
        Err(error) => {
            tracing::error!("Invalid Deepgram response: {error}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Invalid transcription response"})),
            )
                .into_response();
        }
    };
    let transcript = payload["results"]["channels"][0]["alternatives"][0]["transcript"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    Json(serde_json::json!({"transcript": transcript})).into_response()
}

async fn deepgram_token_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };

    let api_key = match std::env::var("DEEPGRAM_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::error!("DEEPGRAM_API_KEY is not configured");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Voice transcription is unavailable"})),
            )
                .into_response();
        }
    };

    let response = match reqwest::Client::new()
        .post("https://api.deepgram.com/v1/auth/grant")
        .header("Authorization", format!("Token {api_key}"))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!("Deepgram token request failed: {error}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Live voice transcription is unavailable"})),
            )
                .into_response();
        }
    };

    if !response.status().is_success() {
        tracing::warn!(
            "Deepgram token request returned status {}",
            response.status()
        );
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "Live voice transcription is unavailable"})),
        )
            .into_response();
    }

    let tts_model = resolve_user_tts_voice(&user.id).await;
    let tts_speed = std::env::var("DEEPGRAM_TTS_SPEED")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| (0.7..=1.5).contains(value))
        .unwrap_or(1.0);

    match response.json::<serde_json::Value>().await {
        Ok(payload) if payload.get("access_token").and_then(|value| value.as_str()).is_some() => {
            (
                [(header::CACHE_CONTROL, "no-store")],
                Json(serde_json::json!({
                    "access_token": payload["access_token"],
                    "expires_in": payload.get("expires_in").cloned().unwrap_or_else(|| serde_json::json!(30)),
                    "tts_model": tts_model,
                    "tts_speed": tts_speed,
                })),
            )
                .into_response()
        }
        Ok(_) => {
            tracing::warn!("Deepgram token response was missing an access token");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Live voice transcription is unavailable"})),
            )
                .into_response()
        }
        Err(error) => {
            tracing::error!("Failed to parse Deepgram token response: {error}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Live voice transcription is unavailable"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct SpeakPayload {
    text: String,
}

async fn speak_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SpeakPayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    if !state.speech_rate_limiter.check(&user.id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "Too many speech requests. Please wait a moment."})),
        )
            .into_response();
    }

    let text = payload.text.trim();
    if text.is_empty() || text.chars().count() > 2_000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Speech text must be between 1 and 2000 characters"})),
        )
            .into_response();
    }

    let api_key = match std::env::var("DEEPGRAM_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::error!("DEEPGRAM_API_KEY is not configured");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Spoken responses are unavailable"})),
            )
                .into_response();
        }
    };
    let model = resolve_user_tts_voice(&user.id).await;
    let speed = std::env::var("DEEPGRAM_TTS_SPEED")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| (0.7..=1.5).contains(value))
        .unwrap_or(1.0)
        .to_string();

    let response = match reqwest::Client::new()
        .post(deepgram_eu_endpoint("/v1/speak"))
        .query(&[
            ("model", model.as_str()),
            ("encoding", "mp3"),
            ("bit_rate", "32000"),
            ("speed", speed.as_str()),
            ("mip_opt_out", DEEPGRAM_MIP_OPT_OUT),
        ])
        .header("Authorization", format!("Token {api_key}"))
        .json(&serde_json::json!({"text": text}))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!("Deepgram speech request failed: {error}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Could not create spoken response"})),
            )
                .into_response();
        }
    };

    if !response.status().is_success() {
        tracing::warn!(
            "Deepgram speech request returned status {}",
            response.status()
        );
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "Could not create spoken response"})),
        )
            .into_response();
    }

    match response.bytes().await {
        Ok(audio) => (
            [
                (header::CONTENT_TYPE, "audio/mpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            audio,
        )
            .into_response(),
        Err(error) => {
            tracing::error!("Could not read Deepgram speech response: {error}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "Could not create spoken response"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct SaveProfilePayload {
    display_name: String,
    relationship_type: String,
    background: String,
    goals: Vec<String>,
    triggers: Vec<String>,
    do_not_say: Vec<String>,
    effective_tone: Vec<String>,
    recent_events: Vec<String>,
    boundaries: Vec<String>,
}

#[derive(Deserialize)]
struct VoiceSettingPayload {
    voice: String,
}

async fn get_voice_setting(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    Json(serde_json::json!({"voice": resolve_user_tts_voice(&user.id).await})).into_response()
}

async fn save_voice_setting(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<VoiceSettingPayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    if !is_supported_tts_voice(&payload.voice) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Unsupported text-to-speech voice"})),
        )
            .into_response();
    }

    let runtime = match agent_runtime().await {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!("Could not open settings store: {error}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Could not save voice setting"})),
            )
                .into_response();
        }
    };
    match runtime.set_tts_voice(user.id, payload.voice.clone()).await {
        Ok(()) => Json(serde_json::json!({"voice": payload.voice})).into_response(),
        Err(error) => {
            tracing::error!("Could not save voice setting: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Could not save voice setting"})),
            )
                .into_response()
        }
    }
}

async fn save_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(payload): Json<SaveProfilePayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();
    let profile = RelationshipProfile {
        user_id: user.id,
        slug,
        display_name: payload.display_name,
        relationship_type: payload.relationship_type,
        background: payload.background,
        goals: payload.goals,
        triggers: payload.triggers,
        do_not_say: payload.do_not_say,
        effective_tone: payload.effective_tone,
        recent_events: payload.recent_events,
        boundaries: payload.boundaries,
    };
    match runtime.save_relationship_profile(profile).await {
        Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// --- Chat Handler ---

#[derive(Deserialize)]
struct ChatPayload {
    session_id: Option<String>,
    message: String,
    mode: Option<String>,
}

async fn chat_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ChatPayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            )
                .into_response()
        }
    };
    let runtime = agent_runtime().await.unwrap();

    // Use existing session or create new one
    let session_id = match payload.session_id {
        Some(ref id) if !id.is_empty() => id.clone(),
        _ => match runtime
            .create_new_session(user.id.clone(), "New Session".into())
            .await
        {
            Ok(s) => s.id,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response()
            }
        },
    };

    let mode = payload.mode.as_deref().unwrap_or("therapist");
    let result = if mode == "draft" {
        runtime
            .draft_message(
                &user.id,
                &session_id,
                "default".into(),
                "general".into(),
                payload.message,
                50,
                50,
                50,
            )
            .await
    } else {
        runtime
            .respond(&user.id, &session_id, payload.message)
            .await
    };

    match result {
        Ok(response) => Json(serde_json::json!({"session_id": session_id, "response": response}))
            .into_response(),
        Err(e) => {
            tracing::error!("Chat handler error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    }
}

// --- Passkey Handlers ---

#[derive(Deserialize)]
struct PasskeyEmailPayload {
    email: String,
}

#[derive(Serialize)]
struct PasskeyStartResponse {
    challenge_id: String,
    options: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_key: Option<String>,
}

async fn passkey_register_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyEmailPayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    let can_register = match get_authed_user(&headers, &state.key).await {
        Some(user) => runtime.start_passkey_registration(user.id).await,
        None => {
            runtime
                .start_passkey_registration_email(payload.email)
                .await
        }
    };
    match can_register {
        Ok(start) => {
            let mut options = serde_json::to_value(&start.challenge).unwrap_or_default();
            options["publicKey"]["authenticatorSelection"]["residentKey"] =
                serde_json::json!("required");
            options["publicKey"]["authenticatorSelection"]["requireResidentKey"] =
                serde_json::json!(true);
            options["publicKey"]["extensions"] = serde_json::json!({
                "prf": { "eval": { "first": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&start.prf_salt) } },
                "largeBlob": { "support": "preferred" }
            });
            Json(PasskeyStartResponse {
                challenge_id: start.challenge_id,
                options,
                recovery_key: start.recovery_key,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct PasskeyCompletePayload {
    challenge_id: String,
    credential: serde_json::Value,
    prf_output: Option<String>,
    prf_enabled: Option<bool>,
    large_blob: Option<String>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    confirm_recovery: bool,
}

async fn passkey_register_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyCompletePayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    if payload.prf_enabled != Some(true) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "This authenticator does not support the required PRF extension"}))).into_response();
    }
    let response: webauthn_rs_proto::RegisterPublicKeyCredential =
        match serde_json::from_value(payload.credential) {
            Ok(response) => response,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid passkey response"})),
                )
                    .into_response()
            }
        };
    let prf_output = match payload.prf_output.and_then(|value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .ok()
    }) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing PRF output"})),
            )
                .into_response()
        }
    };
    match runtime
        .finish_passkey_registration(payload.challenge_id, response, prf_output, payload.label)
        .await
    {
        Ok((user, dek, recovery_key)) => {
            let is_secure = cookie_is_secure(&headers);
            let session = AuthSession {
                user_id: user.id,
                dek,
            };
            let cookie = set_auth_cookie(&state.key, &session, is_secure);
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut resp =
                Json(serde_json::json!({"redirect": "/chat", "recovery_key": recovery_key}))
                    .into_response();
            if let Some(h) = jar.delta().last() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
                );
            }
            resp
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn passkey_login_start() -> Response {
    let runtime = agent_runtime().await.unwrap();
    match runtime.start_passkey_login().await {
        Ok((req_id, challenge, prf_salts)) => {
            let mut options = serde_json::to_value(&challenge).unwrap_or_default();
            // webauthn-rs uses this flow for conditional autofill. This login is
            // explicitly button-triggered, so request the normal account picker.
            if let Some(object) = options.as_object_mut() {
                object.remove("mediation");
            }
            let eval_by_credential: serde_json::Map<String, serde_json::Value> = prf_salts
                .into_iter()
                .map(|(credential_id, salt)| (
                    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(credential_id),
                    serde_json::json!({"first": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(salt)}),
                ))
                .collect();
            options["publicKey"]["extensions"] = serde_json::json!({
                "prf": { "evalByCredential": eval_by_credential },
                "largeBlob": { "read": true }
            });
            Json(PasskeyStartResponse {
                challenge_id: req_id,
                options,
                recovery_key: None,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn passkey_sync_start(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let jar = PrivateCookieJar::from_headers(&headers, state.key.clone());
    let session = match extract_auth_session(&jar) {
        Some(session) => session,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime
        .start_passkey_sync(session.user_id, session.dek.clone())
        .await
    {
        Ok((req_id, challenge, credential_id, prf_salt)) => {
            let mut options = serde_json::to_value(&challenge).unwrap_or_default();
            if let Some(object) = options.as_object_mut() {
                object.remove("mediation");
            }
            options["publicKey"]["allowCredentials"] = serde_json::json!([{
                "type": "public-key",
                "id": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(credential_id)
            }]);
            options["publicKey"]["extensions"] = serde_json::json!({
                "prf": { "eval": { "first": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(prf_salt) } },
                "largeBlob": { "write": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(session.dek) }
            });
            Json(PasskeyStartResponse {
                challenge_id: req_id,
                options,
                recovery_key: None,
            })
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn passkey_login_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyCompletePayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    let recovery_session = {
        let jar = PrivateCookieJar::from_headers(&headers, state.key.clone());
        extract_auth_session(&jar).map(|session| (session.user_id, session.dek))
    };
    let response: webauthn_rs_proto::PublicKeyCredential =
        match serde_json::from_value(payload.credential) {
            Ok(response) => response,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid passkey response"})),
                )
                    .into_response()
            }
        };
    let prf_output = match payload.prf_output.and_then(|value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .ok()
    }) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing PRF output"})),
            )
                .into_response()
        }
    };
    let large_blob = payload.large_blob.and_then(|value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .ok()
    });
    match runtime
        .finish_passkey_login(
            payload.challenge_id,
            response,
            prf_output,
            large_blob,
            recovery_session,
        )
        .await
    {
        Ok((user, dek)) => {
            let is_secure = cookie_is_secure(&headers);
            let session = AuthSession {
                user_id: user.id,
                dek,
            };
            let cookie = set_auth_cookie(&state.key, &session, is_secure);
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut resp = Json(serde_json::json!({"redirect": "/chat"})).into_response();
            if let Some(h) = jar.delta().last() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
                );
            }
            resp
        }
        Err(e) if e.to_string() == agent::SYNCED_PASSKEY_RECOVERY_REQUIRED => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "Apple synced this passkey, but this device uses a different encryption key. Use your recovery key once to authorize this device.",
                "recovery_required": true
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn recovery_rotate_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyCompletePayload>,
) -> Response {
    let jar = PrivateCookieJar::from_headers(&headers, state.key.clone());
    let current_session = match extract_auth_session(&jar) {
        Some(session) => session,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let response: webauthn_rs_proto::PublicKeyCredential =
        match serde_json::from_value(payload.credential) {
            Ok(response) => response,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid passkey response"})),
                )
                    .into_response()
            }
        };
    let prf_output = match payload.prf_output.and_then(|value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .ok()
    }) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing PRF output"})),
            )
                .into_response()
        }
    };
    let large_blob = payload.large_blob.and_then(|value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .ok()
    });
    let runtime = agent_runtime().await.unwrap();
    let (user, dek) = match runtime
        .finish_passkey_login(
            payload.challenge_id,
            response,
            prf_output,
            large_blob,
            Some((current_session.user_id.clone(), current_session.dek)),
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": error.to_string()})),
            )
                .into_response()
        }
    };
    if user.id != current_session.user_id {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Passkey does not match this account"})),
        )
            .into_response();
    }
    match runtime.begin_recovery_rotation(user.id, &dek) {
        Ok((rotation_id, recovery_key)) => Json(serde_json::json!({
            "rotation_id": rotation_id,
            "recovery_key": recovery_key
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct RecoveryRotateConfirmPayload {
    rotation_id: String,
}

async fn recovery_rotate_confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RecoveryRotateConfirmPayload>,
) -> Response {
    let jar = PrivateCookieJar::from_headers(&headers, state.key.clone());
    let session = match extract_auth_session(&jar) {
        Some(session) => session,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime
        .confirm_recovery_rotation(payload.rotation_id, &session.user_id)
        .await
    {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn passkey_revoke_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyCompletePayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };
    let response: webauthn_rs_proto::PublicKeyCredential =
        match serde_json::from_value(payload.credential) {
            Ok(response) => response,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid passkey response"})),
                )
                    .into_response()
            }
        };
    let prf_output = match payload.prf_output.and_then(|value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .ok()
    }) {
        Some(value) => value,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing PRF output"})),
            )
                .into_response()
        }
    };
    let runtime = match agent_runtime().await {
        Ok(runtime) => runtime,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };
    match runtime
        .revoke_passkey(
            payload.challenge_id,
            response,
            prf_output,
            &user.id,
            payload.confirm_recovery,
        )
        .await
    {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

// --- Static file handlers ---

async fn passkey_js_handler() -> impl IntoResponse {
    let js = include_str!("../public/passkey.js");
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        js,
    )
}

async fn static_asset_handler(Path(filename): Path<String>) -> Response {
    if filename == "mandala-avatar.mp4" {
        return match tokio::fs::read(&filename).await {
            Ok(video) => ([(header::CONTENT_TYPE, "video/mp4")], video).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
        };
    }
    if filename == "mandala-avatar.jpg" {
        return match tokio::fs::read(&filename).await {
            Ok(image) => ([(header::CONTENT_TYPE, "image/jpeg")], image).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
        };
    }
    let uri: Uri = format!("/{}", filename).parse().unwrap();
    fileserv::static_file_handler(uri).await
}
