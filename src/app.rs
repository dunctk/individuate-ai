use crate::agent::{agent_chat, create_session, get_chat_history, get_sessions};
use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use std::collections::HashMap;
use std::rc::Rc;

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

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/individuateai.css"/>
        <Title text="IndividuateAI"/>

        <Router>
            <main class="min-h-screen bg-void-green text-parchment font-urbanist selection:bg-integral-turquoise selection:text-void-green overflow-x-hidden">
                <Routes>
                    <Route path="" view=HomePage/>
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    // App State
    let (chat_input, set_chat_input) = create_signal(String::new());
    let (is_loading, set_is_loading) = create_signal(false);
    let (is_sidebar_open, set_is_sidebar_open) = create_signal(false);
    
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

    // Settings State
    let (accountability, set_accountability) = create_signal(50);
    let (spirituality, set_spirituality) = create_signal(30);
    let (directness, set_directness) = create_signal(70);

    // Resources
    let sessions_resource = create_resource(|| (), |_| async move { get_sessions().await.unwrap_or_default() });
    
    let (search_query, set_search_query) = create_signal(String::new());
    
    // Selected Session (String UUID)
    let (selected_session, set_selected_session) = create_signal(None::<String>);

    // Chat History Resource - fetches when session changes
    let history_resource = create_local_resource(
        move || selected_session.get(),
        |session_id| async move {
            if let Some(id) = session_id {
                get_chat_history(id).await.unwrap_or_default()
            } else {
                vec![]
            }
        }
    );

    // Conversations Map: Stores hydrated messages
    // We update this map when history_resource resolves OR when new messages are sent
    let conversations = create_rw_signal(HashMap::<String, Vec<ChatMessage>>::new());

    // Effect to hydrate conversations from history_resource
    create_effect(move |_| {
        let session_id = selected_session.get();
        if let Some(id) = session_id {
            if let Some(logs) = history_resource.get() {
                 conversations.update(|map| {
                     // Only insert if empty or we want to overwrite?
                     // Let's overwrite to ensure sync, unless we have pending optimistic updates?
                     // For simplicity, overwrite from DB on load.
                     if !map.contains_key(&id) || map.get(&id).map(|v| v.is_empty()).unwrap_or(true) {
                         let msgs: Vec<ChatMessage> = logs.into_iter().map(|log| ChatMessage {
                             role: if log.role == "user" { ChatRole::User } else { ChatRole::Assistant },
                             text: create_rw_signal(log.content)
                         }).collect();
                         map.insert(id, msgs);
                     }
                 });
            }
        }
    });

    let current_messages = move || {
        if let Some(id) = selected_session.get() {
            conversations.with(|map| {
                map.get(&id)
                   .cloned()
                   .unwrap_or_else(Vec::new)
            })
        } else {
            // Default "New Chat" view if no session selected?
            // Or maybe a welcome message?
             vec![ChatMessage {
                role: ChatRole::Assistant,
                text: create_rw_signal("Select a session or create a new one to begin.".to_string()),
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
    
    // Actions
    let create_new_chat = create_action(move |title: &String| {
        let title = title.clone();
        async move {
            match create_session(title).await {
                Ok(session) => {
                    leptos::logging::log!("Session created: {:?}", session);
                    sessions_resource.refetch();
                    set_selected_session.set(Some(session.id.clone()));
                    // Initialize empty conversation in map
                    conversations.update(|map| {
                        map.insert(session.id, vec![ChatMessage {
                            role: ChatRole::Assistant,
                            text: create_rw_signal("Welcome to your new session.".to_string()),
                        }]);
                    });
                }
                Err(e) => leptos::logging::error!("Failed to create session: {}", e),
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

        Rc::new(move |session_id: String, trimmed: String| {
            leptos::logging::log!("Submitting to session: {}", session_id);

            // Optimistic Update
            conversations.update(|map| {
                let entry = map.entry(session_id.clone()).or_insert_with(Vec::new);
                entry.push(ChatMessage {
                    role: ChatRole::User,
                    text: create_rw_signal(trimmed.clone()),
                });
                entry.push(ChatMessage {
                    role: ChatRole::Assistant,
                    text: create_rw_signal(String::new()), // Placeholder for stream
                });
            });

            set_chat_input.set(String::new());
            set_is_loading.set(true);

            let acc_val = accountability.get_untracked();
            let spir_val = spirituality.get_untracked();
            let dir_val = directness.get_untracked();
            let conversations = conversations.clone();
            let set_is_loading = set_is_loading.clone();
            let set_toast = set_toast.clone();

            let prompt = format!(
                "User input: {trimmed}\n\nControls -> accountability: {acc_val}, spirituality: {spir_val}, directness: {dir_val}. Use these to tune tone and firmness. Keep response concise."
            );

            #[cfg(feature = "hydrate")]
            {
                use wasm_bindgen::{closure::Closure, JsCast};
                use web_sys::{Event, EventSource, MessageEvent};

                let url = format!(
                    "/api/agent-stream?session_id={}&prompt={}",
                    session_id,
                    urlencoding::encode(&prompt)
                );

                match EventSource::new(&url) {
                    Ok(es) => {
                        let es_clone = es.clone();
                        let es_err = es.clone();
                        let session_id_clone = session_id.clone();

                        let on_message = Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new({
                            let conversations = conversations.clone();
                            let set_is_loading = set_is_loading.clone();
                            let session_id = session_id.clone();
                            move |event: MessageEvent| {
                                if let Some(data) = event.data().as_string() {
                                    if data.starts_with("[error:") {
                                        set_toast.set(Some(data.clone()));
                                    }
                                    if data == "[DONE]" {
                                        set_is_loading.set(false);
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
                            move |_event: Event| {
                                set_is_loading.set(false);
                                set_toast.set(Some("Stream error from agent".to_string()));
                                es_err.close();

                                // fallback
                                let conversations = conversations.clone();
                                let prompt_fallback = prompt_clone.clone();
                                let session_id = session_id.clone();
                                spawn_local(async move {
                                    let reply = agent_chat(prompt_fallback, session_id.clone()).await;
                                    match reply {
                                        Ok(text) => conversations.update(|map| {
                                            if let Some(entry) = map.get_mut(&session_id) {
                                                if let Some(last) = entry.last() {
                                                    if let ChatRole::Assistant = last.role {
                                                        last.text.set(text);
                                                    }
                                                }
                                            }
                                        }),
                                        Err(err) => {
                                            set_toast.set(Some(format!("(error fallback) {}", err)))
                                        }
                                    }
                                });
                            }
                        }));
                        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
                        on_error.forget();
                    }
                    Err(err) => {
                        set_is_loading.set(false);
                        set_toast.set(Some(format!("Failed to start stream: {:?}", err)));
                    }
                }
            }

            #[cfg(not(feature = "hydrate"))]
            {
                spawn_local(async move {
                    let reply = agent_chat(prompt, session_id.clone()).await;
                    match reply {
                        Ok(text) => conversations.update(|map| {
                            if let Some(entry) = map.get_mut(&session_id) {
                                if let Some(last) = entry.last() {
                                    if let ChatRole::Assistant = last.role {
                                        last.text.set(text);
                                    }
                                }
                            }
                        }),
                        Err(err) => { set_toast.set(Some(format!("(error) {}", err))); }
                    }
                    set_is_loading.set(false);
                });
            }
        })
    };

    let on_submit = {
        let chat_input = chat_input.clone();
        let set_is_loading = set_is_loading.clone();
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
                leptos::logging::log!(
                    "Submit skipped: empty={} loading={}",
                    trimmed.is_empty(),
                    is_loading.get_untracked()
                );
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
            let set_is_loading = set_is_loading.clone();
            let set_toast = set_toast.clone();
            let submit_with_session = submit_with_session.clone();
            spawn_local(async move {
                match create_session("New Session".to_string()).await {
                    Ok(session) => {
                        sessions_resource.refetch();
                        set_selected_session.set(Some(session.id.clone()));
                        conversations.update(|map| {
                            map.insert(session.id.clone(), vec![ChatMessage {
                                role: ChatRole::Assistant,
                                text: create_rw_signal("Welcome to your new session.".to_string()),
                            }]);
                        });
                        submit_with_session(session.id, trimmed);
                    }
                    Err(e) => {
                        leptos::logging::error!("Failed to create session: {}", e);
                        set_toast.set(Some("Failed to create a new session.".to_string()));
                        set_is_loading.set(false);
                    }
                }
            });
        })
    };

    let on_submit_key = on_submit.clone();
    let on_submit_click = on_submit.clone();

    view! {
        <div class="flex h-screen relative overflow-hidden">

            // --- SIDE PANEL (Drawer) ---
            <div
                class=move || {
                    format!(
                        "fixed inset-y-0 left-0 z-50 w-80 bg-void-green/95 backdrop-blur-xl border-r border-white/5 shadow-2xl transform transition-transform duration-500 ease-out flex flex-col {}",
                        if is_sidebar_open.get() { "translate-x-0" } else { "-translate-x-full" }
                    )
                }
            >
                // 1. Search Header
                <div class="p-6 border-b border-white/5 space-y-4">
                    <div class="flex items-center justify-between">
                         <h2 class="font-fraunces text-xl text-parchment">"History"</h2>
                         <button
                            class="p-2 hover:bg-white/5 rounded-full transition-colors"
                            on:click=move |_| set_is_sidebar_open.set(false)
                         >
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-6 h-6">
                              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                            </svg>
                         </button>
                    </div>
                     <button
                        class="w-full bg-integral-turquoise/10 hover:bg-integral-turquoise/20 text-integral-turquoise font-bold py-2 rounded-xl transition-colors mb-2"
                        on:click=move |_| create_new_chat.dispatch("New Session".to_string())
                     >
                        "+ New Session"
                     </button>
                    <div class="relative">
                        <input
                            type="text"
                            class="w-full bg-white/5 border border-white/10 rounded-xl py-2 pl-10 pr-4 text-sm focus:outline-none focus:border-integral-turquoise/50 transition-colors placeholder-white/20"
                            placeholder="Search sessions..."
                            on:input=move |ev| set_search_query.set(event_target_value(&ev))
                        />
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4 absolute left-3 top-3 text-white/30">
                          <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
                        </svg>
                    </div>
                </div>

                // 2. Settings Sliders
                <div class="px-6 py-6 space-y-6 border-b border-white/5 bg-black/20">
                    <h3 class="text-xs font-bold text-sage-mist tracking-widest uppercase">"Model Parameters"</h3>

                    <Slider
                        label="Accountability"
                        value=accountability
                        set_value=set_accountability
                        min_label="Gentle"
                        max_label="Ruthless"
                    />
                    <Slider
                        label="Spirituality (Teal)"
                        value=spirituality
                        set_value=set_spirituality
                        min_label="Grounded"
                        max_label="Transcendent"
                    />
                     <Slider
                        label="Directness"
                        value=directness
                        set_value=set_directness
                        min_label="Soft"
                        max_label="Blunt"
                    />
                </div>

                // 3. Session List
                <div class="flex-1 overflow-y-auto p-4 space-y-2 custom-scrollbar">
                    <Suspense fallback=move || view! { <p class="text-center text-white/20 p-4">"Loading..."</p> }>
                        <For
                            each=filtered_sessions
                            key=|session| session.id.clone()
                            children=move |session| {
                                let set_selected_session = set_selected_session.clone();
                                let session_id_active = session.id.clone();
                                let session_id_click = session.id.clone();
                                let is_active = move || selected_session.get() == Some(session_id_active.clone());
                                view! {
                                    <div
                                        class=move || format!(
                                            "group p-4 rounded-xl cursor-pointer transition-all border {}",
                                            if is_active() { "bg-white/10 border-integral-turquoise/30" } else { "hover:bg-white/5 border-transparent hover:border-white/5" }
                                        )
                                        on:click=move |_| set_selected_session.set(Some(session_id_click.clone()))
                                    >
                                        <div class="flex justify-between items-baseline mb-1">
                                            <h4 class="font-bold text-sm text-parchment group-hover:text-integral-turquoise transition-colors">{session.title}</h4>
                                            <span class="text-xs text-white/30 font-mono">{session.date}</span>
                                        </div>
                                        <p class="text-xs text-sage-mist line-clamp-2">{session.preview}</p>
                                    </div>
                                }
                            }
                        />
                    </Suspense>
                </div>

                // 4. User Footer
                <div class="p-4 border-t border-white/5 bg-void-green">
                    <UserMenu />
                </div>
            </div>

            // Overlay for mobile when sidebar is open
            <div
                class=move || {
                    format!(
                        "fixed inset-0 bg-black/60 backdrop-blur-sm z-40 transition-opacity duration-500 {}",
                        if is_sidebar_open.get() { "opacity-100 pointer-events-auto" } else { "opacity-0 pointer-events-none" }
                    )
                }
                on:click=move |_| set_is_sidebar_open.set(false)
            ></div>


            // --- MAIN CONTENT ---
            <div class="flex-1 flex flex-col h-full relative w-full transition-all duration-500">

                // Header
                <header class="p-6 sticky top-0 z-10 flex items-center justify-between">
                    <button
                        class="p-2 -ml-2 text-sage-mist hover:text-parchment transition-colors rounded-full hover:bg-white/5"
                        on:click=move |_| set_is_sidebar_open.set(true)
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-8 h-8">
                          <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h16.5" />
                        </svg>
                    </button>

                    <h1 class="text-2xl font-fraunces text-center bg-clip-text text-transparent bg-gradient-to-r from-parchment to-sage-mist drop-shadow-sm absolute left-1/2 -translate-x-1/2">
                        "IndividuateAI"
                    </h1>

                    <div class="w-8"></div> // Spacer for balance
                </header>

                // Chat Area
                <div class="flex-1 overflow-y-auto px-4 pb-40 w-full max-w-3xl mx-auto custom-scrollbar">
                     <div class="space-y-8 py-10">
                        <For
                            each=move || { current_messages().into_iter().enumerate().collect::<Vec<_>>() }
                            key=|(idx, _)| *idx
                            children=move |(_, msg)| {
                                let is_assistant = matches!(msg.role, ChatRole::Assistant);
                                let alignment = if is_assistant { "items-start" } else { "items-end" };
                                let label = if is_assistant { "Therapist" } else { "You" };
                                let label_class = if is_assistant { "text-integral-turquoise" } else { "text-systemic-yellow" };
                                let bubble_classes = if is_assistant {
                                    "bg-sage-mist/10 p-6 rounded-2xl rounded-tl-sm border border-white/5 backdrop-blur-md shadow-lg"
                                } else {
                                    "bg-white/5 p-6 rounded-2xl rounded-tr-sm border border-white/5 backdrop-blur-md"
                                };
                                let label_pad = if is_assistant { "pl-2" } else { "pr-2" };

                                view! {
                                    <div class=format!("flex flex-col {alignment} space-y-2 animate-fade-in-up")>
                                        <div class=format!("text-xs font-bold tracking-[0.2em] uppercase {} {}", label_class, label_pad)>
                                            {label}
                                        </div>
                                        <div class=bubble_classes>
                                            <p class="text-lg leading-relaxed font-light whitespace-pre-wrap">{move || msg.text.get()}</p>
                                        </div>
                                    </div>
                                }
                            }
                        />
                     </div>
                </div>

                // Input Area
                <div class="absolute bottom-0 left-0 right-0 p-4 pb-8 z-20 bg-gradient-to-t from-void-green via-void-green/90 to-transparent">
                    <div class="max-w-3xl mx-auto relative group">
                        // Glass Background
                        <div class="absolute inset-0 bg-void-green/60 backdrop-blur-xl rounded-3xl border border-white/10 shadow-2xl transition-all duration-300 group-hover:bg-void-green/80"></div>

                        <div class="relative flex items-center p-2 pr-2">
                            <input
                                type="text"
                                class="w-full bg-transparent border-none text-parchment placeholder-white/20 px-6 py-4 text-lg focus:outline-none font-urbanist tracking-wide"
                                placeholder="Type your thoughts..."
                                prop:value=chat_input
                                on:input=move |ev| set_chat_input.set(event_target_value(&ev))
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" && !ev.shift_key() {
                                        let submit = on_submit_key.clone();
                                        submit();
                                    }
                                }
                            />

                            <button
                                class="group/btn relative flex items-center justify-center w-14 h-14 flex-shrink-0 rounded-full bg-gradient-to-br from-integral-turquoise to-systemic-yellow text-void-green shadow-lg hover:shadow-integral-turquoise/40 transition-all duration-300 transform hover:scale-105 active:scale-95"
                                on:click=move |_| {
                                    let submit = on_submit_click.clone();
                                    submit();
                                }
                                disabled=move || is_loading.get()
                            >
                                {move || if is_loading.get() {
                                    view! {
                                        <div class="w-6 h-6 border-2 border-void-green/50 border-t-void-green rounded-full animate-spin"></div>
                                    }.into_view()
                                } else {
                                    view! {
                                        <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor" class="w-6 h-6 transition-transform group-hover/btn:translate-x-1">
                                            <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12h15m0 0l-6.75-6.75M19.5 12l-6.75 6.75" />
                                        </svg>
                                    }.into_view()
                                }}
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        {move || {
            toast.get().map(|msg| {
                view! {
                    <div class="fixed bottom-6 right-6 bg-void-green/90 border border-integral-turquoise/40 text-parchment px-4 py-3 rounded-xl shadow-xl backdrop-blur z-[100]">
                        {msg}
                    </div>
                }
            })
        }}
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
                 <input
                    type="range"
                    min="0"
                    max="100"
                    class="absolute inset-0 w-full h-full opacity-0 cursor-pointer z-10"
                    prop:value=value
                    on:input=move |ev| {
                        let val = event_target_value(&ev).parse::<i32>().unwrap_or(0);
                        set_value.set(val);
                    }
                />
                <div
                    class="absolute top-0 left-0 h-full bg-gradient-to-r from-integral-turquoise to-systemic-yellow rounded-full pointer-events-none transition-all duration-75"
                    style=move || format!("width: {}%", value.get())
                ></div>
                 <div
                    class="absolute top-1/2 -translate-y-1/2 h-4 w-4 bg-parchment rounded-full shadow-lg pointer-events-none transition-all duration-75"
                    style=move || format!("left: {}%", value.get())
                ></div>
            </div>
            <div class="flex justify-between text-[10px] text-white/30 uppercase tracking-wider font-bold">
                <span>{min_label}</span>
                <span>{max_label}</span>
            </div>
        </div>
    }
}

#[component]
fn UserMenu() -> impl IntoView {
    let (is_open, set_is_open) = create_signal(false);

    view! {
        <div class="relative">
            <button
                class="flex items-center w-full space-x-3 p-2 rounded-xl hover:bg-white/5 transition-colors group"
                on:click=move |_| set_is_open.update(|n| *n = !*n)
            >
                <div class="w-10 h-10 rounded-full bg-gradient-to-tr from-integral-turquoise to-systemic-yellow p-[2px]">
                    <div class="w-full h-full rounded-full bg-void-green flex items-center justify-center">
                        <span class="text-sm font-bold text-parchment">"DT"</span>
                    </div>
                </div>
                <div class="flex-1 text-left">
                    <div class="text-sm font-bold text-parchment">"Duncan"</div>
                    <div class="text-xs text-white/40 group-hover:text-integral-turquoise transition-colors">"View Profile"</div>
                </div>
                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-5 h-5 text-white/30">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M8.25 15L12 18.75 15.75 15m-7.5-6L12 5.25 15.75 9" />
                </svg>
            </button>

            <div
                class=move || {
                    format!(
                        "absolute bottom-full left-0 w-full mb-2 bg-void-green border border-white/10 rounded-xl shadow-xl overflow-hidden transition-all duration-200 origin-bottom {}",
                        if is_open.get() { "opacity-100 scale-100 translate-y-0" } else { "opacity-0 scale-95 translate-y-2 pointer-events-none" }
                    )
                }
            >
                <a href="#" class="block px-4 py-3 text-sm text-parchment hover:bg-white/5 flex items-center space-x-2">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
                    </svg>
                    <span>"Account Settings"</span>
                </a>
                <a href="#" class="block px-4 py-3 text-sm text-red-400 hover:bg-red-500/10 flex items-center space-x-2">
                    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15M12 9l-3 3m0 0l3 3m-3-3h12.75" />
                    </svg>
                    <span>"Log Out"</span>
                </a>
            </div>
        </div>
    }
}
