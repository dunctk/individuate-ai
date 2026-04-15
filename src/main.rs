#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{routing::get, Router};
    use axum_extra::extract::cookie::Key;
    use individuateai::agent::{draft_stream_handler, graph_handler, stream_handler};
    use individuateai::app::*;
    use individuateai::fileserv::file_and_error_handler;
    use leptos::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};

    let _ = dotenvy::dotenv();

    // Setting get_configuration(None) means we'll use the default Cargo.toml location
    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let key = individuateai::agent::cookie_key();

    #[derive(Clone)]
    struct AppState {
        leptos_options: LeptosOptions,
        key: Key,
    }

    impl axum::extract::FromRef<AppState> for Key {
        fn from_ref(state: &AppState) -> Self {
            state.key.clone()
        }
    }

    impl axum::extract::FromRef<AppState> for LeptosOptions {
        fn from_ref(state: &AppState) -> Self {
            state.leptos_options.clone()
        }
    }

    let state = AppState {
        leptos_options: leptos_options.clone(),
        key,
    };

    // build our application with a route
    let app = Router::new()
        .route("/api/agent-stream", get(stream_handler))
        .route("/api/draft-stream", get(draft_stream_handler))
        .route("/api/graph/:user_id", get(graph_handler))
        .leptos_routes(&state, routes, App)
        .fallback(file_and_error_handler)
        .with_state(state);

    // run our app with hyper
    println!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
}
