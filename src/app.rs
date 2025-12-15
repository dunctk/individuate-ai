use leptos::*;
use leptos_meta::*;
use leptos_router::*;

#[derive(Clone, Debug, PartialEq)]
struct Session {
    id: usize,
    title: String,
    date: String,
    preview: String,
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
    
    // Settings State
    let (accountability, set_accountability) = create_signal(50);
    let (spirituality, set_spirituality) = create_signal(30);
    let (directness, set_directness) = create_signal(70);

    // Session Data
    let sessions = vec![
        Session { id: 1, title: "Recurring Anxiety Loops".to_string(), date: "Today".to_string(), preview: "Exploring the root cause of the Sunday scaries...".to_string() },
        Session { id: 2, title: "Dream Analysis: The Tower".to_string(), date: "Yesterday".to_string(), preview: "The falling tower archetype and what it means for my career...".to_string() },
        Session { id: 3, title: "Shadow Work: Anger".to_string(), date: "Dec 12".to_string(), preview: "Why do I get triggered when...".to_string() },
        Session { id: 4, title: "Integration Phase".to_string(), date: "Dec 10".to_string(), preview: "Connecting the dots between...".to_string() },
    ];
    let (search_query, set_search_query) = create_signal(String::new());

    let filtered_sessions = move || {
        let query = search_query.get().to_lowercase();
        if query.is_empty() {
            sessions.clone()
        } else {
            sessions.iter()
                .filter(|s| s.title.to_lowercase().contains(&query) || s.preview.to_lowercase().contains(&query))
                .cloned()
                .collect()
        }
    };

    let on_submit = move |_| {
        set_is_loading.set(true);
        // Simulate delay
        set_timeout(move || set_is_loading.set(false), std::time::Duration::from_secs(2));
    };

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
                    <For
                        each=filtered_sessions
                        key=|session| session.id
                        children=move |session| {
                            view! {
                                <div class="group p-4 rounded-xl hover:bg-white/5 cursor-pointer transition-all border border-transparent hover:border-white/5">
                                    <div class="flex justify-between items-baseline mb-1">
                                        <h4 class="font-bold text-sm text-parchment group-hover:text-integral-turquoise transition-colors">{session.title}</h4>
                                        <span class="text-xs text-white/30 font-mono">{session.date}</span>
                                    </div>
                                    <p class="text-xs text-sage-mist line-clamp-2">{session.preview}</p>
                                </div>
                            }
                        }
                    />
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
                        // Welcome Message
                        <div class="flex flex-col items-start space-y-2 animate-fade-in-up">
                            <div class="text-xs font-bold text-integral-turquoise tracking-[0.2em] uppercase pl-2">"Therapist"</div>
                            <div class="bg-sage-mist/10 p-6 rounded-2xl rounded-tl-sm border border-white/5 backdrop-blur-md shadow-lg">
                                <p class="text-lg leading-relaxed font-light">
                                    "Welcome. I am your Jungian guide. The path to individuation is a spiral, not a straight line. What brings you to the garden today?"
                                </p>
                            </div>
                        </div>

                        // User Message Example
                        <div class="flex flex-col items-end space-y-2 opacity-60">
                             <div class="text-xs font-bold text-systemic-yellow tracking-[0.2em] uppercase pr-2">"You"</div>
                             <div class="bg-white/5 p-6 rounded-2xl rounded-tr-sm border border-white/5 backdrop-blur-md">
                                <p class="text-lg leading-relaxed font-light">
                                    "I feel stuck in a loop. I keep making the same mistakes."
                                </p>
                            </div>
                        </div>
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
                                    if ev.key() == "Enter" {
                                        on_submit(());
                                    }
                                }
                            />
                            
                            <button 
                                class="group/btn relative flex items-center justify-center w-14 h-14 flex-shrink-0 rounded-full bg-gradient-to-br from-integral-turquoise to-systemic-yellow text-void-green shadow-lg hover:shadow-integral-turquoise/40 transition-all duration-300 transform hover:scale-105 active:scale-95"
                                on:click=move |_| on_submit(())
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
