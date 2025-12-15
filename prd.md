# IndividuateAI.com PRD

A rust web app built in Leptos with sqlite db. It is a therapist using rig.rs as the ai agent framework powering it. 

It has a powerful memory feature, seamlessly remembering all the important details about each user. 

Default to using the latest X AI Grok model (set by GROK_MODEL in .env / os env with GROK_API_KEY as api key)

Vector Memory: Local SQLite (Plain Text)

Library: rig-sqlite + sqlite-vec (extension).

Storage: A standard .sqlite file on the local disk.

Performance: Uses Memory Mapping (mmap) for maximum read speed (which encryption would have disabled).

## Architecture Diagram

User Query -> Rust Agent

Rust Agent -> Local SQLite (Search for context)

Local SQLite -> Returns raw text chunks (No decryption step)

Rust Agent -> xAI API (Prompt + Context)

xAI API -> Rust Agent (Response)

## Debugging Setup 

Here is the "AI-Friendly" Debugging Setup for Leptos.

1. The "Panic Portal" (Essential)

First, ensure that if your Rust code crashes in the browser, the panic message is screamingly obvious in the console (and ready to be piped).

Add to Cargo.toml:

Ini, TOML
[dependencies]
console_error_panic_hook = "0.1"
Add to src/main.rs (or lib.rs):

Rust
pub fn main() {
    // 1. Hook panics to the console immediately
    console_error_panic_hook::set_once();
    
    // ... rest of your app
    mount_to_body(|| view! { <App/> })
}
Now, at least the errors exist in the JS console. But the agent still can't see them.

2. The "Mirror" (Server Logging)

To let the AI see browser errors, you need to stream browser logs to your server's terminal.

The Strategy: Create a simple Server Function that acts as a remote logger.

Step A: Define the Logger (src/logging.rs)

Rust
use leptos::*;

#[server(LogToServer, "/api")]
pub async fn log_to_server(level: String, msg: String) -> Result<(), ServerFnError> {
    // This prints to your TERMINAL, where Gemini can see it
    println!("[CLIENT-{}] {}", level, msg); 
    Ok(())
}

// A macro to make using it easy
#[macro_export]
macro_rules! ai_log {
    ($($t:tt)*) => {
        leptos::spawn_local(async move {
            let _ = $crate::log_to_server("INFO".to_string(), format!($($t)*)).await;
        });
    }
}

#[macro_export]
macro_rules! ai_error {
    ($($t:tt)*) => {
        leptos::spawn_local(async move {
            let _ = $crate::log_to_server("ERROR".to_string(), format!($($t)*)).await;
        });
    }
}
Step B: Use it in your components

Rust
#[component]
fn Counter() -> impl IntoView {
    let (value, set_value) = create_signal(0);

    let on_click = move |_| {
        set_value.update(|n| *n += 1);
        // This will show up in your terminal running 'cargo leptos watch'
        ai_log!("Counter clicked! New value: {}", value.get() + 1);
    };

    // ...
}
3. The "Headless" Workflow (For AI Agents)

If you want the AI to fix a bug without you manually clicking things, you should use Headless Browser Tests. This allows the AI to run a command, see the failure in the terminal, and fix the code loop.

1. Install the runner:

Bash
cargo install wasm-pack
2. Create a test file (tests/web.rs):

Rust
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn test_counter_increments() {
    // The AI can run this and see the output in the terminal
    let app = leptos::view! { <Counter/> };
    // ... logic to click and assert ...
}
3. The Command for the Agent: Tell Gemini: "Run the tests using this command to check for errors."

Bash
wasm-pack test --headless --firefox
--headless: Runs invisible browser.

stdout: All console logs and panics are piped directly to the terminal.

Summary of the "AI-Ready" Stack

Feature	Tool	Benefit for AI Agent
Crash Reporting	console_error_panic_hook	Makes crashes "catchable" strings.
Live Logs	log_to_server (Server Fn)	Teleports browser logs to the terminal.
Verification	wasm-pack test --headless	Allows AI to verify fixes without human eyes.
