# IndividuateAI Context

## Project Overview
**IndividuateAI** is a Rust-based web application acting as an AI Therapist. It utilizes the **Rig.rs** agent framework to power a "Jungian" therapist persona that leverages long-term vector memory to retain user context.

**Status:** 🛠 Active prototype.

## Technology Stack
*   **Language:** Rust 🦀
*   **Frontend/Full-stack Framework:** [Leptos](https://leptos.dev/) (WebAssembly)
*   **AI Agent Framework:** [Rig.rs](https://github.com/0xPlaygrounds/rig)
*   **LLM Provider:** xAI Grok (Latest Model via API)
*   **Database & Memory:**
    *   **Primary Storage:** SQLite (Local `.sqlite` file)
    *   **Vector Search:** `sqlite-vec` extension with `rig-sqlite`
    *   **Performance:** Memory Mapping (mmap) enabled; Encryption disabled for speed.

## Architecture
1.  **User Interface:** Leptos-based web UI.
2.  **Agent Logic:** Rust backend (Rig.rs) processes queries.
3.  **Memory Retrieval:** Agent searches local SQLite vector store for relevant context.
4.  **Inference:** Agent sends Prompt + Context to xAI Grok API.
5.  **Response:** Agent streams response back to UI.

## Mind Map Graph (Persistent)
To avoid context overflow, the "mind map" lives as a persistent graph in SQLite instead of being reconstructed every session.

- **Source of Truth:** `patient_graphs` table stores a JSON blob (`PatientGraph`) with `nodes` and `edges`.
- **Rig Tools:** `read_mind_map` (read) and `update_mind_map` (write) are implemented as `rig::tool` tools.
- **Delta Extraction:** A background `rig::extractor` produces incremental changes (`ConversationGraphDelta`) after each exchange.
- **API + UI:** `/api/graph/:user_id` returns the current graph; `/mind-map` renders it in the UI using an interactive D3.js force-directed graph.

## UI/Design System: "Organic Integral"
*   **Philosophy:** "The Digital Greenhouse" – A botanical, Jungian aesthetic avoiding sterile SaaS tropes.
*   **Key Colors:**
    *   Background: Deep Void Green (`#0F1C18`)
    *   Text: Parchment Paper (`#F2F0E9`)
    *   Accents: Integral Turquoise (`#2A9D8F`), Systemic Yellow (`#E9C46A`)
*   **Typography:**
    *   Headings: *Fraunces* or *Young Serif* (Mystical/Literary).
    *   Body: *Urbanist* or *Satoshi* (Geometric/Clean).
*   **Components:** Glassmorphism, Super-Ellipses, Fractal Noise textures.

## Development Guidelines (Planned)
*   **Debugging:**
    *   **Panic Portal:** `console_error_panic_hook` to pipe web panics to browser console.
    *   **The Mirror:** Custom `log_to_server` server function to stream browser logs to the terminal (`ai_log!`, `ai_error!` macros).
*   **Testing:**
    *   Headless browser testing via `wasm-pack test --headless --firefox`.

## Key Files
*   `prd.md`: Comprehensive Product Requirements and Architecture.
*   `ui-design.md`: Visual Design Specification.
*   `.env`: (Required) Stores `GROK_API_KEY`, `GROK_MODEL`.
