use leptos::*;
use leptos_meta::*;
use leptos_router::*;

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
    let (chat_input, set_chat_input) = create_signal(String::new());
    let (is_loading, set_is_loading) = create_signal(false);

    let on_submit = move |_| {
        set_is_loading.set(true);
        // TODO: backend logic
        // Simulate delay
        set_timeout(move || set_is_loading.set(false), std::time::Duration::from_secs(2));
    };

    view! {
        <div class="flex flex-col min-h-screen relative bg-[url('/img/noise.png')]">
            // Header
            <header class="p-6 sticky top-0 z-10 backdrop-blur-sm bg-void-green/80 border-b border-white/5 transition-all duration-500">
                <h1 class="text-3xl font-fraunces text-center bg-clip-text text-transparent bg-gradient-to-r from-parchment to-sage-mist drop-shadow-sm">
                    "IndividuateAI"
                </h1>
            </header>

            // Chat Area (Placeholder content)
            <div class="flex-1 flex flex-col items-center justify-center p-4 space-y-8 pb-32 w-full max-w-4xl mx-auto">
                 <div class="w-full space-y-8">
                    // Welcome Message
                    <div class="flex flex-col items-start space-y-2 animate-fade-in-up">
                        <div class="text-xs font-bold text-integral-turquoise tracking-[0.2em] uppercase pl-2">"Therapist"</div>
                        <div class="bg-sage-mist/10 p-6 rounded-2xl rounded-tl-sm border border-white/5 backdrop-blur-md shadow-lg max-w-2xl">
                            <p class="text-lg leading-relaxed font-light">
                                "Welcome. I am your Jungian guide. The path to individuation is a spiral, not a straight line. What brings you to the garden today?"
                            </p>
                        </div>
                    </div>

                    // User Message Example
                    <div class="flex flex-col items-end space-y-2 opacity-60">
                         <div class="text-xs font-bold text-systemic-yellow tracking-[0.2em] uppercase pr-2">"You"</div>
                         <div class="bg-white/5 p-6 rounded-2xl rounded-tr-sm border border-white/5 backdrop-blur-md max-w-2xl">
                            <p class="text-lg leading-relaxed font-light">
                                "I feel stuck in a loop. I keep making the same mistakes."
                            </p>
                        </div>
                    </div>
                 </div>
            </div>

            // Input Area
            <div class="fixed bottom-0 left-0 right-0 p-4 pb-8 z-20">
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
                            
                            // "Seed" pulsing effect
                            <div class="absolute inset-0 rounded-full bg-white/40 animate-ping opacity-0 group-hover/btn:opacity-100 duration-1000 pointer-events-none"></div>
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}
