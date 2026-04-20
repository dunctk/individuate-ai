#[cfg(feature = "hydrate")]
use crate::agent::agent_chat;
use crate::agent::{
    create_session, draft_message, finish_passkey_login, finish_passkey_register,
    finish_passkey_register_email, get_chat_history, get_context_user, get_patient_graph,
    get_relationship_profiles, get_sessions, login, logout, save_relationship_profile,
    start_passkey_login, start_passkey_register, start_passkey_register_email, PatientGraph,
    RelationshipProfile, User,
};
use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

// Safe wrapper for mindmap rendering
fn render_mind_map_safe(id: &str, data: &JsValue) {
    if let Some(window) = web_sys::window() {
        let func_name = JsValue::from_str("renderMindMap");
        if let Ok(func) = js_sys::Reflect::get(&window, &func_name) {
            if let Some(f) = func.dyn_ref::<js_sys::Function>() {
                let _ = f.call2(&JsValue::NULL, &JsValue::from_str(id), data);
            } else {
                leptos::logging::warn!("renderMindMap is not a function");
            }
        } else {
            leptos::logging::warn!("renderMindMap not found on window");
        }
    }
}

// Passkey JS wrappers
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn registerPasskey(options: JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn loginPasskey(options: JsValue) -> Result<JsValue, JsValue>;
}

#[derive(Clone, Debug, PartialEq)]
enum ChatRole {
    Assistant,
    User,
}

#[derive(Clone, Debug, PartialEq)]
struct ChatMessage {
    role: ChatRole,
    text: RwSignal<String>,
}

fn default_relationship_options() -> Vec<(String, String)> {
    vec![
        ("mother".to_string(), "Mother".to_string()),
        ("brother".to_string(), "Brother".to_string()),
        ("dad".to_string(), "Dad".to_string()),
        ("partner".to_string(), "Partner".to_string()),
        ("friend".to_string(), "Friend".to_string()),
    ]
}

fn parse_list_input(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for part in value.split(['\n', ',']) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed.to_lowercase();
        if seen.insert(lowered) {
            items.push(trimmed.to_string());
        }
    }
    items
}

fn join_list_input(items: &[String]) -> String {
    items.join("\n")
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    create_effect(move |_| {
        if let Some(window) = web_sys::window() {
            let location = window.location();
            if let Ok(hostname) = location.hostname() {
                if hostname == "127.0.0.1" {
                    let port = location.port().unwrap_or_default();
                    let port_part = if port.is_empty() {
                        String::new()
                    } else {
                        format!(":{port}")
                    };
                    let protocol = location.protocol().unwrap_or_else(|_| "http:".into());
                    let path = location.pathname().unwrap_or_default();
                    let search = location.search().unwrap_or_default();
                    let hash = location.hash().unwrap_or_default();
                    let next = format!("{protocol}//localhost{port_part}{path}{search}{hash}");
                    let _ = location.set_href(&next);
                }
            }
        }
    });

    // Global User State — blocking so SSR waits for auth before rendering,
    // preventing reactive re-triggers on disposed runtimes.
    let user_resource = create_blocking_resource(
        || (),
        |_| async move { get_context_user().await.ok().flatten() },
    );
    provide_context(user_resource);

    view! {
        <Stylesheet id="leptos" href="/pkg/individuateai.css"/>
        <Script src="/passkey.js"/>
        <Title text="IndividuateAI"/>

        <Router>
            <main class="min-h-screen bg-void-green text-parchment font-urbanist selection:bg-integral-turquoise selection:text-void-green overflow-x-hidden">
                <Routes>
                    <Route path="/login" view=LoginPage/>
                    <Route path="/signup" view=SignupPage/>
                    <Route path="" view=HomePage/>
                    <Route path="/mind-map" view=MindMapPage/>
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn LoginPage() -> impl IntoView {
    let (username, set_username) = create_signal(String::new());
    let (password, set_password) = create_signal(String::new());
    let (error, set_error) = create_signal(None::<String>);
    let user_resource = use_context::<Resource<(), Option<User>>>().expect("User resource missing");
    let navigate = use_navigate();
    let navigate_submit = navigate.clone();
    let navigate_passkey = navigate.clone();

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let u = username.get();
        let p = password.get();
        let navigate = navigate_submit.clone();
        spawn_local(async move {
            match login(u, p).await {
                Ok(_) => {
                    user_resource.refetch();
                    navigate("/", Default::default());
                }
                Err(e) => set_error.set(Some(e.to_string())),
            }
        });
    };

    let on_passkey = move |_| {
        let navigate = navigate_passkey.clone();
        spawn_local(async move {
            let u = username.get_untracked();
            if u.trim().is_empty() {
                set_error.set(Some("Please enter email for passkey login".into()));
                return;
            }

            // 1. Start
            let (req_id, challenge) = match start_passkey_login(u).await {
                Ok(res) => res,
                Err(e) => {
                    set_error.set(Some(e.to_string()));
                    return;
                }
            };

            // 2. JS Handshake
            let challenge_js = serde_wasm_bindgen::to_value(&challenge).unwrap();
            let cred_response = match loginPasskey(challenge_js).await {
                Ok(res) => res,
                Err(e) => {
                    set_error.set(Some(format!("Passkey error: {:?}", e)));
                    return;
                }
            };

            let response: webauthn_rs_proto::PublicKeyCredential =
                serde_wasm_bindgen::from_value(cred_response).unwrap();

            // 3. Finish
            match finish_passkey_login(req_id, response).await {
                Ok(_) => {
                    user_resource.refetch();
                    navigate("/", Default::default());
                }
                Err(e) => set_error.set(Some(e.to_string())),
            }
        });
    };

    view! {
        <div class="flex items-center justify-center min-h-screen bg-gradient-to-br from-void-green to-black/80">
            <div class="w-full max-w-md p-8 space-y-6 bg-white/5 backdrop-blur-xl border border-white/10 rounded-3xl shadow-2xl">
                <h1 class="text-3xl font-fraunces text-center text-parchment">"IndividuateAI"</h1>
                <h2 class="text-xs uppercase tracking-[0.2em] text-center text-sage-mist">"Sign in"</h2>

                {move || error.get().map(|e| view! { <div class="p-3 text-sm text-red-200 bg-red-900/20 border border-red-500/30 rounded-xl text-center">{e}</div> })}

                <form on:submit=on_submit class="space-y-4">
                    <div>
                        <label class="block text-xs uppercase tracking-wider text-white/40 mb-2">"Email"</label>
                        <input type="text" class="w-full bg-black/20 border border-white/10 rounded-xl px-4 py-3 focus:outline-none focus:border-integral-turquoise/50 text-parchment"
                            on:input=move |ev| set_username.set(event_target_value(&ev))
                            prop:value=username
                        />
                    </div>
                    <div>
                        <label class="block text-xs uppercase tracking-wider text-white/40 mb-2">"Password"</label>
                        <input type="password" class="w-full bg-black/20 border border-white/10 rounded-xl px-4 py-3 focus:outline-none focus:border-integral-turquoise/50 text-parchment"
                            on:input=move |ev| set_password.set(event_target_value(&ev))
                            prop:value=password
                        />
                    </div>
                    <button type="submit" class="w-full py-3 bg-integral-turquoise/20 hover:bg-integral-turquoise/30 border border-integral-turquoise/40 rounded-xl text-integral-turquoise font-bold transition-all">
                        "Login"
                    </button>
                </form>

                <div class="relative flex py-2 items-center">
                    <div class="flex-grow border-t border-white/10"></div>
                    <span class="flex-shrink-0 mx-4 text-white/20 text-xs uppercase tracking-widest">OR</span>
                    <div class="flex-grow border-t border-white/10"></div>
                </div>

                <button on:click=on_passkey class="w-full py-3 bg-white/5 hover:bg-white/10 border border-white/20 rounded-xl text-parchment font-bold transition-all flex items-center justify-center gap-2">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
                    </svg>
                    "Sign in with Passkey"
                </button>

                <div class="text-center text-sm text-white/40">
                    "New here? " <A href="/signup" class="text-integral-turquoise hover:underline">"Create account"</A>
                </div>
            </div>
        </div>
    }
}

#[component]
fn SignupPage() -> impl IntoView {
    let (email, set_email) = create_signal(String::new());
    let (error, set_error) = create_signal(None::<String>);
    let user_resource = use_context::<Resource<(), Option<User>>>().expect("User resource missing");
    let navigate = use_navigate();

    let on_passkey_signup = move |_| {
        let navigate = navigate.clone();
        spawn_local(async move {
            set_error.set(None);
            let addr = email.get_untracked();
            if addr.trim().is_empty() {
                set_error.set(Some("Please enter an email to continue".into()));
                return;
            }

            let (req_id, challenge) = match start_passkey_register_email(addr).await {
                Ok(res) => res,
                Err(e) => {
                    set_error.set(Some(e.to_string()));
                    return;
                }
            };

            let challenge_js = serde_wasm_bindgen::to_value(&challenge).unwrap();
            let cred_response = match registerPasskey(challenge_js).await {
                Ok(res) => res,
                Err(e) => {
                    set_error.set(Some(format!("Passkey error: {:?}", e)));
                    return;
                }
            };

            let response: webauthn_rs_proto::RegisterPublicKeyCredential =
                serde_wasm_bindgen::from_value(cred_response).unwrap();

            match finish_passkey_register_email(req_id, response).await {
                Ok(_) => {
                    user_resource.refetch();
                    navigate("/", Default::default());
                }
                Err(e) => set_error.set(Some(e.to_string())),
            }
        });
    };

    view! {
        <div class="flex items-center justify-center min-h-screen bg-gradient-to-br from-void-green to-black/80">
            <div class="w-full max-w-md p-8 space-y-6 bg-white/5 backdrop-blur-xl border border-white/10 rounded-3xl shadow-2xl">
                <h1 class="text-3xl font-fraunces text-center text-parchment">"Create account"</h1>

                {move || error.get().map(|e| view! { <div class="p-3 text-sm text-red-200 bg-red-900/20 border border-red-500/30 rounded-xl text-center">{e}</div> })}

                <div class="space-y-4">
                    <div>
                        <label class="block text-xs uppercase tracking-wider text-white/40 mb-2">"Email"</label>
                        <input type="email" class="w-full bg-black/20 border border-white/10 rounded-xl px-4 py-3 focus:outline-none focus:border-integral-turquoise/50 text-parchment"
                            on:input=move |ev| set_email.set(event_target_value(&ev))
                            prop:value=email
                        />
                    </div>
                    <button on:click=on_passkey_signup class="w-full py-3 bg-systemic-yellow/20 hover:bg-systemic-yellow/30 border border-systemic-yellow/40 rounded-xl text-systemic-yellow font-bold transition-all flex items-center justify-center gap-2">
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5">
                          <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
                        </svg>
                        "Create passkey"
                    </button>
                </div>
                <div class="text-center text-sm text-white/40">
                    "Already have an account? " <A href="/login" class="text-integral-turquoise hover:underline">"Login"</A>
                </div>
            </div>
        </div>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let user_resource = use_context::<Resource<(), Option<User>>>().expect("User resource missing");
    let navigate = use_navigate();

    // Auth Guard
    create_effect(move |_| {
        if user_resource.loading().get() {
            return;
        }
        if !matches!(user_resource.get(), Some(Some(_))) {
            navigate("/login", Default::default());
        }
    });

    let (chat_input, set_chat_input) = create_signal(String::new());
    let (is_loading, set_is_loading) = create_signal(false);
    let (is_sidebar_open, set_is_sidebar_open) = create_signal(false);
    let (is_profile_drawer_open, set_is_profile_drawer_open) = create_signal(false);
    let (toast, set_toast) = create_signal(None::<String>);

    create_effect(move |_| {
        if toast.get().is_some() {
            let set_toast = set_toast.clone();
            set_timeout(
                move || set_toast.set(None),
                std::time::Duration::from_secs(4),
            );
        }
    });

    let (accountability, set_accountability) = create_signal(50);
    let (spirituality, set_spirituality) = create_signal(30);
    let (directness, set_directness) = create_signal(70);
    let (composer_mode, set_composer_mode) = create_signal("therapist".to_string());
    let (draft_relationship, set_draft_relationship) = create_signal("mother".to_string());
    let (draft_intent, set_draft_intent) = create_signal("repair".to_string());
    let (profile_editor_slug, set_profile_editor_slug) = create_signal("mother".to_string());
    let (profile_display_name, set_profile_display_name) = create_signal(String::new());
    let (profile_relationship_type, set_profile_relationship_type) = create_signal(String::new());
    let (profile_background, set_profile_background) = create_signal(String::new());
    let (profile_goals, set_profile_goals) = create_signal(String::new());
    let (profile_triggers, set_profile_triggers) = create_signal(String::new());
    let (profile_do_not_say, set_profile_do_not_say) = create_signal(String::new());
    let (profile_effective_tone, set_profile_effective_tone) = create_signal(String::new());
    let (profile_recent_events, set_profile_recent_events) = create_signal(String::new());
    let (profile_boundaries, set_profile_boundaries) = create_signal(String::new());

    let sessions_resource = create_resource(
        move || user_resource.get(),
        |user| async move {
            if let Some(Some(_)) = user {
                get_sessions().await.unwrap_or_default()
            } else {
                vec![]
            }
        },
    );
    let relationship_profiles_resource = create_resource(
        move || user_resource.get(),
        |user| async move {
            if let Some(Some(_)) = user {
                get_relationship_profiles().await.unwrap_or_default()
            } else {
                Vec::<RelationshipProfile>::new()
            }
        },
    );

    let (search_query, set_search_query) = create_signal(String::new());
    let (selected_session, set_selected_session) = create_signal(None::<String>);

    let history_resource = create_local_resource(
        move || selected_session.get(),
        |session_id| async move {
            if let Some(id) = session_id {
                get_chat_history(id).await.unwrap_or_default()
            } else {
                vec![]
            }
        },
    );

    let conversations = create_rw_signal(HashMap::<String, Vec<ChatMessage>>::new());

    create_effect(move |_| {
        let session_id = selected_session.get();
        if let Some(id) = session_id {
            if let Some(logs) = history_resource.get() {
                conversations.update(|map| {
                    if !map.contains_key(&id) || map.get(&id).map(|v| v.is_empty()).unwrap_or(true)
                    {
                        let msgs: Vec<ChatMessage> = logs
                            .into_iter()
                            .map(|log| ChatMessage {
                                role: if log.role == "user" {
                                    ChatRole::User
                                } else {
                                    ChatRole::Assistant
                                },
                                text: create_rw_signal(log.content),
                            })
                            .collect();
                        map.insert(id, msgs);
                    }
                });
            }
        }
    });

    let current_messages = move || {
        if let Some(id) = selected_session.get() {
            conversations.with(|map| map.get(&id).cloned().unwrap_or_else(Vec::new))
        } else {
            vec![ChatMessage {
                role: ChatRole::Assistant,
                text: create_rw_signal(
                    "Select a session or create a new one to begin.".to_string(),
                ),
            }]
        }
    };

    let filtered_sessions = move || {
        let query = search_query.get().to_lowercase();
        let all = sessions_resource.get().unwrap_or_default();
        if query.is_empty() {
            all
        } else {
            all.into_iter()
                .filter(|s| {
                    s.title.to_lowercase().contains(&query)
                        || s.preview.to_lowercase().contains(&query)
                })
                .collect()
        }
    };
    let relationship_options = move || {
        let mut options = default_relationship_options();
        let mut seen: HashMap<String, ()> =
            options.iter().map(|(slug, _)| (slug.clone(), ())).collect();
        for profile in relationship_profiles_resource.get().unwrap_or_default() {
            if !seen.contains_key(&profile.slug) {
                options.push((profile.slug.clone(), profile.display_name.clone()));
                seen.insert(profile.slug, ());
            }
        }
        options
    };
    let selected_draft_profile = move || {
        relationship_profiles_resource
            .get()
            .unwrap_or_default()
            .into_iter()
            .find(|profile| profile.slug == draft_relationship.get())
    };

    create_effect(move |_| {
        let slug = profile_editor_slug.get();
        let options = relationship_options();
        let option_label = options
            .iter()
            .find(|(candidate, _)| *candidate == slug)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| slug.replace('_', " "));

        if let Some(profile) = relationship_profiles_resource
            .get()
            .unwrap_or_default()
            .into_iter()
            .find(|profile| profile.slug == slug)
        {
            set_profile_display_name.set(profile.display_name);
            set_profile_relationship_type.set(profile.relationship_type);
            set_profile_background.set(profile.background);
            set_profile_goals.set(join_list_input(&profile.goals));
            set_profile_triggers.set(join_list_input(&profile.triggers));
            set_profile_do_not_say.set(join_list_input(&profile.do_not_say));
            set_profile_effective_tone.set(join_list_input(&profile.effective_tone));
            set_profile_recent_events.set(join_list_input(&profile.recent_events));
            set_profile_boundaries.set(join_list_input(&profile.boundaries));
        } else {
            set_profile_display_name.set(option_label.clone());
            set_profile_relationship_type.set(slug.clone());
            set_profile_background.set(String::new());
            set_profile_goals.set(String::new());
            set_profile_triggers.set(String::new());
            set_profile_do_not_say.set(String::new());
            set_profile_effective_tone.set(String::new());
            set_profile_recent_events.set(String::new());
            set_profile_boundaries.set(String::new());
        }
    });

    let save_profile_action = create_action(move |_| {
        let slug = profile_editor_slug.get_untracked();
        let display_name = profile_display_name.get_untracked();
        let relationship_type = profile_relationship_type.get_untracked();
        let background = profile_background.get_untracked();
        let goals = parse_list_input(&profile_goals.get_untracked());
        let triggers = parse_list_input(&profile_triggers.get_untracked());
        let do_not_say = parse_list_input(&profile_do_not_say.get_untracked());
        let effective_tone = parse_list_input(&profile_effective_tone.get_untracked());
        let recent_events = parse_list_input(&profile_recent_events.get_untracked());
        let boundaries = parse_list_input(&profile_boundaries.get_untracked());
        async move {
            save_relationship_profile(
                slug,
                display_name,
                relationship_type,
                background,
                goals,
                triggers,
                do_not_say,
                effective_tone,
                recent_events,
                boundaries,
            )
            .await
        }
    });

    create_effect(move |_| {
        if let Some(result) = save_profile_action.value().get() {
            match result {
                Ok(_) => {
                    relationship_profiles_resource.refetch();
                    set_toast.set(Some("Relationship profile saved.".into()));
                }
                Err(err) => set_toast.set(Some(err.to_string())),
            }
        }
    });

    let create_new_chat = create_action(move |title: &String| {
        let title = title.clone();
        async move {
            match create_session(title).await {
                Ok(session) => {
                    sessions_resource.refetch();
                    set_selected_session.set(Some(session.id.clone()));
                    conversations.update(|map| {
                        map.insert(
                            session.id,
                            vec![ChatMessage {
                                role: ChatRole::Assistant,
                                text: create_rw_signal("Welcome.".to_string()),
                            }],
                        );
                    });
                }
                Err(e) => leptos::logging::error!("Failed: {}", e),
            }
        }
    });

    let submit_with_session = {
        let set_chat_input = set_chat_input.clone();
        let set_is_loading = set_is_loading.clone();
        let conversations = conversations.clone();
        let set_toast = set_toast.clone();
        let accountability = accountability.clone();
        let spirituality = spirituality.clone();
        let directness = directness.clone();
        let _user_resource = user_resource.clone();
        let composer_mode = composer_mode.clone();
        let draft_relationship = draft_relationship.clone();
        let draft_intent = draft_intent.clone();
        let sessions_resource = sessions_resource.clone();

        Rc::new(move |session_id: String, trimmed: String| {
            let mode = composer_mode.get_untracked();
            let relationship_slug = draft_relationship.get_untracked();
            let intent = draft_intent.get_untracked();
            let display_user_text = if mode == "draft" {
                format!("Draft to {} ({}): {}", relationship_slug, intent, trimmed)
            } else {
                trimmed.clone()
            };

            // Optimistic Update
            conversations.update(|map| {
                let entry = map.entry(session_id.clone()).or_insert_with(Vec::new);
                entry.push(ChatMessage {
                    role: ChatRole::User,
                    text: create_rw_signal(display_user_text),
                });
                entry.push(ChatMessage {
                    role: ChatRole::Assistant,
                    text: create_rw_signal(String::new()),
                });
            });

            set_chat_input.set(String::new());
            set_is_loading.set(true);

            if mode == "draft" {
                #[cfg(feature = "hydrate")]
                {
                    use wasm_bindgen::{closure::Closure, JsCast};
                    use web_sys::{Event, EventSource, MessageEvent};

                    let current_user_id = if let Some(Some(u)) = _user_resource.get() {
                        u.id
                    } else {
                        set_toast.set(Some("User context lost. Please refresh.".into()));
                        set_is_loading.set(false);
                        return;
                    };

                    let accountability_value = accountability.get_untracked();
                    let spirituality_value = spirituality.get_untracked();
                    let directness_value = directness.get_untracked();
                    let prompt = trimmed.clone();
                    let url = format!(
                        "/api/draft-stream?session_id={}&user_id={}&relationship_slug={}&intent={}&accountability={}&spirituality={}&directness={}&prompt={}",
                        session_id,
                        current_user_id,
                        urlencoding::encode(&relationship_slug),
                        urlencoding::encode(&intent),
                        accountability_value,
                        spirituality_value,
                        directness_value,
                        urlencoding::encode(&prompt)
                    );

                    match EventSource::new(&url) {
                        Ok(es) => {
                            let es_clone = es.clone();
                            let es_err = es.clone();
                            let session_id_clone = session_id.clone();
                            let sessions_resource = sessions_resource.clone();

                            let on_message = Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new({
                                let conversations = conversations.clone();
                                let set_is_loading = set_is_loading.clone();
                                let set_toast = set_toast.clone();
                                let session_id = session_id.clone();
                                move |event: MessageEvent| {
                                    if let Some(data) = event.data().as_string() {
                                        if data.starts_with("error:") {
                                            set_toast.set(Some(data.clone()));
                                        }
                                        if data == "[DONE]" {
                                            set_is_loading.set(false);
                                            sessions_resource.refetch();
                                            es_clone.close();
                                            return;
                                        }
                                        conversations.update(|map| {
                                            if let Some(entry) = map.get_mut(&session_id) {
                                                if let Some(last) = entry.last() {
                                                    if let ChatRole::Assistant = last.role {
                                                        last.text.update(|t| t.push_str(&data));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                }
                            }));
                            es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
                            on_message.forget();

                            let on_error = Closure::<dyn FnMut(Event)>::wrap(Box::new({
                                let set_is_loading = set_is_loading.clone();
                                let set_toast = set_toast.clone();
                                let conversations = conversations.clone();
                                let session_id = session_id_clone.clone();
                                let prompt_fallback = prompt.clone();
                                let relationship_slug = relationship_slug.clone();
                                let intent = intent.clone();
                                let sessions_resource = sessions_resource.clone();
                                move |_| {
                                    set_is_loading.set(false);
                                    set_toast.set(Some("Stream error from drafter".to_string()));
                                    es_err.close();
                                    let conversations = conversations.clone();
                                    let session_id = session_id.clone();
                                    let prompt_fallback = prompt_fallback.clone();
                                    let relationship_slug = relationship_slug.clone();
                                    let intent = intent.clone();
                                    let sessions_resource = sessions_resource.clone();
                                    spawn_local(async move {
                                        match draft_message(
                                            prompt_fallback.clone(),
                                            session_id.clone(),
                                            relationship_slug,
                                            intent,
                                            accountability_value,
                                            spirituality_value,
                                            directness_value,
                                        )
                                        .await
                                        {
                                            Ok(text) => {
                                                conversations.update(|map| {
                                                    if let Some(entry) = map.get_mut(&session_id) {
                                                        if let Some(last) = entry.last() {
                                                            if let ChatRole::Assistant = last.role {
                                                                last.text.set(text);
                                                            }
                                                        }
                                                    }
                                                });
                                                sessions_resource.refetch();
                                            }
                                            Err(e) => set_toast.set(Some(e.to_string())),
                                        }
                                    });
                                }
                            }));
                            es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
                            on_error.forget();
                        }
                        Err(e) => {
                            set_is_loading.set(false);
                            set_toast.set(Some(format!("Failed to start draft stream: {:?}", e)));
                        }
                    }
                }
                #[cfg(not(feature = "hydrate"))]
                {
                    let conversations = conversations.clone();
                    let set_is_loading = set_is_loading.clone();
                    let set_toast = set_toast.clone();
                    let sessions_resource = sessions_resource.clone();
                    let accountability_value = accountability.get_untracked();
                    let spirituality_value = spirituality.get_untracked();
                    let directness_value = directness.get_untracked();
                    let prompt = trimmed.clone();
                    let session_id_for_request = session_id.clone();
                    spawn_local(async move {
                        match draft_message(
                            prompt,
                            session_id_for_request.clone(),
                            relationship_slug,
                            intent,
                            accountability_value,
                            spirituality_value,
                            directness_value,
                        )
                        .await
                        {
                            Ok(text) => {
                                conversations.update(|map| {
                                    if let Some(entry) = map.get_mut(&session_id_for_request) {
                                        if let Some(last) = entry.last() {
                                            if let ChatRole::Assistant = last.role {
                                                last.text.set(text);
                                            }
                                        }
                                    }
                                });
                                sessions_resource.refetch();
                            }
                            Err(e) => set_toast.set(Some(e.to_string())),
                        }
                        set_is_loading.set(false);
                    });
                }
                return;
            }

            #[cfg(feature = "hydrate")]
            {
                use wasm_bindgen::{closure::Closure, JsCast};
                use web_sys::{Event, EventSource, MessageEvent};

                let prompt = format!(
                    "User input: {}\n\nControls -> accountability: {}, spirituality: {}, directness: {}.",
                    trimmed,
                    accountability.get_untracked(),
                    spirituality.get_untracked(),
                    directness.get_untracked()
                );

                let current_user_id = if let Some(Some(u)) = _user_resource.get() {
                    u.id
                } else {
                    set_toast.set(Some("User context lost. Please refresh.".into()));
                    set_is_loading.set(false);
                    return;
                };

                let url = format!(
                    "/api/agent-stream?session_id={}&user_id={}&prompt={}",
                    session_id,
                    current_user_id,
                    urlencoding::encode(&prompt)
                );

                match EventSource::new(&url) {
                    Ok(es) => {
                        let es_clone = es.clone();
                        let es_err = es.clone();
                        let session_id_clone = session_id.clone();
                        let sessions_resource = sessions_resource.clone();

                        let on_message = Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new({
                            let conversations = conversations.clone();
                            let set_is_loading = set_is_loading.clone();
                            let session_id = session_id.clone();
                            let set_toast = set_toast.clone();
                            move |event: MessageEvent| {
                                if let Some(data) = event.data().as_string() {
                                    if data.starts_with("error:") {
                                        set_toast.set(Some(data.clone()));
                                    }
                                    if data == "[DONE]" {
                                        set_is_loading.set(false);
                                        sessions_resource.refetch();
                                        es_clone.close();
                                        return;
                                    }
                                    conversations.update(|map| {
                                        if let Some(entry) = map.get_mut(&session_id) {
                                            if let Some(last) = entry.last() {
                                                if let ChatRole::Assistant = last.role {
                                                    last.text.update(|t| t.push_str(&data));
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        }));
                        es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
                        on_message.forget();

                        let on_error = Closure::<dyn FnMut(Event)>::wrap(Box::new({
                            let set_is_loading = set_is_loading.clone();
                            let set_toast = set_toast.clone();
                            let conversations = conversations.clone();
                            let session_id = session_id_clone.clone();
                            let prompt_clone = prompt.clone();
                            let sessions_resource = sessions_resource.clone();
                            move |_| {
                                set_is_loading.set(false);
                                set_toast.set(Some("Stream error from agent".to_string()));
                                es_err.close();
                                // Fallback
                                let conversations = conversations.clone();
                                let prompt_fallback = prompt_clone.clone();
                                let session_id = session_id.clone();
                                let sessions_resource = sessions_resource.clone();
                                spawn_local(async move {
                                    match agent_chat(prompt_fallback, session_id.clone()).await {
                                        Ok(text) => {
                                            conversations.update(|map| {
                                                if let Some(entry) = map.get_mut(&session_id) {
                                                    if let Some(last) = entry.last() {
                                                        if let ChatRole::Assistant = last.role {
                                                            last.text.set(text);
                                                        }
                                                    }
                                                }
                                            });
                                            sessions_resource.refetch();
                                        }
                                        Err(e) => set_toast.set(Some(e.to_string())),
                                    }
                                });
                            }
                        }));
                        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
                        on_error.forget();
                    }
                    Err(e) => {
                        set_is_loading.set(false);
                        set_toast.set(Some(format!("Failed to start stream: {:?}", e)));
                    }
                }
            }
            #[cfg(not(feature = "hydrate"))]
            {
                spawn_local(async move {
                    // Logic for fallback
                });
            }
        })
    };

    let on_submit_handler = {
        let chat_input = chat_input.clone();
        let conversations = conversations.clone();
        let set_toast = set_toast.clone();
        let selected_session = selected_session.clone();
        let set_selected_session = set_selected_session.clone();
        let sessions_resource = sessions_resource.clone();
        let submit_with_session = submit_with_session.clone();

        Rc::new(move || {
            let text = chat_input.get_untracked();
            let trimmed = text.trim();
            if trimmed.is_empty() || is_loading.get_untracked() {
                return;
            }

            let trimmed = trimmed.to_string();
            if let Some(id) = selected_session.get_untracked() {
                submit_with_session(id, trimmed);
                return;
            }

            set_is_loading.set(true);
            let conversations = conversations.clone();
            let sessions_resource = sessions_resource.clone();
            let set_selected_session = set_selected_session.clone();
            let submit_with_session = submit_with_session.clone();
            let set_toast = set_toast.clone();
            let set_is_loading = set_is_loading.clone();

            spawn_local(async move {
                match create_session("New Session".to_string()).await {
                    Ok(session) => {
                        sessions_resource.refetch();
                        set_selected_session.set(Some(session.id.clone()));
                        conversations.update(|map| {
                            map.insert(
                                session.id.clone(),
                                vec![ChatMessage {
                                    role: ChatRole::Assistant,
                                    text: create_rw_signal("Welcome.".to_string()),
                                }],
                            );
                        });
                        submit_with_session(session.id, trimmed);
                    }
                    Err(e) => {
                        leptos::logging::error!("Failed: {}", e);
                        set_toast.set(Some("Failed to create session.".into()));
                        set_is_loading.set(false);
                    }
                }
            });
        })
    };

    let submit_preset = {
        let selected_session = selected_session.clone();
        let set_selected_session = set_selected_session.clone();
        let sessions_resource = sessions_resource.clone();
        let conversations = conversations.clone();
        let set_toast = set_toast.clone();
        let set_is_loading = set_is_loading.clone();
        let submit_with_session = submit_with_session.clone();

        Rc::new(move |text: String| {
            if text.trim().is_empty() || is_loading.get_untracked() {
                return;
            }

            if let Some(id) = selected_session.get_untracked() {
                submit_with_session(id, text);
                return;
            }

            set_is_loading.set(true);
            let conversations = conversations.clone();
            let sessions_resource = sessions_resource.clone();
            let set_selected_session = set_selected_session.clone();
            let submit_with_session = submit_with_session.clone();
            let set_toast = set_toast.clone();
            let set_is_loading = set_is_loading.clone();

            spawn_local(async move {
                match create_session("New Session".to_string()).await {
                    Ok(session) => {
                        sessions_resource.refetch();
                        set_selected_session.set(Some(session.id.clone()));
                        conversations.update(|map| {
                            map.insert(
                                session.id.clone(),
                                vec![ChatMessage {
                                    role: ChatRole::Assistant,
                                    text: create_rw_signal("Welcome.".to_string()),
                                }],
                            );
                        });
                        submit_with_session(session.id, text);
                    }
                    Err(e) => {
                        leptos::logging::error!("Failed: {}", e);
                        set_toast.set(Some("Failed to create session.".into()));
                        set_is_loading.set(false);
                    }
                }
            });
        })
    };

    let on_submit_key = on_submit_handler.clone();
    let on_submit_click = on_submit_handler.clone();
    let submit_continue = submit_preset.clone();
    let submit_go_deeper = submit_preset.clone();
    let submit_be_direct = submit_preset.clone();
    let submit_summarize_pattern = submit_preset.clone();

    let app_view = move || {
        let submit_continue = submit_continue.clone();
        let submit_go_deeper = submit_go_deeper.clone();
        let submit_be_direct = submit_be_direct.clone();
        let submit_summarize_pattern = submit_summarize_pattern.clone();
        view! {
        <div class="flex h-screen relative overflow-hidden">
            // Side Panel
            <div class=move || format!("fixed inset-y-0 left-0 z-50 w-80 bg-void-green/95 backdrop-blur-xl border-r border-white/5 shadow-2xl transform transition-transform duration-500 ease-out flex flex-col {}", if is_sidebar_open.get() { "translate-x-0 pointer-events-auto" } else { "-translate-x-full pointer-events-none" })>
                <div class="p-6 border-b border-white/5 space-y-4">
                    <div class="flex items-center justify-between">
                        <h2 class="font-fraunces text-xl text-parchment">"History"</h2>
                        <button class="p-2 hover:bg-white/5 rounded-full" on:click=move |_| set_is_sidebar_open.set(false)>
                            <svg class="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M6 18L18 6M6 6l12 12" /></svg>
                        </button>
                    </div>
                    <button class="w-full bg-integral-turquoise/10 hover:bg-integral-turquoise/20 text-integral-turquoise font-bold py-2 rounded-xl transition-colors mb-2"
                        on:click=move |_| create_new_chat.dispatch("New Session".to_string())>"+ New Session"</button>
                    <div class="relative">
                        <input type="text" class="w-full bg-white/5 border border-white/10 rounded-xl py-2 pl-10 pr-4 text-sm text-parchment"
                            placeholder="Search..." on:input=move |ev| set_search_query.set(event_target_value(&ev)) />
                    </div>
                </div>
                <div class="px-6 py-6 space-y-6 border-b border-white/5 bg-black/20">
                    <h3 class="text-xs font-bold text-sage-mist tracking-widest uppercase">"Model Parameters"</h3>
                    <Slider label="Accountability" value=accountability set_value=set_accountability min_label="Gentle" max_label="Ruthless" />
                    <Slider label="Spirituality" value=spirituality set_value=set_spirituality min_label="Grounded" max_label="Transcendent" />
                    <Slider label="Directness" value=directness set_value=set_directness min_label="Soft" max_label="Blunt" />
                </div>
                <div class="flex-1 overflow-y-auto p-4 space-y-2 custom-scrollbar">
                    <Suspense fallback=move || view! { <p class="text-center text-white/20 p-4">"Loading..."</p> }>
                        <For each=filtered_sessions key=|s| s.id.clone() children=move |session| {
                            let set_selected_session = set_selected_session.clone();
                            let sid = session.id.clone();
                            let sid_click = session.id.clone();
                            let is_active = move || selected_session.get() == Some(sid.clone());
                            view! {
                                <div class=move || format!("group p-4 rounded-xl cursor-pointer transition-all border {}", if is_active() { "bg-white/10 border-integral-turquoise/30" } else { "hover:bg-white/5 border-transparent" })
                                    on:click=move |_| set_selected_session.set(Some(sid_click.clone()))>
                                    <div class="flex justify-between items-baseline mb-1">
                                        <h4 class="font-bold text-sm text-parchment group-hover:text-integral-turquoise">{session.title}</h4>
                                    </div>
                                    <p class="text-xs text-sage-mist line-clamp-2">{session.preview}</p>
                                </div>
                            }
                        }/>
                    </Suspense>
                </div>
                <div class="p-4 border-t border-white/5 bg-void-green"><UserMenu /></div>
            </div>

            // Overlay
            <div class=move || format!("fixed inset-0 bg-black/60 backdrop-blur-sm z-40 transition-opacity duration-500 {}", if is_sidebar_open.get() { "opacity-100 pointer-events-auto" } else { "opacity-0 pointer-events-none" }) on:click=move |_| set_is_sidebar_open.set(false)></div>
            <div class=move || format!("fixed inset-0 bg-black/50 backdrop-blur-sm z-[55] transition-opacity duration-300 {}", if is_profile_drawer_open.get() { "opacity-100 pointer-events-auto" } else { "opacity-0 pointer-events-none" }) on:click=move |_| set_is_profile_drawer_open.set(false)></div>
            <div class=move || format!("fixed top-0 right-0 h-full w-full max-w-xl z-[60] bg-void-green/95 backdrop-blur-xl border-l border-white/10 shadow-2xl transform transition-transform duration-300 {}", if is_profile_drawer_open.get() { "translate-x-0 pointer-events-auto" } else { "translate-x-full pointer-events-none" })>
                <div class="h-full flex flex-col">
                    <div class="p-6 border-b border-white/10 flex items-start justify-between gap-4">
                        <div>
                            <h2 class="font-fraunces text-2xl text-parchment">"Relationship Profile"</h2>
                            <p class="text-sm text-white/45">"Review or correct what drafting should remember."</p>
                        </div>
                        <div class="flex gap-2">
                            <button
                                class="px-3 py-2 rounded-full bg-integral-turquoise/15 border border-integral-turquoise/30 text-[10px] uppercase tracking-[0.18em] text-integral-turquoise hover:bg-integral-turquoise/25 transition"
                                on:click=move |_| save_profile_action.dispatch(())
                            >
                                "Save"
                            </button>
                            <button class="p-2 hover:bg-white/5 rounded-full" on:click=move |_| set_is_profile_drawer_open.set(false)>
                                <svg class="w-6 h-6 text-sage-mist" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M6 18L18 6M6 6l12 12" /></svg>
                            </button>
                        </div>
                    </div>
                    <div class="flex-1 overflow-y-auto p-6 space-y-4 custom-scrollbar">
                        <select
                            class="w-full border border-white/10 rounded-xl px-4 py-3 text-xs uppercase tracking-[0.18em] text-parchment focus:outline-none"
                            style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;"
                            prop:value=profile_editor_slug
                            on:change=move |ev| set_profile_editor_slug.set(event_target_value(&ev))
                        >
                            <For
                                each=relationship_options
                                key=|(slug, _)| slug.clone()
                                children=move |(slug, label)| view! { <option value=slug>{label}</option> }
                            />
                        </select>
                        <input type="text" class="w-full border border-white/10 rounded-xl px-4 py-3 text-sm text-parchment focus:outline-none" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Display name" prop:value=profile_display_name on:input=move |ev| set_profile_display_name.set(event_target_value(&ev)) />
                        <input type="text" class="w-full border border-white/10 rounded-xl px-4 py-3 text-sm text-parchment focus:outline-none" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Relationship type" prop:value=profile_relationship_type on:input=move |ev| set_profile_relationship_type.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[96px] border border-white/10 rounded-xl px-4 py-3 text-sm text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Background" prop:value=profile_background on:input=move |ev| set_profile_background.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[84px] border border-white/10 rounded-xl px-4 py-3 text-xs text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Goals, one per line" prop:value=profile_goals on:input=move |ev| set_profile_goals.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[84px] border border-white/10 rounded-xl px-4 py-3 text-xs text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Triggers, one per line" prop:value=profile_triggers on:input=move |ev| set_profile_triggers.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[84px] border border-white/10 rounded-xl px-4 py-3 text-xs text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Boundaries, one per line" prop:value=profile_boundaries on:input=move |ev| set_profile_boundaries.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[72px] border border-white/10 rounded-xl px-4 py-3 text-xs text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Tone preferences, one per line" prop:value=profile_effective_tone on:input=move |ev| set_profile_effective_tone.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[72px] border border-white/10 rounded-xl px-4 py-3 text-xs text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Recent events, one per line" prop:value=profile_recent_events on:input=move |ev| set_profile_recent_events.set(event_target_value(&ev)) />
                        <textarea class="w-full min-h-[72px] border border-white/10 rounded-xl px-4 py-3 text-xs text-parchment focus:outline-none resize-y" style="background-color: rgba(0, 0, 0, 0.30); color: #F2F0E9; color-scheme: dark;" placeholder="Do not say, one per line" prop:value=profile_do_not_say on:input=move |ev| set_profile_do_not_say.set(event_target_value(&ev)) />
                    </div>
                </div>
            </div>

            // Main Content
            <div class="flex-1 flex flex-col h-full relative w-full transition-all duration-500">
                <header class="p-6 sticky top-0 z-10 flex items-center justify-between">
                    <button class="p-2 -ml-2 text-sage-mist hover:text-parchment transition-colors rounded-full hover:bg-white/5" on:click=move |_| set_is_sidebar_open.set(true)>
                        <svg class="w-8 h-8" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" /></svg>
                    </button>
                    <h1 class="text-2xl font-fraunces text-center bg-clip-text text-transparent bg-gradient-to-r from-parchment to-sage-mist">"IndividuateAI"</h1>
                    <div class="flex items-center gap-2">
                        <button
                            class="px-3 py-2 rounded-full border border-white/10 text-xs uppercase tracking-[0.2em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/40 transition"
                            on:click=move |_| set_is_profile_drawer_open.set(true)
                        >
                            "Profiles"
                        </button>
                        <A href="/mind-map" class="px-3 py-2 rounded-full border border-white/10 text-xs uppercase tracking-[0.2em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/40 transition">"Mind Map"</A>
                    </div>
                </header>
                <div class="flex-1 overflow-y-auto px-4 pb-40 w-full max-w-3xl mx-auto custom-scrollbar">
                    <div class="space-y-8 py-10">
                        <For
                            each={move || current_messages().into_iter().enumerate().collect::<Vec<_>>()}
                            key=|(idx, _)| *idx
                            children=move |(_, msg)| {
                                let is_assistant = matches!(msg.role, ChatRole::Assistant);
                                let alignment = if is_assistant { "items-start" } else { "items-end" };
                                let label_class = if is_assistant { "text-integral-turquoise" } else { "text-systemic-yellow" };
                                let bubble_classes = if is_assistant { "bg-sage-mist/10 p-6 rounded-2xl rounded-tl-sm border border-white/5 backdrop-blur-md shadow-lg" } else { "bg-white/5 p-6 rounded-2xl rounded-tr-sm border border-white/5 backdrop-blur-md" };
                                view! {
                                    <div class=format!("flex flex-col {alignment} space-y-2 animate-fade-in-up")>
                                        <div class=format!("text-xs font-bold tracking-[0.2em] uppercase {} px-2", label_class)>{if is_assistant { "Therapist" } else { "You" }}</div>
                                        <div class=bubble_classes><p class="text-lg leading-relaxed font-light whitespace-pre-wrap">{move || msg.text.get()}</p></div>
                                    </div>
                                }
                            }
                        />
                    </div>
                </div>
                <div class="absolute bottom-0 left-0 right-0 p-4 pb-8 z-20 bg-gradient-to-t from-void-green via-void-green/90 to-transparent">
                    <div class="max-w-3xl mx-auto relative group">
                        <div class="absolute inset-0 bg-void-green/60 backdrop-blur-xl rounded-3xl border border-white/10 shadow-2xl transition-all duration-300 group-hover:bg-void-green/80"></div>
                        <div class="relative p-3 space-y-3">
                            <div class="flex flex-wrap items-center gap-2 px-3 pt-1">
                                <button
                                    class=move || format!("px-3 py-2 rounded-full text-xs uppercase tracking-[0.18em] border transition {}", if composer_mode.get() == "therapist" { "bg-integral-turquoise/20 border-integral-turquoise/40 text-integral-turquoise" } else { "border-white/10 text-white/50 hover:text-parchment" })
                                    on:click=move |_| set_composer_mode.set("therapist".to_string())
                                >
                                    "Therapist"
                                </button>
                                <button
                                    class=move || format!("px-3 py-2 rounded-full text-xs uppercase tracking-[0.18em] border transition {}", if composer_mode.get() == "draft" { "bg-systemic-yellow/20 border-systemic-yellow/40 text-systemic-yellow" } else { "border-white/10 text-white/50 hover:text-parchment" })
                                    on:click=move |_| set_composer_mode.set("draft".to_string())
                                >
                                    "Draft"
                                </button>
                                <Show when=move || composer_mode.get() == "draft">
                                    <select
                                        class="bg-black/30 border border-white/10 rounded-full px-4 py-2 text-xs uppercase tracking-[0.18em] text-parchment focus:outline-none"
                                        prop:value=draft_relationship
                                        on:change=move |ev| {
                                            let value = event_target_value(&ev);
                                            set_draft_relationship.set(value.clone());
                                            set_profile_editor_slug.set(value);
                                        }
                                    >
                                        <For
                                            each=relationship_options
                                            key=|(slug, _)| slug.clone()
                                            children=move |(slug, label)| view! { <option value=slug>{label}</option> }
                                        />
                                    </select>
                                </Show>
                                <Show when=move || composer_mode.get() == "draft">
                                    <select
                                        class="bg-black/30 border border-white/10 rounded-full px-4 py-2 text-xs uppercase tracking-[0.18em] text-parchment focus:outline-none"
                                        prop:value=draft_intent
                                        on:change=move |ev| set_draft_intent.set(event_target_value(&ev))
                                    >
                                        <option value="repair">"Repair"</option>
                                        <option value="boundary">"Boundary"</option>
                                        <option value="request">"Request"</option>
                                        <option value="update">"Update"</option>
                                        <option value="check_in">"Check-in"</option>
                                    </select>
                                </Show>
                            </div>
                            <Show when=move || composer_mode.get() == "draft">
                                <div class="mx-3 rounded-2xl border border-systemic-yellow/20 bg-systemic-yellow/8 px-4 py-4 space-y-3">
                                    <div class="flex flex-wrap items-center justify-between gap-3">
                                        <div class="space-y-1">
                                            <div class="text-[10px] uppercase tracking-[0.22em] text-systemic-yellow/80">
                                                "Draft Context"
                                            </div>
                                            <div class="text-sm text-parchment">
                                                {move || {
                                                    if let Some(profile) = selected_draft_profile() {
                                                        format!(
                                                            "Using {} profile, recent session memory, broader history, and the mind map.",
                                                            profile.display_name
                                                        )
                                                    } else {
                                                        let selected_slug = draft_relationship.get();
                                                        let label = relationship_options()
                                                            .into_iter()
                                                            .find(|(slug, _)| slug == &selected_slug)
                                                            .map(|(_, label)| label)
                                                            .unwrap_or(selected_slug);
                                                        format!(
                                                            "Using {} context from recent session memory, broader history, and the mind map while auto-building the profile.",
                                                            label
                                                        )
                                                    }
                                                }}
                                            </div>
                                        </div>
                                        <button
                                            class="px-3 py-2 rounded-full border border-systemic-yellow/30 text-[10px] uppercase tracking-[0.18em] text-systemic-yellow hover:bg-systemic-yellow/10 transition"
                                            on:click=move |_| {
                                                set_profile_editor_slug.set(draft_relationship.get());
                                                set_is_profile_drawer_open.set(true);
                                            }
                                        >
                                            "Edit Profile"
                                        </button>
                                    </div>
                                    <div class="flex flex-wrap gap-2">
                                        {move || {
                                            if let Some(profile) = selected_draft_profile() {
                                                let mut chips = Vec::new();
                                                for goal in profile.goals.iter().take(2) {
                                                    chips.push(view! {
                                                        <span class="px-3 py-1 rounded-full bg-integral-turquoise/10 border border-integral-turquoise/20 text-[11px] text-integral-turquoise">
                                                            {format!("Goal: {}", goal)}
                                                        </span>
                                                    });
                                                }
                                                for boundary in profile.boundaries.iter().take(2) {
                                                    chips.push(view! {
                                                        <span class="px-3 py-1 rounded-full bg-white/5 border border-white/10 text-[11px] text-parchment/80">
                                                            {format!("Boundary: {}", boundary)}
                                                        </span>
                                                    });
                                                }
                                                for trigger in profile.triggers.iter().take(2) {
                                                    chips.push(view! {
                                                        <span class="px-3 py-1 rounded-full bg-systemic-yellow/10 border border-systemic-yellow/20 text-[11px] text-systemic-yellow">
                                                            {format!("Trigger: {}", trigger)}
                                                        </span>
                                                    });
                                                }
                                                if chips.is_empty() {
                                                    chips.push(view! {
                                                        <span class="px-3 py-1 rounded-full bg-white/5 border border-white/10 text-[11px] text-parchment/70">
                                                            "No saved goals, boundaries, or triggers yet."
                                                        </span>
                                                    });
                                                }
                                                chips.into_view()
                                            } else {
                                                view! {
                                                    <span class="px-3 py-1 rounded-full bg-white/5 border border-white/10 text-[11px] text-parchment/70">
                                                        "Profile is being inferred from conversation history."
                                                    </span>
                                                }
                                                .into_view()
                                            }
                                        }}
                                    </div>
                                </div>
                            </Show>
                            <Show when=move || composer_mode.get() == "therapist">
                                <div class="mx-3 flex flex-wrap gap-2">
                                    <button
                                        class="px-3 py-2 rounded-full border border-white/10 text-[11px] uppercase tracking-[0.18em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/30 hover:bg-white/5 transition"
                                        on:click={
                                            let submit_continue = submit_continue.clone();
                                            move |_| submit_continue("Continue from where we left off and stay with the same thread.".to_string())
                                        }
                                    >
                                        "Continue"
                                    </button>
                                    <button
                                        class="px-3 py-2 rounded-full border border-white/10 text-[11px] uppercase tracking-[0.18em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/30 hover:bg-white/5 transition"
                                        on:click={
                                            let submit_go_deeper = submit_go_deeper.clone();
                                            move |_| submit_go_deeper("Go deeper into the emotional pattern underneath this.".to_string())
                                        }
                                    >
                                        "Go Deeper"
                                    </button>
                                    <button
                                        class="px-3 py-2 rounded-full border border-white/10 text-[11px] uppercase tracking-[0.18em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/30 hover:bg-white/5 transition"
                                        on:click={
                                            let submit_be_direct = submit_be_direct.clone();
                                            move |_| submit_be_direct("Be more direct about what you think is happening.".to_string())
                                        }
                                    >
                                        "Be More Direct"
                                    </button>
                                    <button
                                        class="px-3 py-2 rounded-full border border-white/10 text-[11px] uppercase tracking-[0.18em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/30 hover:bg-white/5 transition"
                                        on:click={
                                            let submit_summarize_pattern = submit_summarize_pattern.clone();
                                            move |_| submit_summarize_pattern("Summarize the main pattern you see in me right now.".to_string())
                                        }
                                    >
                                        "Summarize Pattern"
                                    </button>
                                </div>
                            </Show>
                            <div class="flex items-center pr-2">
                            <input type="text" class="w-full bg-transparent border-none text-parchment placeholder-white/20 px-6 py-4 text-lg focus:outline-none font-urbanist tracking-wide" placeholder=move || if composer_mode.get() == "draft" { "What happened, and what do you want to say?" } else { "Type your thoughts..." } prop:value=chat_input on:input=move |ev| set_chat_input.set(event_target_value(&ev)) on:keydown=move |ev| { if ev.key() == "Enter" && !ev.shift_key() { on_submit_key(); } } />
                            <button class="group/btn relative flex items-center justify-center w-14 h-14 flex-shrink-0 rounded-full bg-gradient-to-br from-integral-turquoise to-systemic-yellow text-void-green shadow-lg hover:shadow-integral-turquoise/40 transition-all duration-300 transform hover:scale-105 active:scale-95" on:click=move |_| on_submit_click() disabled=move || is_loading.get()>
                                {move || if is_loading.get() { view!{<div class="w-6 h-6 border-2 border-void-green/50 border-t-void-green rounded-full animate-spin"></div>}.into_view() } else { view!{<svg class="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4.5 12h15m0 0l-6.75-6.75M19.5 12l-6.75 6.75" /></svg>}.into_view() }}
                            </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        {move || toast.get().map(|msg| view! { <div class="fixed bottom-6 right-6 bg-void-green/90 border border-integral-turquoise/40 text-parchment px-4 py-3 rounded-xl shadow-xl backdrop-blur z-[100]">{msg}</div> })}
    }
    .into_view()
    };

    app_view()
}

#[component]
fn MindMapPage() -> impl IntoView {
    let user_resource = use_context::<Resource<(), Option<User>>>().expect("User resource missing");
    let navigate = use_navigate();
    let is_authed = move || matches!(user_resource.get(), Some(Some(_)));
    let gate_active = move || !is_authed();

    // Auth Guard
    create_effect(move |_| {
        if user_resource.loading().get() {
            return;
        }
        if !matches!(user_resource.get(), Some(Some(_))) {
            navigate("/login", Default::default());
        }
    });

    let graph_resource = create_resource(
        move || user_resource.get(),
        |user| async move {
            if let Some(Some(u)) = user {
                get_patient_graph(u.id).await.unwrap_or_default()
            } else {
                PatientGraph::default()
            }
        },
    );

    let graph = move || graph_resource.get().unwrap_or_default();

    let refresh_graph = {
        let graph_resource = graph_resource.clone();
        move |_| graph_resource.refetch()
    };

    // Effect to render D3 graph
    create_effect(move |_| {
        let data = graph();
        if !data.nodes.is_empty() {
            set_timeout(
                move || {
                    if let Ok(js_value) = serde_wasm_bindgen::to_value(&data) {
                        render_mind_map_safe("mind-map-canvas", &js_value);
                    }
                },
                std::time::Duration::from_millis(50),
            );
        }
    });

    view! {
        <div class="relative min-h-screen">
            <div class=move || if gate_active() { "pointer-events-none select-none blur-[1px]" } else { "" }>
                <div class="min-h-screen pb-16 flex flex-col h-screen overflow-hidden">
            <header class="px-6 pt-6 pb-4 flex items-center justify-between shrink-0 z-10">
                <A href="/" class="px-4 py-2 rounded-full border border-white/10 text-xs uppercase tracking-[0.2em] text-sage-mist hover:text-parchment hover:border-integral-turquoise/40 transition bg-void-green/80 backdrop-blur">"Back to Chat"</A>
                <div class="text-xs uppercase tracking-[0.4em] text-white/40">"Mind Map"</div>
                <div class="flex gap-3">
                     <button class="px-4 py-2 rounded-full bg-void-green/80 border border-white/10 text-white/40 text-xs uppercase tracking-[0.2em] hover:text-parchment transition"
                        on:click=move |_| { let data = graph(); if let Ok(js_value) = serde_wasm_bindgen::to_value(&data) { render_mind_map_safe("mind-map-canvas", &js_value); } }>
                        "Reset View"
                    </button>
                    <button class="px-4 py-2 rounded-full bg-integral-turquoise/20 text-integral-turquoise text-xs uppercase tracking-[0.2em] border border-integral-turquoise/40 hover:bg-integral-turquoise/30 transition backdrop-blur"
                        on:click=refresh_graph>"Sync Graph"</button>
                </div>
            </header>
            <section class="flex-1 relative overflow-hidden px-4 pb-4">
                 <div class="absolute inset-0 z-0">
                    <div class="absolute top-[-20%] left-[-10%] w-[500px] h-[500px] bg-integral-turquoise/5 blur-[120px] rounded-full pointer-events-none"></div>
                    <div class="absolute bottom-[-20%] right-[-10%] w-[600px] h-[600px] bg-systemic-yellow/5 blur-[150px] rounded-full pointer-events-none"></div>
                 </div>
                 <Suspense fallback=move || view! { <div class="absolute inset-0 flex items-center justify-center text-white/30 tracking-widest uppercase text-xs animate-pulse">"Loading Rhizome..."</div> }>
                    {move || {
                        let g = graph();
                        if g.nodes.is_empty() {
                             view! {
                                <div class="absolute inset-0 flex items-center justify-center">
                                    <div class="text-center p-8 border border-dashed border-white/10 rounded-3xl bg-black/20 backdrop-blur-sm max-w-md">
                                        <h3 class="text-xl font-fraunces text-parchment mb-2">"The Canvas is Empty"</h3>
                                        <p class="text-sage-mist text-sm mb-4">"Start a session to begin mapping your psyche. Nodes appear as you speak."</p>
                                        <A href="/" class="text-integral-turquoise underline hover:text-white transition">"Return to Therapy"</A>
                                    </div>
                                </div>
                            }.into_view()
                        } else {
                            view! {
                                <div class="w-full h-full relative">
                                    <div class="absolute top-4 left-4 z-10 flex flex-col gap-2 pointer-events-none">
                                        <div class="bg-black/40 backdrop-blur-md border border-white/10 px-4 py-2 rounded-xl">
                                            <span class="block text-[10px] uppercase tracking-widest text-white/40">Nodes</span>
                                            <span class="font-fraunces text-xl text-parchment">{g.nodes.len()}</span>
                                        </div>
                                        <div class="bg-black/40 backdrop-blur-md border border-white/10 px-4 py-2 rounded-xl">
                                            <span class="block text-[10px] uppercase tracking-widest text-white/40">Edges</span>
                                            <span class="font-fraunces text-xl text-parchment">{g.edges.len()}</span>
                                        </div>
                                    </div>
                                    <div id="mind-map-canvas" class="w-full h-full rounded-3xl border border-white/5 bg-black/20 shadow-inner overflow-hidden"></div>
                                    <div class="absolute bottom-4 right-4 z-10 bg-black/60 backdrop-blur-xl border border-white/10 p-4 rounded-2xl shadow-2xl max-w-[200px]">
                                        <h4 class="text-[10px] uppercase tracking-widest text-white/40 mb-3 border-b border-white/10 pb-2">"Legend"</h4>
                                        <div class="grid grid-cols-2 gap-2 text-[10px] text-sage-mist">
                                            <div class="flex items-center gap-2"><span class="w-2 h-2 rounded-full bg-[#E9C46A]"></span>"Trigger"</div>
                                            <div class="flex items-center gap-2"><span class="w-2 h-2 rounded-full bg-[#2A9D8F]"></span>"Belief"</div>
                                            <div class="flex items-center gap-2"><span class="w-2 h-2 rounded-full bg-[#F2F0E9]"></span>"Emotion"</div>
                                            <div class="flex items-center gap-2"><span class="w-2 h-2 rounded-full bg-[#4A635D]"></span>"Somatic"</div>
                                            <div class="flex items-center gap-2"><span class="w-2 h-2 rounded-full bg-white border border-white/20"></span>"Pattern"</div>
                                        </div>
                                    </div>
                                </div>
                            }.into_view()
                        }
                    }}
                </Suspense>
            </section>
                </div>
            </div>
            <div class=move || if gate_active() { "absolute inset-0 flex items-center justify-center bg-void-green/90 text-parchment" } else { "hidden" }>
                <div class="text-center space-y-4">
                    <p class="text-sm uppercase tracking-[0.3em] text-white/40">"Session Required"</p>
                    <A href="/login" class="inline-flex items-center px-5 py-3 rounded-full border border-white/20 text-sm uppercase tracking-[0.2em] hover:border-integral-turquoise/60 hover:text-integral-turquoise transition">
                        "Go to login"
                    </A>
                </div>
            </div>
        </div>
    }
}

#[component]
fn Slider(
    label: &'static str,
    value: ReadSignal<i32>,
    set_value: WriteSignal<i32>,
    #[prop(optional)] min_label: &'static str,
    #[prop(optional)] max_label: &'static str,
) -> impl IntoView {
    view! {
        <div class="space-y-3">
            <div class="flex justify-between items-center text-sm">
                <span class="text-parchment font-medium">{label}</span>
                <span class="text-integral-turquoise font-mono text-xs">{move || value.get()}%</span>
            </div>
            <div class="relative h-2 bg-white/10 rounded-full">
                 <input type="range" min="0" max="100" class="absolute inset-0 w-full h-full opacity-0 cursor-pointer z-10"
                    prop:value=value on:input=move |ev| { let val = event_target_value(&ev).parse::<i32>().unwrap_or(0); set_value.set(val); } />
                <div class="absolute top-0 left-0 h-full bg-gradient-to-r from-integral-turquoise to-systemic-yellow rounded-full pointer-events-none transition-all duration-75" style=move || format!("width: {}%", value.get())></div>
                 <div class="absolute top-1/2 -translate-y-1/2 h-4 w-4 bg-parchment rounded-full shadow-lg pointer-events-none transition-all duration-75" style=move || format!("left: {}%", value.get())></div>
            </div>
            <div class="flex justify-between text-[10px] text-white/30 uppercase tracking-wider font-bold"><span>{min_label}</span><span>{max_label}</span></div>
        </div>
    }
}

#[component]
fn UserMenu() -> impl IntoView {
    let (is_open, set_is_open) = create_signal(false);
    let (error, set_error) = create_signal(None::<String>);
    let user_resource = use_context::<Resource<(), Option<User>>>().expect("User resource missing");

    let logout_action = create_action(move |_| async move {
        let _ = logout().await;
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    });

    let on_register_passkey = move |_| {
        spawn_local(async move {
            set_error.set(None);

            // 1. Start Registration
            let (req_id, challenge) = match start_passkey_register().await {
                Ok(res) => res,
                Err(e) => {
                    set_error.set(Some(e.to_string()));
                    return;
                }
            };

            // 2. JS Handshake
            let challenge_js = serde_wasm_bindgen::to_value(&challenge).unwrap();
            let cred_response = match registerPasskey(challenge_js).await {
                Ok(res) => res,
                Err(e) => {
                    set_error.set(Some(format!("Passkey error: {:?}", e)));
                    return;
                }
            };

            let response: webauthn_rs_proto::RegisterPublicKeyCredential =
                serde_wasm_bindgen::from_value(cred_response).unwrap();

            // 3. Finish Registration
            match finish_passkey_register(req_id, response).await {
                Ok(_) => {
                    if let Some(w) = web_sys::window() {
                        let _ = w.alert_with_message("Passkey registered successfully!");
                    }
                }
                Err(e) => set_error.set(Some(e.to_string())),
            }
        });
    };

    view! {
        <div class="relative">
            {move || error.get().map(|e| view! { <div class="absolute bottom-full mb-2 w-64 bg-red-900/90 text-red-200 text-xs p-2 rounded-lg">{e}</div> })}

            <button class="flex items-center w-full space-x-3 p-2 rounded-xl hover:bg-white/5 transition-colors group" on:click=move |_| set_is_open.update(|n| *n = !*n)>
                <div class="w-10 h-10 rounded-full bg-gradient-to-tr from-integral-turquoise to-systemic-yellow p-[2px]">
                    <div class="w-full h-full rounded-full bg-void-green flex items-center justify-center">
                        <span class="text-sm font-bold text-parchment">
                            {move || user_resource.get().flatten().map(|u| u.username.chars().next().unwrap_or('?').to_string().to_uppercase()).unwrap_or("?".into())}
                        </span>
                    </div>
                </div>
                <div class="flex-1 text-left">
                    <div class="text-sm font-bold text-parchment">
                        {move || user_resource.get().flatten().map(|u| u.username).unwrap_or("Guest".into())}
                    </div>
                    <div class="text-xs text-white/40 group-hover:text-integral-turquoise transition-colors">"View Profile"</div>
                </div>
                <svg class="w-5 h-5 text-white/30" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M8.25 15L12 18.75 15.75 15m-7.5-6L12 5.25 15.75 9" /></svg>
            </button>

            <div class=move || format!("absolute bottom-full left-0 w-full mb-2 bg-void-green border border-white/10 rounded-xl shadow-xl overflow-hidden transition-all duration-200 origin-bottom {}", if is_open.get() { "opacity-100 scale-100 translate-y-0" } else { "opacity-0 scale-95 translate-y-2 pointer-events-none" })>
                <a href="#" class="block px-4 py-3 text-sm text-parchment hover:bg-white/5 flex items-center space-x-2">
                    <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" /></svg>
                    <span>"Account Settings"</span>
                </a>
                <button on:click=on_register_passkey class="block w-full text-left px-4 py-3 text-sm text-integral-turquoise hover:bg-integral-turquoise/10 flex items-center space-x-2">
                    <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" /></svg>
                    <span>"Register Passkey"</span>
                </button>
                <button on:click=move |_| logout_action.dispatch(()) class="block w-full text-left px-4 py-3 text-sm text-red-400 hover:bg-red-500/10 flex items-center space-x-2">
                    <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15M12 9l-3 3m0 0l3 3m-3-3h12.75" /></svg>
                    <span>"Log Out"</span>
                </button>
            </div>
        </div>
    }
}
