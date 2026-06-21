use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, Key, PrivateCookieJar};
use dashmap::DashMap;
use individuateai::agent::{
    self, agent_runtime, cookie_key, has_auth_cookie, draft_stream_handler, graph_handler,
    stream_handler, RelationshipProfile, User,
};
use individuateai::fileserv;
use individuateai::templates;
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    key: Key,
    templates: Arc<Environment<'static>>,
    rate_limiter: RateLimiter,
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
        let mut entry = self.attempts.entry(key.to_string()).or_insert_with(Vec::new);
        entry.retain(|t| now.duration_since(*t) < self.window);
        if entry.len() >= self.max_attempts {
            return false;
        }
        entry.push(now);
        true
    }
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
    };

    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let rate_limited_routes = Router::new()
        .route("/api/login", post(login_handler))
        .route("/api/signup", post(signup_handler))
        .route("/api/forgot-password", post(forgot_password_handler))
        .route("/api/reset-password", post(reset_password_handler))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit_auth));

    let app = Router::new()
        // Pages
        .route("/", get(home_page))
        .route("/login", get(login_page))
        .route("/signup", get(signup_page))
        .route("/mind-map", get(mind_map_page))
        .route("/forgot-password", get(forgot_password_page))
        .route("/reset-password/{token}", get(reset_password_page))
        // Fragments
        .route("/fragments/sidebar", get(sidebar_fragment))
        .route("/fragments/chat/{session_id}", get(chat_fragment))
        .route("/fragments/profile-drawer", get(profile_drawer_fragment))
        // API (non-rate-limited)
        .route("/api/logout", get(logout_handler))
        .route("/api/whoami", get(whoami_handler))
        .route("/api/verify-email/{token}", get(verify_email_handler))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}/history", get(chat_history))
        .route("/api/profiles", get(list_profiles))
        .route("/api/profiles/{slug}", post(save_profile))
        .route("/api/chat", post(chat_handler))
        // SSE streams
        .route("/api/agent-stream", get(stream_handler))
        .route("/api/draft-stream", get(draft_stream_handler))
        .route("/api/graph/{user_id}", get(graph_handler))
        // Passkey
        .route("/api/passkey/register/start", post(passkey_register_start))
        .route("/api/passkey/register/complete", post(passkey_register_complete))
        .route("/api/passkey/login/start", post(passkey_login_start))
        .route("/api/passkey/login/complete", post(passkey_login_complete))
        // Static
        .route("/pkg/*path", get(fileserv::static_file_handler))
        .route("/passkey.js", get(passkey_js_handler))
        .route("/:filename", get(static_asset_handler))
        // Rate-limited auth routes
        .merge(rate_limited_routes)
        // CORS (outermost)
        .layer(cors)
        // Auth middleware
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_guard))
        .with_state(state);

    let addr = "0.0.0.0:3008";
    println!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app.into_make_service()).await.unwrap();
}

// --- Auth Middleware ---

async fn auth_guard(State(state): State<AppState>, req: axum::http::Request<axum::body::Body>, next: Next) -> Response {
    let path = req.uri().path().trim_end_matches('/');
    let protected = path.is_empty()
        || path == "/mind-map"
        || path.starts_with("/fragments")
        || path.starts_with("/api/sessions")
        || path.starts_with("/api/profiles")
        || path.starts_with("/api/chat");
    let is_api = path.starts_with("/api/") && !path.contains("/login")
        && !path.contains("/signup") && !path.contains("/passkey/login")
        && !path.contains("/passkey/register") && !path.contains("forgot-password")
        && !path.contains("reset-password") && !path.contains("verify-email");

    if (protected || is_api) && !has_auth_cookie(req.headers(), &state.key) {
        if path.starts_with("/api/") {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
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

fn set_auth_cookie(_key: &Key, user_id: &str, is_secure: bool) -> Cookie<'static> {
    Cookie::build((agent::AUTH_COOKIE_NAME, user_id.to_string()))
        .path("/")
        .secure(is_secure)
        .http_only(true)
        .max_age(time::Duration::days(30))
        .build()
}

fn remove_auth_cookie(_key: &Key, is_secure: bool) -> Cookie<'static> {
    let mut c = Cookie::build((agent::AUTH_COOKIE_NAME, ""))
        .path("/")
        .secure(is_secure)
        .http_only(true)
        .max_age(time::Duration::seconds(0))
        .build();
    c.make_removal();
    c
}

fn extract_user_id(jar: &PrivateCookieJar) -> Option<String> {
    jar.get(agent::AUTH_COOKIE_NAME).map(|c| c.value().to_string())
}

fn cookie_is_secure(headers: &HeaderMap) -> bool {
    agent::cookie_is_secure(headers)
}

async fn get_authed_user(headers: &HeaderMap, key: &Key) -> Option<User> {
    let jar = PrivateCookieJar::from_headers(headers, key.clone());
    let user_id = extract_user_id(&jar)?;
    let runtime = agent_runtime().await.ok()?;
    runtime.get_user(user_id).await.ok()
}

// --- Page Handlers ---

async fn home_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return Redirect::temporary("/login").into_response(),
    };
    let html = templates::render_home(&state.templates, &user, "");
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

async fn login_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_login(&state.templates);
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
    let graph = runtime.get_patient_graph(user.id.clone()).await.unwrap_or_default();
    let html = templates::render_mind_map(&state.templates, &graph, &user.id);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

// --- Fragment Handlers ---

async fn sidebar_fragment(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    let sessions = runtime.list_sessions(user.id.clone()).await.unwrap_or_default();
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
    let messages = runtime.get_session_history(user.id, session_id).await.unwrap_or_default();
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
    let slug = params.get("slug").map(|s| s.as_str()).unwrap_or("mother");
    let runtime = agent_runtime().await.unwrap();
    let profiles = runtime.get_relationship_profiles(user.id.clone()).await.unwrap_or_default();
    let html = templates::render_profile_drawer(&state.templates, &profiles, slug);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

// --- API Handlers ---

#[derive(Deserialize)]
struct LoginPayload { email: String, password: String }

async fn login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: axum::extract::Form<LoginPayload>,
) -> Response {
    let runtime = match agent_runtime().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    match runtime.login(payload.email.clone(), payload.password.clone()).await {
        Ok(user) => {
            let is_secure = cookie_is_secure(&headers);
            let cookie = set_auth_cookie(&state.key, &user.id, is_secure);
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut resp = Json(serde_json::json!(user)).into_response();
            if let Some(h) = jar.delta().last() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
                );
            }
            resp.headers_mut().insert(
                header::HeaderName::from_static("hx-redirect"),
                header::HeaderValue::from_static("/"),
            );
            resp
        }
        Err(_) => (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid credentials"}))).into_response(),
    }
}

#[derive(Deserialize)]
struct SignupPayload { email: String }

async fn signup_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SignupPayload>,
) -> Response {
    let runtime = match agent_runtime().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    let random_password = uuid::Uuid::new_v4().to_string();
    match runtime.signup(payload.email.clone(), random_password).await {
        Ok(user) => {
            // Generate email verification token
            if let Ok(verify_token) = runtime.generate_email_verification_token(&user.id).await {
                let verify_url = format!("{}/api/verify-email/{}",
                    std::env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3008".to_string()),
                    verify_token
                );
                tracing::info!("Email verification URL: {}", verify_url);
            }

            let is_secure = cookie_is_secure(&headers);
            let cookie = set_auth_cookie(&state.key, &user.id, is_secure);
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut resp = Json(serde_json::json!(user)).into_response();
            if let Some(h) = jar.delta().last() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
                );
            }
            resp
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// --- Password Reset Handlers ---

async fn forgot_password_page(State(state): State<AppState>) -> impl IntoResponse {
    let html = templates::render_forgot_password(&state.templates);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

async fn reset_password_page(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let html = templates::render_reset_password(&state.templates, &token);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

#[derive(Deserialize)]
struct ForgotPasswordPayload { email: String }

async fn forgot_password_handler(
    Json(payload): Json<ForgotPasswordPayload>,
) -> Response {
    let runtime = match agent_runtime().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    match runtime.generate_password_reset_token(&payload.email).await {
        Ok(token) => {
            let reset_url = format!("{}/reset-password/{}",
                std::env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3008".to_string()),
                token
            );
            tracing::info!("Password reset URL: {}", reset_url);
            Json(serde_json::json!({
                "ok": true,
                "message": "If an account exists with that email, a reset link has been sent.",
                "dev_reset_url": reset_url,
            })).into_response()
        }
        Err(e) => {
            // Don't reveal if email exists - always return success
            tracing::warn!("Password reset request failed: {}", e);
            Json(serde_json::json!({
                "ok": true,
                "message": "If an account exists with that email, a reset link has been sent.",
            })).into_response()
        }
    }
}

#[derive(Deserialize)]
struct ResetPasswordPayload { token: String, password: String }

async fn reset_password_handler(
    Json(payload): Json<ResetPasswordPayload>,
) -> Response {
    if payload.password.len() < 8 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Password must be at least 8 characters"}))).into_response();
    }
    let runtime = match agent_runtime().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    match runtime.verify_and_reset_password(&payload.token, &payload.password).await {
        Ok(_) => Json(serde_json::json!({"ok": true, "message": "Password reset successfully. You can now log in."})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// --- Email Verification Handler ---

async fn verify_email_handler(
    Path(token): Path<String>,
) -> Response {
    let runtime = match agent_runtime().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    match runtime.verify_email_with_token(&token).await {
        Ok(_) => Json(serde_json::json!({"ok": true, "message": "Email verified successfully."})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn logout_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
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

async fn whoami_handler(State(state): State<AppState>, headers: HeaderMap) -> Json<Option<User>> {
    let user = get_authed_user(&headers, &state.key).await;
    Json(user)
}

#[derive(Deserialize)]
struct CreateSessionPayload { title: Option<String> }

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateSessionPayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response(),
    };
    let title = payload.title.unwrap_or_else(|| "New Session".to_string());
    let runtime = agent_runtime().await.unwrap();
    match runtime.create_new_session(user.id, title).await {
        Ok(session) => Json(session).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_sessions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.list_sessions(user.id).await {
        Ok(sessions) => Json(sessions).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn chat_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_session_history(user.id, id).await {
        Ok(messages) => Json(messages).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_profiles(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response(),
    };
    let runtime = agent_runtime().await.unwrap();
    match runtime.get_relationship_profiles(user.id).await {
        Ok(profiles) => Json(profiles).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
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

async fn save_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(payload): Json<SaveProfilePayload>,
) -> Response {
    let user = match get_authed_user(&headers, &state.key).await {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response(),
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
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
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
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response(),
    };
    let runtime = agent_runtime().await.unwrap();

    // Use existing session or create new one
    let session_id = match payload.session_id {
        Some(ref id) if !id.is_empty() => id.clone(),
        _ => match runtime.create_new_session(user.id.clone(), "New Session".into()).await {
            Ok(s) => s.id,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        },
    };

    let mode = payload.mode.as_deref().unwrap_or("therapist");
    let result = if mode == "draft" {
        runtime.draft_message(&user.id, &session_id, "default".into(), "general".into(), payload.message, 50, 50, 50).await
    } else {
        runtime.respond(&user.id, &session_id, payload.message).await
    };

    match result {
        Ok(response) => Json(serde_json::json!({"session_id": session_id, "response": response})).into_response(),
        Err(e) => {
            tracing::error!("Chat handler error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

// --- Passkey Handlers ---

#[derive(Deserialize)]
struct PasskeyEmailPayload { email: String }

#[derive(Serialize)]
struct PasskeyStartResponse {
    challenge_id: String,
    options: serde_json::Value,
}

async fn passkey_register_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyEmailPayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    let can_register = match get_authed_user(&headers, &state.key).await {
        Some(user) => runtime.start_passkey_registration(user.id).await,
        None => runtime.start_passkey_registration_email(payload.email).await,
    };
    match can_register {
        Ok((req_id, challenge)) => {
            let options = serde_json::to_value(&challenge).unwrap_or_default();
            Json(PasskeyStartResponse { challenge_id: req_id, options }).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct PasskeyCompletePayload {
    challenge_id: String,
    credential: serde_json::Value,
}

async fn passkey_register_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyCompletePayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    let response: webauthn_rs_proto::RegisterPublicKeyCredential =
        serde_json::from_value(payload.credential).unwrap();
    match runtime.finish_passkey_registration(payload.challenge_id, response).await {
        Ok(user) => {
            let is_secure = cookie_is_secure(&headers);
            let cookie = set_auth_cookie(&state.key, &user.id, is_secure);
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut resp = Json(serde_json::json!({"redirect": "/"})).into_response();
            if let Some(h) = jar.delta().last() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
                );
            }
            resp
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn passkey_login_start(
    Json(payload): Json<PasskeyEmailPayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    match runtime.start_passkey_login(payload.email).await {
        Ok((req_id, challenge)) => {
            let options = serde_json::to_value(&challenge).unwrap_or_default();
            Json(PasskeyStartResponse { challenge_id: req_id, options }).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn passkey_login_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyCompletePayload>,
) -> Response {
    let runtime = agent_runtime().await.unwrap();
    let response: webauthn_rs_proto::PublicKeyCredential =
        serde_json::from_value(payload.credential).unwrap();
    match runtime.finish_passkey_login(payload.challenge_id, response).await {
        Ok(user) => {
            let is_secure = cookie_is_secure(&headers);
            let cookie = set_auth_cookie(&state.key, &user.id, is_secure);
            let mut jar = cookie::CookieJar::new();
            jar.private_mut(&state.key).add(cookie);
            let mut resp = Json(serde_json::json!({"redirect": "/"})).into_response();
            if let Some(h) = jar.delta().last() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    header::HeaderValue::from_str(&h.encoded().to_string()).unwrap(),
                );
            }
            resp
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// --- Static file handlers ---

async fn passkey_js_handler() -> impl IntoResponse {
    let js = include_str!("../public/passkey.js");
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], js)
}

async fn static_asset_handler(Path(filename): Path<String>) -> Response {
    let uri: Uri = format!("/{}", filename).parse().unwrap();
    fileserv::static_file_handler(uri).await
}
