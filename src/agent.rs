use leptos::*;
use serde::{Deserialize, Serialize};

pub const DEFAULT_GRAPH_USER_ID: &str = "local-user";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub date: String,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatLog {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(schemars::JsonSchema))]
pub struct PatientGraph {
    pub user_id: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl Default for PatientGraph {
    fn default() -> Self {
        Self {
            user_id: DEFAULT_GRAPH_USER_ID.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(schemars::JsonSchema))]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub category: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(schemars::JsonSchema))]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RelationshipProfile {
    pub user_id: String,
    pub slug: String,
    pub display_name: String,
    pub relationship_type: String,
    pub background: String,
    pub goals: Vec<String>,
    pub triggers: Vec<String>,
    pub do_not_say: Vec<String>,
    pub effective_tone: Vec<String>,
    pub recent_events: Vec<String>,
    pub boundaries: Vec<String>,
}

#[cfg(feature = "ssr")]
mod runtime {
    use super::{
        ChatLog, GraphEdge, GraphNode, PatientGraph, RelationshipProfile, Session, User,
        DEFAULT_GRAPH_USER_ID,
    };
    use std::{
        collections::{HashMap, HashSet},
        path::Path as FsPath,
        sync::{Arc, Once},
        time::Instant,
    };

    use anyhow::{Context, Result};
    use argon2::{
        password_hash::{
            rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
        },
        Argon2,
    };
    use axum::{
        extract::{Path, Query},
        http::{HeaderMap, StatusCode},
        response::sse::{Event, Sse},
        response::Json,
    };
    use axum_extra::extract::cookie::{Key, PrivateCookieJar};
    use dashmap::DashMap;
    use leptos::ServerFnError;
    use rig::streaming::StreamingPrompt;
    use rig::vector_store::request::{Filter, VectorSearchRequest};
    use rig::vector_store::{VectorStoreError, VectorStoreIndex};
    use rig::{
        agent::AgentBuilder,
        client::{CompletionClient, EmbeddingsClient},
        completion::{message::Text, AssistantContent, Message, Prompt},
        embeddings::EmbeddingsBuilder,
        providers::{openai, openrouter},
        Embed,
    };
    use rig::{completion::ToolDefinition, tool::Tool};
    use rig_sqlite::{
        Column, ColumnValue, SqliteSearchFilter, SqliteVectorIndex, SqliteVectorStore,
        SqliteVectorStoreTable,
    };
    use rusqlite::ffi::{sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
    use rusqlite::OptionalExtension;
    use schemars::{schema_for, JsonSchema};
    use serde::{Deserialize, Serialize};
    use sqlite_vec::sqlite3_vec_init;
    use tokio::sync::{mpsc, OnceCell, RwLock};
    use tokio::time::{timeout, Duration};
    use tokio_rusqlite::Connection;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;
    use webauthn_rs::prelude::*;

    type SqliteExtensionFn =
        unsafe extern "C" fn(*mut sqlite3, *mut *mut i8, *const sqlite3_api_routines) -> i32;

    const THERAPIST_SYSTEM_PROMPT: &str = r###"
        You are IndividuateAI, a Jungian, somatic-aware therapist. Keep responses under ~180 words, grounded, and practical. Mirror the user briefly, surface patterns, propose one concrete practice, and end with a concise reflective question. If the user shares safety-critical content, encourage professional or emergency support.
    "###;
    const DRAFT_SYSTEM_PROMPT: &str = r###"
        You write messages on behalf of the user.
        Write in first person as the user, not as a therapist or coach.
        Use the supplied relationship context, broader autobiographical memory, and recent conversation context.
        Prefer emotionally accurate, human language over clinical language.
        Preserve the user's boundaries and goals. Avoid manipulation, guilt-tripping, threats, or false promises.
        Return:
        Draft 1: <message>
        Draft 2: <message>
        Notes: <2-4 brief bullets on tone/tradeoffs>
    "###;
    const GRAPH_DELTA_PROMPT: &str = r###"
        Identify new psychological concepts or connections in the conversation.
        Only return NEW additions or explicit removals.
        Use stable snake_case ids, lowercase with underscores (example: sleep_deprivation).
        Keep labels 2-4 words and categories one of: Trigger, Belief, Emotion, Somatic, Pattern, Need, Goal, Resource, Other.
        If nothing changes, return empty arrays.
    "###;
    const RELATIONSHIP_PROFILE_PROMPT: &str = r###"
        Extract close-relationship memory from the text.
        Focus on people like mother, mom, dad, father, brother, partner, spouse, girlfriend, boyfriend, and close friends.
        Return only profiles that are explicitly mentioned or strongly implied.
        Use stable slugs like mother, dad, brother, partner, friend, or a simple snake_case name if a specific friend is repeatedly named.
        Keep fields concise and grounded in what the user actually said.
        Put only the most relevant facts in background.
        Include only actionable goals, triggers, boundaries, tone preferences, and recent events that are clearly supported by the text.
        If nothing useful is present, return an empty profiles array.
    "###;
    const SESSION_SUMMARY_PROMPT: &str = r###"
        Summarize the conversation into a short session title and preview.
        The title should be 2-6 words, natural language, and specific enough to distinguish this session from other sessions.
        The preview should be one sentence fragment under 120 characters that captures the current focus.
        Avoid generic words like therapy session, check-in, conversation, or support unless necessary.
    "###;

    pub(crate) const COOKIE_SECRET: &[u8] =
        b"SUPER_SECRET_KEY_MUST_BE_CHANGED_IN_PROD_12345678901234567890123456789012";
    pub(crate) const AUTH_COOKIE_NAME: &str = "auth_token";

    pub fn cookie_key() -> Key {
        if let Ok(secret) = std::env::var("COOKIE_SECRET") {
            if !secret.trim().is_empty() {
                let mut key_bytes = [0u8; 64];
                for (index, byte) in secret.as_bytes().iter().enumerate() {
                    key_bytes[index % key_bytes.len()] ^= *byte;
                }
                return Key::from(&key_bytes);
            }
        }
        Key::from(COOKIE_SECRET)
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct GraphUpdateArgs {
        pub user_id: String,
        pub new_nodes: Vec<GraphNode>,
        pub new_edges: Vec<GraphEdge>,
        pub nodes_to_remove_ids: Vec<String>,
        #[serde(default)]
        pub edges_to_remove: Vec<GraphEdge>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct GraphReadArgs {
        pub user_id: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct RelationshipProfileRecord {
        pub display_name: String,
        pub relationship_type: String,
        pub background: String,
        pub goals: Vec<String>,
        pub triggers: Vec<String>,
        pub do_not_say: Vec<String>,
        pub effective_tone: Vec<String>,
        pub recent_events: Vec<String>,
        pub boundaries: Vec<String>,
    }

    impl From<RelationshipProfile> for RelationshipProfileRecord {
        fn from(value: RelationshipProfile) -> Self {
            Self {
                display_name: value.display_name,
                relationship_type: value.relationship_type,
                background: value.background,
                goals: value.goals,
                triggers: value.triggers,
                do_not_say: value.do_not_say,
                effective_tone: value.effective_tone,
                recent_events: value.recent_events,
                boundaries: value.boundaries,
            }
        }
    }

    impl RelationshipProfileRecord {
        fn with_identity(self, user_id: String, slug: String) -> RelationshipProfile {
            RelationshipProfile {
                user_id,
                slug,
                display_name: self.display_name,
                relationship_type: self.relationship_type,
                background: self.background,
                goals: self.goals,
                triggers: self.triggers,
                do_not_say: self.do_not_say,
                effective_tone: self.effective_tone,
                recent_events: self.recent_events,
                boundaries: self.boundaries,
            }
        }
    }

    #[derive(Clone, Debug)]
    struct MemoryCandidate {
        score: i32,
        summary: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct GraphUpdateSummary {
        added_nodes: usize,
        added_edges: usize,
        removed_nodes: usize,
        removed_edges: usize,
        total_nodes: usize,
        total_edges: usize,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct ExtractedRelationshipProfile {
        pub slug: String,
        pub display_name: String,
        pub relationship_type: String,
        pub background: String,
        #[serde(default)]
        pub goals: Vec<String>,
        #[serde(default)]
        pub triggers: Vec<String>,
        #[serde(default)]
        pub do_not_say: Vec<String>,
        #[serde(default)]
        pub effective_tone: Vec<String>,
        #[serde(default)]
        pub recent_events: Vec<String>,
        #[serde(default)]
        pub boundaries: Vec<String>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct RelationshipProfileDelta {
        #[serde(default)]
        pub profiles: Vec<ExtractedRelationshipProfile>,
    }

    impl RelationshipProfileDelta {
        fn is_empty(&self) -> bool {
            self.profiles.is_empty()
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct SessionSummaryData {
        pub title: String,
        pub preview: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct ConversationGraphDelta {
        pub new_concepts: Vec<GraphNode>,
        pub new_connections: Vec<GraphEdge>,
        pub obsolete_concept_ids: Vec<String>,
        #[serde(default)]
        pub obsolete_connections: Vec<GraphEdge>,
    }

    impl ConversationGraphDelta {
        fn is_empty(&self) -> bool {
            self.new_concepts.is_empty()
                && self.new_connections.is_empty()
                && self.obsolete_concept_ids.is_empty()
                && self.obsolete_connections.is_empty()
        }
    }

    #[derive(Debug, thiserror::Error)]
    enum GraphToolError {
        #[error("{0}")]
        Message(String),
    }

    #[derive(Clone)]
    struct GraphReaderTool {
        conn: Connection,
    }

    #[derive(Clone)]
    struct GraphManagerTool {
        conn: Connection,
    }

    struct SqliteIndexAdapter<E, T>
    where
        E: rig::embeddings::EmbeddingModel + 'static,
        T: SqliteVectorStoreTable + 'static,
    {
        inner: SqliteVectorIndex<E, T>,
    }

    impl<E, T> VectorStoreIndex for SqliteIndexAdapter<E, T>
    where
        E: rig::embeddings::EmbeddingModel + Sync + Send + 'static,
        T: SqliteVectorStoreTable + Clone + for<'de> Deserialize<'de> + 'static,
    {
        type Filter = Filter<serde_json::Value>;

        async fn top_n<D>(
            &self,
            req: VectorSearchRequest<Self::Filter>,
        ) -> Result<Vec<(f64, String, D)>, VectorStoreError>
        where
            D: for<'de> Deserialize<'de> + Send,
        {
            let mapped = req.map_filter(Filter::interpret::<SqliteSearchFilter>);
            self.inner.top_n(mapped).await
        }

        async fn top_n_ids(
            &self,
            req: VectorSearchRequest<Self::Filter>,
        ) -> Result<Vec<(f64, String)>, VectorStoreError> {
            let mapped = req.map_filter(Filter::interpret::<SqliteSearchFilter>);
            self.inner.top_n_ids(mapped).await
        }
    }

    #[derive(Embed, Clone, Debug, Serialize, Deserialize)]
    pub struct MemoryFragment {
        pub id: String,
        pub title: String,
        #[embed]
        pub content: String,
        pub tags: String,
    }

    impl SqliteVectorStoreTable for MemoryFragment {
        fn name() -> &'static str {
            "therapy_memory"
        }

        fn schema() -> Vec<Column> {
            vec![
                Column::new("id", "TEXT PRIMARY KEY"),
                Column::new("title", "TEXT"),
                Column::new("content", "TEXT"),
                Column::new("tags", "TEXT"),
            ]
        }

        fn id(&self) -> String {
            self.id.clone()
        }

        fn column_values(&self) -> Vec<(&'static str, Box<dyn ColumnValue>)> {
            vec![
                ("id", Box::new(self.id.clone())),
                ("title", Box::new(self.title.clone())),
                ("content", Box::new(self.content.clone())),
                ("tags", Box::new(self.tags.clone())),
            ]
        }
    }

    fn seed_memory() -> Vec<MemoryFragment> {
        vec![
            MemoryFragment {
                id: "persona".into(),
                title: "Therapist voice".into(),
                content: "Organic Integral tone: Jungian, shadow-aware, dream-friendly, somatic. Encourage slow, embodied pacing. Balance empathy with gentle accountability.".into(),
                tags: "persona,style".into(),
            },
            MemoryFragment {
                id: "session-frame".into(),
                title: "Session structure".into(),
                content: "Flow: (1) Mirror what you heard. (2) Name pattern/archetype/image. (3) Offer one grounded practice (breath, journaling, active imagination). (4) Close with a concise reflective question.".into(),
                tags: "structure,flow".into(),
            },
            MemoryFragment {
                id: "guardrails".into(),
                title: "Safety + scope".into(),
                content: "Not a crisis line. Avoid medical diagnosis or prescriptions. If user signals self-harm, redirect to emergency services or trusted humans. Keep advice within coaching/therapeutic support bounds.".into(),
                tags: "safety,boundaries".into(),
            },
            MemoryFragment {
                id: "slider-meaning".into(),
                title: "Controls meaning".into(),
                content: "Accountability slider: higher -> more direct commitments and follow-ups; lower -> gentle encouragement. Spirituality slider: higher -> archetypes, mythic images, symbolism; lower -> plain, pragmatic. Directness slider: higher -> blunt clarity; lower -> soft phrasing.".into(),
                tags: "ui,controls".into(),
            },
            MemoryFragment {
                id: "drafting-voice".into(),
                title: "Drafting voice".into(),
                content: "When asked to draft a message, write as the user in clear, human first-person language. Be emotionally honest, concrete, and concise. Avoid therapist framing unless explicitly requested.".into(),
                tags: "drafting,style".into(),
            },
            MemoryFragment {
                id: "drafting-boundaries".into(),
                title: "Drafting boundaries".into(),
                content: "When writing difficult relationship messages, protect the user's boundaries, avoid manipulation, and prefer warmth plus clarity over over-explaining.".into(),
                tags: "drafting,boundaries".into(),
            },
        ]
    }

    fn init_sqlite_extensions() {
        static SQLITE_VEC: Once = Once::new();
        SQLITE_VEC.call_once(|| unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<*const (), SqliteExtensionFn>(
                sqlite3_vec_init as *const (),
            )));
        });
    }

    async fn ensure_data_dir(db_path: &str) -> Result<()> {
        if let Some(parent) = FsPath::new(db_path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Creating data dir {:?}", parent))?;
            }
        }
        Ok(())
    }

    fn table_exists(conn: &rusqlite::Connection, table: &str) -> rusqlite::Result<bool> {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, i64>(0).map(|v| v == 1),
        )
    }

    fn table_has_column(
        conn: &rusqlite::Connection,
        table: &str,
        column: &str,
    ) -> rusqlite::Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn ensure_schema(conn: &Connection) -> Result<()> {
        conn.call(|conn| {
            conn.execute_batch(
                r###"
                CREATE TABLE IF NOT EXISTS users (
                    id TEXT PRIMARY KEY,
                    username TEXT UNIQUE NOT NULL,
                    password_hash TEXT NOT NULL,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    user_id TEXT,
                    title TEXT NOT NULL,
                    preview TEXT NOT NULL DEFAULT '',
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY(user_id) REFERENCES users(id)
                );
                CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY(session_id) REFERENCES sessions(id)
                );
                CREATE TABLE IF NOT EXISTS patient_graphs (
                    user_id TEXT PRIMARY KEY,
                    graph_json TEXT NOT NULL,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS relationship_profiles (
                    user_id TEXT NOT NULL,
                    slug TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    relationship_type TEXT NOT NULL,
                    profile_json TEXT NOT NULL,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (user_id, slug),
                    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS passkeys (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL,
                    credential_id BLOB UNIQUE NOT NULL,
                    passkey BLOB NOT NULL,
                    counter INTEGER NOT NULL DEFAULT 0,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                    last_used_at DATETIME,
                    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_passkeys_user_id ON passkeys(user_id);
                "###,
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)?;

            if table_exists(conn, "sessions").map_err(tokio_rusqlite::Error::Rusqlite)?
                && !table_has_column(conn, "sessions", "user_id")
                    .map_err(tokio_rusqlite::Error::Rusqlite)?
            {
                conn.execute("ALTER TABLE sessions ADD COLUMN user_id TEXT", [])
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
            }

            if table_exists(conn, "sessions").map_err(tokio_rusqlite::Error::Rusqlite)?
                && !table_has_column(conn, "sessions", "preview")
                    .map_err(tokio_rusqlite::Error::Rusqlite)?
            {
                conn.execute(
                    "ALTER TABLE sessions ADD COLUMN preview TEXT NOT NULL DEFAULT ''",
                    [],
                )
                .map_err(tokio_rusqlite::Error::Rusqlite)?;
            }

            if table_exists(conn, "users").map_err(tokio_rusqlite::Error::Rusqlite)?
                && !table_has_column(conn, "users", "password_hash")
                    .map_err(tokio_rusqlite::Error::Rusqlite)?
            {
                conn.execute("ALTER TABLE users ADD COLUMN password_hash TEXT", [])
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
            }

            Ok(())
        })
        .await
        .context("Initializing chat schema")
    }

    async fn table_has_rows(conn: &Connection, table: &str) -> Result<bool> {
        let stmt = format!("SELECT EXISTS(SELECT 1 FROM {table} LIMIT 1)");
        let exists = conn
            .call(move |conn| {
                conn.query_row(&stmt, [], |row| row.get::<_, i64>(0))
                    .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .with_context(|| format!("Checking rows for table {table}"))?;

        Ok(exists == 1)
    }

    async fn read_graph(conn: &Connection, user_id: &str) -> Result<PatientGraph> {
        let user_id_owned = user_id.to_string();
        let stored: Option<String> = conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT graph_json FROM patient_graphs WHERE user_id = ?1",
                    [user_id_owned],
                    |row| row.get(0),
                )
                .optional()
                .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .context("Fetching patient graph")?;

        if let Some(raw) = stored {
            let graph =
                serde_json::from_str::<PatientGraph>(&raw).context("Parsing patient graph JSON")?;
            return Ok(graph);
        }

        let graph = PatientGraph {
            user_id: user_id.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        write_graph(conn, &graph).await?;
        Ok(graph)
    }

    async fn write_graph(conn: &Connection, graph: &PatientGraph) -> Result<()> {
        let user_id = graph.user_id.clone();
        let payload = serde_json::to_string(graph).context("Serializing patient graph")?;
        conn.call(move |conn| {
            conn.execute(
                r###"
                INSERT INTO patient_graphs (user_id, graph_json)
                VALUES (?1, ?2)
                ON CONFLICT(user_id)
                DO UPDATE SET graph_json = excluded.graph_json,
                              updated_at = CURRENT_TIMESTAMP
                "###,
                rusqlite::params![user_id, payload],
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await
        .context("Persisting patient graph")?;
        Ok(())
    }

    fn apply_graph_update(graph: &mut PatientGraph, update: GraphUpdateArgs) -> GraphUpdateSummary {
        let mut added_nodes = 0;
        let mut added_edges = 0;
        let mut removed_nodes = 0;
        let mut removed_edges = 0;

        let remove_nodes: std::collections::HashSet<String> =
            update.nodes_to_remove_ids.into_iter().collect();
        if !remove_nodes.is_empty() {
            let before = graph.nodes.len();
            graph.nodes.retain(|node| !remove_nodes.contains(&node.id));
            removed_nodes = before.saturating_sub(graph.nodes.len());
        }

        if !remove_nodes.is_empty() {
            let before = graph.edges.len();
            graph.edges.retain(|edge| {
                !remove_nodes.contains(&edge.from) && !remove_nodes.contains(&edge.to)
            });
            removed_edges += before.saturating_sub(graph.edges.len());
        }

        let remove_edges: std::collections::HashSet<(String, String, String)> = update
            .edges_to_remove
            .into_iter()
            .map(|edge| (edge.from, edge.to, edge.relation))
            .collect();
        if !remove_edges.is_empty() {
            let before = graph.edges.len();
            graph.edges.retain(|edge| {
                !remove_edges.contains(&(edge.from.clone(), edge.to.clone(), edge.relation.clone()))
            });
            removed_edges += before.saturating_sub(graph.edges.len());
        }

        let mut existing_nodes: std::collections::HashSet<String> =
            graph.nodes.iter().map(|node| node.id.clone()).collect();
        for node in update.new_nodes {
            if existing_nodes.insert(node.id.clone()) {
                graph.nodes.push(node);
                added_nodes += 1;
            }
        }

        let mut existing_edges: std::collections::HashSet<(String, String, String)> = graph
            .edges
            .iter()
            .map(|edge| (edge.from.clone(), edge.to.clone(), edge.relation.clone()))
            .collect();
        for edge in update.new_edges {
            if !existing_nodes.contains(&edge.from) || !existing_nodes.contains(&edge.to) {
                continue;
            }
            let key = (edge.from.clone(), edge.to.clone(), edge.relation.clone());
            if existing_edges.insert(key) {
                graph.edges.push(edge);
                added_edges += 1;
            }
        }

        GraphUpdateSummary {
            added_nodes,
            added_edges,
            removed_nodes,
            removed_edges,
            total_nodes: graph.nodes.len(),
            total_edges: graph.edges.len(),
        }
    }

    fn graph_context(graph: &PatientGraph) -> String {
        if graph.nodes.is_empty() && graph.edges.is_empty() {
            return "Current graph is empty.".to_string();
        }

        let mut lines = Vec::new();
        lines.push("Current nodes (id: label [category]):".to_string());
        for node in graph.nodes.iter().take(60) {
            lines.push(format!("- {}: {} [{}]", node.id, node.label, node.category));
        }
        lines.push("Current edges (from -> to: relation):".to_string());
        for edge in graph.edges.iter().take(80) {
            lines.push(format!(
                "- {} -> {} ({})",
                edge.from, edge.to, edge.relation
            ));
        }
        lines.join("\n")
    }

    fn normalize_slug(value: &str) -> String {
        value
            .trim()
            .to_lowercase()
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string()
    }

    fn tokenize(value: &str) -> HashSet<String> {
        value
            .to_lowercase()
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|part| part.len() >= 3)
            .map(ToOwned::to_owned)
            .collect()
    }

    fn relationship_aliases(
        slug: &str,
        relationship_type: &str,
        display_name: &str,
    ) -> Vec<String> {
        let mut aliases = HashSet::new();
        for value in [slug, relationship_type, display_name] {
            let lowered = value.trim().to_lowercase();
            if !lowered.is_empty() {
                aliases.insert(lowered.clone());
                aliases.insert(lowered.replace('_', " "));
            }
        }

        match slug {
            "mother" | "mom" => {
                aliases.extend(
                    ["mother", "mom", "mum", "mama"]
                        .into_iter()
                        .map(str::to_string),
                );
            }
            "father" | "dad" => {
                aliases.extend(
                    ["father", "dad", "daddy", "pops"]
                        .into_iter()
                        .map(str::to_string),
                );
            }
            "brother" => {
                aliases.extend(
                    ["brother", "bro", "my brother"]
                        .into_iter()
                        .map(str::to_string),
                );
            }
            "partner" => {
                aliases.extend(
                    ["partner", "girlfriend", "boyfriend", "wife", "husband"]
                        .into_iter()
                        .map(str::to_string),
                );
            }
            _ => {}
        }

        aliases.into_iter().collect()
    }

    fn overlap_score(text: &str, query_terms: &HashSet<String>) -> i32 {
        let text_terms = tokenize(text);
        query_terms
            .iter()
            .filter(|term| text_terms.contains(*term))
            .count() as i32
    }

    fn join_items(items: &[String]) -> String {
        if items.is_empty() {
            "none".to_string()
        } else {
            items.join(", ")
        }
    }

    fn canonical_relationship_slug(
        slug: &str,
        relationship_type: &str,
        display_name: &str,
    ) -> String {
        let normalized = normalize_slug(slug);
        match normalized.as_str() {
            "mom" | "mum" | "mother" | "mama" => "mother".to_string(),
            "father" | "dad" | "daddy" | "pops" => "dad".to_string(),
            "bro" | "brother" => "brother".to_string(),
            "partner" | "spouse" | "wife" | "husband" | "girlfriend" | "boyfriend" => {
                "partner".to_string()
            }
            "friend" | "friends" | "best_friend" => "friend".to_string(),
            _ => {
                let relationship_type = normalize_slug(relationship_type);
                match relationship_type.as_str() {
                    "mother" | "mom" | "mum" => "mother".to_string(),
                    "father" | "dad" => "dad".to_string(),
                    "brother" => "brother".to_string(),
                    "partner" | "spouse" | "wife" | "husband" | "girlfriend" | "boyfriend" => {
                        "partner".to_string()
                    }
                    "friend" | "friends" => "friend".to_string(),
                    _ => {
                        let display_name = normalize_slug(display_name);
                        if !display_name.is_empty() {
                            display_name
                        } else {
                            normalized
                        }
                    }
                }
            }
        }
    }

    fn merge_unique_strings(existing: &[String], incoming: &[String], limit: usize) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut merged = Vec::new();
        for value in existing.iter().chain(incoming.iter()) {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lowered = trimmed.to_lowercase();
            if seen.insert(lowered) {
                merged.push(trimmed.to_string());
            }
            if merged.len() >= limit {
                break;
            }
        }
        merged
    }

    fn merge_background(existing: &str, incoming: &str) -> String {
        let existing = existing.trim();
        let incoming = incoming.trim();
        if existing.is_empty() {
            return incoming.to_string();
        }
        if incoming.is_empty() {
            return existing.to_string();
        }
        if existing.eq_ignore_ascii_case(incoming) || existing.contains(incoming) {
            return existing.to_string();
        }
        if incoming.contains(existing) {
            return incoming.to_string();
        }
        format!("{} {}", existing, incoming)
    }

    fn merge_relationship_profile(
        user_id: String,
        existing: Option<RelationshipProfile>,
        incoming: ExtractedRelationshipProfile,
    ) -> RelationshipProfile {
        let slug = canonical_relationship_slug(
            &incoming.slug,
            &incoming.relationship_type,
            &incoming.display_name,
        );
        let existing = existing.unwrap_or_default();
        let display_name = if incoming.display_name.trim().is_empty() {
            if existing.display_name.trim().is_empty() {
                slug.replace('_', " ")
            } else {
                existing.display_name
            }
        } else {
            incoming.display_name.trim().to_string()
        };
        let relationship_type = if incoming.relationship_type.trim().is_empty() {
            if existing.relationship_type.trim().is_empty() {
                slug.clone()
            } else {
                existing.relationship_type
            }
        } else {
            incoming.relationship_type.trim().to_string()
        };

        RelationshipProfile {
            user_id,
            slug,
            display_name,
            relationship_type,
            background: merge_background(&existing.background, &incoming.background),
            goals: merge_unique_strings(&existing.goals, &incoming.goals, 8),
            triggers: merge_unique_strings(&existing.triggers, &incoming.triggers, 8),
            do_not_say: merge_unique_strings(&existing.do_not_say, &incoming.do_not_say, 8),
            effective_tone: merge_unique_strings(
                &existing.effective_tone,
                &incoming.effective_tone,
                8,
            ),
            recent_events: merge_unique_strings(
                &existing.recent_events,
                &incoming.recent_events,
                10,
            ),
            boundaries: merge_unique_strings(&existing.boundaries, &incoming.boundaries, 8),
        }
    }

    fn compress_logs_for_profile_bootstrap(
        logs: &[(String, String, String, String)],
        max_chars: usize,
    ) -> String {
        let mut lines = Vec::new();
        let mut total = 0usize;
        for (role, content, title, created_at) in logs.iter() {
            let snippet = content.trim();
            if snippet.is_empty() {
                continue;
            }
            let line = format!("[{} | {}] {}: {}", created_at, title, role, snippet);
            total += line.len();
            if total > max_chars {
                break;
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn compress_chat_logs(logs: &[ChatLog], max_chars: usize) -> String {
        let mut lines = Vec::new();
        let mut total = 0usize;
        for log in logs.iter() {
            let snippet = log.content.trim();
            if snippet.is_empty() {
                continue;
            }
            let line = format!("{}: {}", log.role, snippet);
            total += line.len();
            if total > max_chars {
                break;
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn fallback_session_preview(logs: &[ChatLog]) -> String {
        logs.iter()
            .rev()
            .find_map(|log| {
                let snippet = log.content.trim();
                (!snippet.is_empty()).then(|| snippet.chars().take(120).collect::<String>())
            })
            .unwrap_or_else(|| "Begin exploring what's here.".to_string())
    }

    impl Tool for GraphReaderTool {
        const NAME: &'static str = "read_mind_map";
        type Error = GraphToolError;
        type Args = GraphReadArgs;
        type Output = PatientGraph;

        async fn definition(&self, _prompt: String) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Read the current patient mind map from SQLite.".to_string(),
                parameters: serde_json::json!(schema_for!(GraphReadArgs)),
            }
        }

        async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
            read_graph(&self.conn, &args.user_id)
                .await
                .map_err(|err| GraphToolError::Message(err.to_string()))
        }
    }

    impl Tool for GraphManagerTool {
        const NAME: &'static str = "update_mind_map";
        type Error = GraphToolError;
        type Args = GraphUpdateArgs;
        type Output = GraphUpdateSummary;

        async fn definition(&self, _prompt: String) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Apply incremental updates to the patient mind map.".to_string(),
                parameters: serde_json::json!(schema_for!(GraphUpdateArgs)),
            }
        }

        async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
            let mut graph = read_graph(&self.conn, &args.user_id)
                .await
                .map_err(|err| GraphToolError::Message(err.to_string()))?;
            let summary = apply_graph_update(&mut graph, args);
            write_graph(&self.conn, &graph)
                .await
                .map_err(|err| GraphToolError::Message(err.to_string()))?;
            Ok(summary)
        }
    }

    pub struct AgentRuntime {
        therapist_agent: rig::agent::Agent<openrouter::completion::CompletionModel>,
        draft_agent: rig::agent::Agent<openrouter::completion::CompletionModel>,
        histories: RwLock<HashMap<String, Vec<Message>>>,
        conn: Connection,
        openai_client: openai::Client,
        graph_reader: GraphReaderTool,
        graph_writer: GraphManagerTool,
        graph_user_id: String,
        webauthn: Webauthn,
        pending_registrations: DashMap<String, (Instant, String, PasskeyRegistration)>,
        pending_logins: DashMap<String, (Instant, PasskeyAuthentication)>,
    }

    impl AgentRuntime {
        async fn new() -> Result<Self> {
            init_sqlite_extensions();

            let db_path =
                std::env::var("MEMORY_DB_PATH").unwrap_or_else(|_| "data/memory.sqlite".into());
            ensure_data_dir(&db_path).await?;
            let db_path_display = db_path.clone();
            let conn = Connection::open(db_path)
                .await
                .context("Opening sqlite memory store")?;

            // Init Chat + Graph Schema + Passkeys
            ensure_schema(&conn)
                .await
                .with_context(|| format!("Initializing chat schema at {db_path_display}"))?;

            let openai_key =
                std::env::var("OPENAI_API_KEY").context("Set OPENAI_API_KEY for embeddings")?;
            let openai_client: openai::Client = openai::Client::builder()
                .api_key(openai_key)
                .build()
                .context("Building OpenAI client")?;
            let embedding_model_name = std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| openai::TEXT_EMBEDDING_ADA_002.to_string());
            let embedding_model = openai_client.embedding_model(embedding_model_name);

            let vector_store: SqliteVectorStore<_, MemoryFragment> =
                SqliteVectorStore::new(conn.clone(), &embedding_model)
                    .await
                    .context("Initializing sqlite vector store")?;

            if !table_has_rows(&conn, MemoryFragment::name()).await? {
                let embeddings = EmbeddingsBuilder::new(embedding_model.clone())
                    .documents(seed_memory())?
                    .build()
                    .await
                    .context("Building seed embeddings")?;

                vector_store
                    .add_rows(embeddings)
                    .await
                    .context("Seeding sqlite vector store")?;
            }

            let vector_index = SqliteIndexAdapter {
                inner: vector_store.index(embedding_model),
            };

            let openrouter_key =
                std::env::var("OPENROUTER_API_KEY").context("Set OPENROUTER_API_KEY")?;
            let openrouter_model = std::env::var("OPENROUTER_MODEL")
                .unwrap_or_else(|_| "moonshotai/kimi-k2-thinking".to_string());
            let openrouter_client = openrouter::Client::builder()
                .api_key(openrouter_key)
                .build()
                .context("Building OpenRouter client")?;

            let therapist_agent =
                AgentBuilder::new(openrouter_client.completion_model(openrouter_model.clone()))
                    .name("individuateai_therapist")
                    .preamble(THERAPIST_SYSTEM_PROMPT)
                    .dynamic_context(4, vector_index)
                    .build();
            let draft_agent =
                AgentBuilder::new(openrouter_client.completion_model(openrouter_model))
                    .name("individuateai_drafter")
                    .preamble(DRAFT_SYSTEM_PROMPT)
                    .build();

            let graph_reader = GraphReaderTool { conn: conn.clone() };
            let graph_writer = GraphManagerTool { conn: conn.clone() };
            let graph_user_id = std::env::var("GRAPH_USER_ID")
                .unwrap_or_else(|_| DEFAULT_GRAPH_USER_ID.to_string());

            // Initialize WebAuthn
            let rp_id = std::env::var("RP_ID").unwrap_or_else(|_| "localhost".to_string());
            let rp_origin =
                std::env::var("RP_ORIGIN").unwrap_or_else(|_| "http://localhost:3008".to_string());

            let rp_origin_url = Url::parse(&rp_origin).context("Invalid RP_ORIGIN")?;
            let builder =
                WebauthnBuilder::new(&rp_id, &rp_origin_url).expect("Invalid WebAuthn config");
            let webauthn = builder.build().expect("Failed to create Webauthn instance");

            Ok(Self {
                therapist_agent,
                draft_agent,
                histories: RwLock::new(HashMap::new()),
                openai_client,
                graph_reader,
                graph_writer,
                graph_user_id,
                conn,
                webauthn,
                pending_registrations: DashMap::new(),
                pending_logins: DashMap::new(),
            })
        }

        // --- Auth & User Management ---

        async fn create_user(&self, username: String, password: String) -> Result<User> {
            let id = Uuid::new_v4().to_string();
            let id_clone = id.clone();
            let username_clone = username.clone();
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            let password_hash = argon2
                .hash_password(password.as_bytes(), &salt)
                .map_err(|e| anyhow::anyhow!("Hashing failed: {}", e))?
                .to_string();

            self.conn
                .call(move |conn| {
                    conn.execute(
                        "INSERT INTO users (id, username, password_hash) VALUES (?1, ?2, ?3)",
                        rusqlite::params![id_clone, username_clone, password_hash],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            Ok(User { id, username })
        }

        async fn verify_user(&self, username: String, password: String) -> Result<User> {
            let username_clone = username.clone();
            let (id, stored_hash) = self
                .conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT id, password_hash FROM users WHERE username = ?1",
                        [username_clone],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("User not found")?;

            let parsed_hash = PasswordHash::new(&stored_hash)
                .map_err(|e| anyhow::anyhow!("Invalid hash format: {}", e))?;

            Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .map_err(|e| anyhow::anyhow!("Invalid password: {}", e))?;

            Ok(User { id, username })
        }

        async fn get_user_by_id(&self, id: String) -> Result<User> {
            self.conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT id, username FROM users WHERE id = ?1",
                        [id],
                        |row| {
                            Ok(User {
                                id: row.get(0)?,
                                username: row.get(1)?,
                            })
                        },
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("User not found")
        }

        async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
            let username = username.to_string();
            self.conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT id, username FROM users WHERE username = ?1",
                        [username],
                        |row| {
                            Ok(User {
                                id: row.get(0)?,
                                username: row.get(1)?,
                            })
                        },
                    )
                    .optional()
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Looking up user by username")
        }

        async fn count_passkeys_for_user(&self, user_id: &str) -> Result<i64> {
            let user_id = user_id.to_string();
            self.conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT COUNT(*) FROM passkeys WHERE user_id = ?1",
                        [user_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Counting user passkeys")
        }

        // --- Passkey Management ---

        pub async fn start_passkey_registration_email(
            &self,
            email: String,
        ) -> Result<(String, CreationChallengeResponse)> {
            let email = email.trim().to_string();
            if email.is_empty() {
                return Err(anyhow::anyhow!("Email is required"));
            }

            let user = match self.get_user_by_username(&email).await? {
                Some(user) => {
                    let passkey_count = self.count_passkeys_for_user(&user.id).await?;
                    if passkey_count > 0 {
                        return Err(anyhow::anyhow!("Account already exists. Please log in."));
                    }
                    user
                }
                None => {
                    let random_password = Uuid::new_v4().to_string();
                    self.create_user(email, random_password).await?
                }
            };

            self.start_passkey_registration(user.id).await
        }

        pub async fn start_passkey_registration(
            &self,
            user_id: String,
        ) -> Result<(String, CreationChallengeResponse)> {
            let user = self.get_user_by_id(user_id.clone()).await?;
            let user_uuid = Uuid::parse_str(&user.id).unwrap_or_else(|_| Uuid::new_v4());

            let existing_cred_ids: Vec<CredentialID> = self
                .conn
                .call(move |conn| {
                    let mut stmt =
                        conn.prepare("SELECT credential_id FROM passkeys WHERE user_id = ?1")?;
                    let rows = stmt.query_map([user_id], |row| {
                        let cred_blob: Vec<u8> = row.get(0)?;
                        Ok(CredentialID::from(cred_blob))
                    })?;
                    let mut res = Vec::new();
                    for r in rows {
                        res.push(r?);
                    }
                    Ok(res)
                })
                .await?;

            let exclude_credentials = if existing_cred_ids.is_empty() {
                None
            } else {
                Some(existing_cred_ids)
            };

            let (challenge, state) = self
                .webauthn
                .start_passkey_registration(
                    user_uuid,
                    &user.username,
                    &user.username,
                    exclude_credentials,
                )
                .map_err(|e| anyhow::anyhow!("WebAuthn start failed: {}", e))?;

            let req_id = Uuid::new_v4().to_string();
            self.pending_registrations
                .insert(req_id.clone(), (Instant::now(), user.id.clone(), state));

            Ok((req_id, challenge))
        }

        pub async fn finish_passkey_registration(
            &self,
            req_id: String,
            response: RegisterPublicKeyCredential,
        ) -> Result<User> {
            let (_, (_, user_id, state)) = self
                .pending_registrations
                .remove(&req_id)
                .ok_or_else(|| anyhow::anyhow!("Registration expired or invalid"))?;

            let passkey = self
                .webauthn
                .finish_passkey_registration(&response, &state)
                .map_err(|e| anyhow::anyhow!("WebAuthn verification failed: {}", e))?;

            let cred_id_blob: Vec<u8> = passkey.cred_id().as_ref().to_vec();
            let passkey_blob = serde_cbor_2::to_vec(&passkey).context("Serializing passkey")?;

            let user_id_for_insert = user_id.clone();
            self.conn
                .call(move |conn| {
                    conn.execute(
                    "INSERT INTO passkeys (user_id, credential_id, passkey) VALUES (?1, ?2, ?3)",
                    rusqlite::params![user_id_for_insert, cred_id_blob, passkey_blob],
                ).map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            self.get_user_by_id(user_id).await
        }

        pub async fn start_passkey_login(
            &self,
            username: String,
        ) -> Result<(String, RequestChallengeResponse)> {
            let username_clone = username.clone();
            let user = match self
                .conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT id FROM users WHERE username = ?1",
                        [username_clone],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
            {
                Ok(id) => Ok(User { id, username }),
                Err(_) => Err(anyhow::anyhow!("User not found")),
            }?;

            let user_id = user.id.clone();
            let passkey_blobs: Vec<Vec<u8>> = self
                .conn
                .call(move |conn| {
                    let mut stmt =
                        conn.prepare("SELECT passkey FROM passkeys WHERE user_id = ?1")?;
                    let rows = stmt.query_map([user_id], |row| row.get(0))?;
                    let mut res = Vec::new();
                    for r in rows {
                        res.push(r?);
                    }
                    Ok(res)
                })
                .await?;

            let allow_creds: Vec<Passkey> = passkey_blobs
                .into_iter()
                .map(|blob| serde_cbor_2::from_slice(&blob).context("Deserializing passkey"))
                .collect::<Result<_>>()?;

            let (challenge, state) = self
                .webauthn
                .start_passkey_authentication(&allow_creds)
                .map_err(|e| anyhow::anyhow!("WebAuthn login start failed: {}", e))?;

            let req_id = Uuid::new_v4().to_string();
            self.pending_logins
                .insert(req_id.clone(), (Instant::now(), state));

            Ok((req_id, challenge))
        }

        pub async fn finish_passkey_login(
            &self,
            req_id: String,
            response: PublicKeyCredential,
        ) -> Result<User> {
            let (_, (_, state)) = self
                .pending_logins
                .remove(&req_id)
                .ok_or_else(|| anyhow::anyhow!("Login expired or invalid"))?;

            let auth_result = self
                .webauthn
                .finish_passkey_authentication(&response, &state)
                .map_err(|e| anyhow::anyhow!("WebAuthn login verification failed: {}", e))?;

            let cred_id_blob = auth_result.cred_id().as_ref().to_vec();
            let new_counter = auth_result.counter();
            let cred_id_for_counter = cred_id_blob.clone();

            self.conn.call(move |conn| {
                conn.execute(
                    "UPDATE passkeys SET counter = ?1, last_used_at = CURRENT_TIMESTAMP WHERE credential_id = ?2",
                    rusqlite::params![new_counter, cred_id_for_counter],
                ).map_err(tokio_rusqlite::Error::Rusqlite)
            }).await?;

            if auth_result.needs_update() {
                let cred_id_for_passkey = cred_id_blob.clone();
                if let Some(passkey_blob) = self
                    .conn
                    .call(move |conn| {
                        conn.query_row(
                            "SELECT passkey FROM passkeys WHERE credential_id = ?1",
                            [cred_id_for_passkey],
                            |row| row.get::<_, Vec<u8>>(0),
                        )
                        .optional()
                        .map_err(tokio_rusqlite::Error::Rusqlite)
                    })
                    .await?
                {
                    let mut passkey: Passkey = serde_cbor_2::from_slice(&passkey_blob)
                        .context("Deserializing passkey for update")?;
                    if passkey.update_credential(&auth_result).unwrap_or(false) {
                        let updated_blob = serde_cbor_2::to_vec(&passkey)
                            .context("Serializing updated passkey")?;
                        let cred_id_for_save = cred_id_blob.clone();
                        self.conn
                            .call(move |conn| {
                                conn.execute(
                                    "UPDATE passkeys SET passkey = ?1 WHERE credential_id = ?2",
                                    rusqlite::params![updated_blob, cred_id_for_save],
                                )
                                .map_err(tokio_rusqlite::Error::Rusqlite)
                            })
                            .await?;
                    }
                }
            }

            let cred_id_for_user = cred_id_blob.clone();
            let user = self
                .conn
                .call(move |conn| {
                    conn.query_row(
                        r###"
                    SELECT u.id, u.username 
                    FROM users u
                    JOIN passkeys p ON p.user_id = u.id
                    WHERE p.credential_id = ?1
                    "###,
                        [cred_id_for_user],
                        |row| {
                            Ok(User {
                                id: row.get(0)?,
                                username: row.get(1)?,
                            })
                        },
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            Ok(user)
        }

        // --- Persistence Helpers ---

        async fn create_session(&self, user_id: String, title: String) -> Result<Session> {
            let id = Uuid::new_v4().to_string();
            let s = Session {
                id: id.clone(),
                user_id: user_id.clone(),
                title: title.clone(),
                date: "Just now".into(),
                preview: "Begin exploring what's here.".into(),
            };

            self.conn
                .call(move |conn| {
                    conn.execute(
                        "INSERT INTO sessions (id, user_id, title, preview) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![id, user_id, title, "Begin exploring what's here."],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            Ok(s)
        }

        async fn get_sessions(&self, user_id: String) -> Result<Vec<Session>> {
            self.conn.call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, created_at, user_id, preview FROM sessions WHERE user_id = ?1 ORDER BY updated_at DESC"
                )?;
                let rows = stmt.query_map([user_id], |row| {
                    let id: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    let date: String = row.get(2)?;
                    let uid: String = row.get(3)?;
                    let preview: String = row.get(4)?;
                    Ok(Session {
                        id,
                        user_id: uid,
                        title,
                        date,
                        preview,
                    })
                })?;
                let mut sessions = Vec::new();
                for r in rows {
                    sessions.push(r?);
                }
                Ok(sessions)
            }).await.context("Fetching sessions")
        }

        async fn save_message(
            &self,
            session_id: String,
            role: String,
            content: String,
        ) -> Result<()> {
            let session_id_for_touch = session_id.clone();
            self.conn
                .call(move |conn| {
                    conn.execute(
                        "INSERT INTO messages (session_id, role, content) VALUES (?1, ?2, ?3)",
                        rusqlite::params![session_id, role, content],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;
            self.conn
                .call(move |conn| {
                    conn.execute(
                        "UPDATE sessions SET updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                        [session_id_for_touch],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;
            Ok(())
        }

        async fn get_history(&self, session_id: String) -> Result<Vec<ChatLog>> {
            self.conn
                .call(move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT role, content FROM messages WHERE session_id = ?1 ORDER BY id ASC",
                    )?;
                    let rows = stmt.query_map([session_id], |row| {
                        Ok(ChatLog {
                            role: row.get(0)?,
                            content: row.get(1)?,
                        })
                    })?;
                    let mut logs = Vec::new();
                    for r in rows {
                        logs.push(r?);
                    }
                    Ok(logs)
                })
                .await
                .context("Fetching history")
        }

        async fn update_session_summary(
            &self,
            session_id: String,
            summary: SessionSummaryData,
        ) -> Result<()> {
            let title = if summary.title.trim().is_empty() {
                "New Session".to_string()
            } else {
                summary.title.trim().to_string()
            };
            let preview = if summary.preview.trim().is_empty() {
                "Begin exploring what's here.".to_string()
            } else {
                summary.preview.trim().to_string()
            };

            self.conn
                .call(move |conn| {
                    conn.execute(
                        "UPDATE sessions SET title = ?1, preview = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
                        rusqlite::params![title, preview, session_id],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Updating session summary")?;

            Ok(())
        }

        fn spawn_session_summary_update(self: &Arc<Self>, session_id: String) {
            let runtime = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(err) = runtime.sync_session_summary(session_id).await {
                    eprintln!("[session_summary_update] {}", err);
                }
            });
        }

        async fn sync_session_summary(&self, session_id: String) -> Result<()> {
            let logs = self.get_history(session_id.clone()).await?;
            if logs.is_empty() {
                return Ok(());
            }

            let transcript = compress_chat_logs(&logs, 12_000);
            if transcript.trim().is_empty() {
                return Ok(());
            }

            let model = std::env::var("SESSION_SUMMARY_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string());
            let extractor = self
                .openai_client
                .extractor::<SessionSummaryData>(model)
                .preamble(SESSION_SUMMARY_PROMPT)
                .build();

            let fallback_preview = fallback_session_preview(&logs);
            let summary = extractor
                .extract(transcript)
                .await
                .map_err(|err| anyhow::anyhow!("Session summary extraction failed: {}", err))
                .unwrap_or(SessionSummaryData {
                    title: "New Session".to_string(),
                    preview: fallback_preview,
                });

            self.update_session_summary(session_id, summary).await
        }

        async fn user_owns_session(&self, user_id: String, session_id: String) -> Result<bool> {
            self.conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1 AND user_id = ?2)",
                        rusqlite::params![session_id, user_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|value| value == 1)
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Checking session ownership")
        }

        async fn require_session_ownership(&self, user_id: &str, session_id: &str) -> Result<()> {
            if self
                .user_owns_session(user_id.to_string(), session_id.to_string())
                .await?
            {
                Ok(())
            } else {
                Err(anyhow::anyhow!("Unauthorized"))
            }
        }

        async fn get_relationship_profile(
            &self,
            user_id: String,
            slug: String,
        ) -> Result<Option<RelationshipProfile>> {
            let normalized_slug = normalize_slug(&slug);
            let user_id_for_query = user_id.clone();
            let slug_for_query = normalized_slug.clone();
            self.conn
                .call(move |conn| {
                    conn.query_row(
                        "SELECT profile_json FROM relationship_profiles WHERE user_id = ?1 AND slug = ?2",
                        rusqlite::params![user_id_for_query, slug_for_query],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Fetching relationship profile")?
                .map(|raw| {
                    let record: RelationshipProfileRecord =
                        serde_json::from_str(&raw).context("Parsing relationship profile JSON")?;
                    Ok(record.with_identity(user_id, normalized_slug))
                })
                .transpose()
        }

        async fn list_relationship_profiles(
            &self,
            user_id: String,
        ) -> Result<Vec<RelationshipProfile>> {
            let user_id_for_query = user_id.clone();
            self.conn
                .call(move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT slug, profile_json FROM relationship_profiles WHERE user_id = ?1 ORDER BY updated_at DESC",
                    )?;
                    let rows = stmt.query_map([user_id_for_query], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?;
                    let mut items = Vec::new();
                    for row in rows {
                        items.push(row?);
                    }
                    Ok(items)
                })
                .await
                .context("Listing relationship profiles")?
                .into_iter()
                .map(|(slug, raw)| {
                    let record: RelationshipProfileRecord =
                        serde_json::from_str(&raw).context("Parsing relationship profile JSON")?;
                    Ok(record.with_identity(user_id.clone(), slug))
                })
                .collect()
        }

        async fn upsert_relationship_profile(&self, profile: RelationshipProfile) -> Result<()> {
            let user_id = profile.user_id.clone();
            let slug = normalize_slug(&profile.slug);
            let display_name = if profile.display_name.trim().is_empty() {
                slug.replace('_', " ")
            } else {
                profile.display_name.trim().to_string()
            };
            let relationship_type = if profile.relationship_type.trim().is_empty() {
                slug.clone()
            } else {
                profile.relationship_type.trim().to_string()
            };
            let payload = serde_json::to_string(&RelationshipProfileRecord {
                display_name: display_name.clone(),
                relationship_type: relationship_type.clone(),
                background: profile.background.trim().to_string(),
                goals: profile.goals,
                triggers: profile.triggers,
                do_not_say: profile.do_not_say,
                effective_tone: profile.effective_tone,
                recent_events: profile.recent_events,
                boundaries: profile.boundaries,
            })
            .context("Serializing relationship profile")?;

            self.conn
                .call(move |conn| {
                    conn.execute(
                        r###"
                        INSERT INTO relationship_profiles (user_id, slug, display_name, relationship_type, profile_json)
                        VALUES (?1, ?2, ?3, ?4, ?5)
                        ON CONFLICT(user_id, slug)
                        DO UPDATE SET
                            display_name = excluded.display_name,
                            relationship_type = excluded.relationship_type,
                            profile_json = excluded.profile_json,
                            updated_at = CURRENT_TIMESTAMP
                        "###,
                        rusqlite::params![user_id, slug, display_name, relationship_type, payload],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Persisting relationship profile")?;

            Ok(())
        }

        async fn get_user_memory_logs(
            &self,
            user_id: String,
            limit: i64,
        ) -> Result<Vec<(String, String, String, String)>> {
            self.conn
                .call(move |conn| {
                    let mut stmt = conn.prepare(
                        r###"
                        SELECT m.role, m.content, s.title, COALESCE(m.created_at, s.updated_at, s.created_at, '')
                        FROM messages m
                        JOIN sessions s ON s.id = m.session_id
                        WHERE s.user_id = ?1
                        ORDER BY m.id DESC
                        LIMIT ?2
                        "###,
                    )?;
                    let rows = stmt.query_map(rusqlite::params![user_id, limit], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })?;
                    let mut items = Vec::new();
                    for row in rows {
                        items.push(row?);
                    }
                    Ok(items)
                })
                .await
                .context("Fetching user memory logs")
        }

        async fn extract_relationship_profiles_from_text(
            &self,
            source_text: String,
            existing_profiles: &[RelationshipProfile],
        ) -> Result<RelationshipProfileDelta> {
            let existing_context = if existing_profiles.is_empty() {
                "No saved relationship profiles yet.".to_string()
            } else {
                existing_profiles
                    .iter()
                    .map(|profile| {
                        format!(
                            "{} [{}] goals: {} | triggers: {} | boundaries: {}",
                            profile.display_name,
                            profile.slug,
                            join_items(&profile.goals),
                            join_items(&profile.triggers),
                            join_items(&profile.boundaries)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            let model = std::env::var("RELATIONSHIP_PROFILE_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string());
            let extractor = self
                .openai_client
                .extractor::<RelationshipProfileDelta>(model)
                .preamble(RELATIONSHIP_PROFILE_PROMPT)
                .context(&existing_context)
                .build();

            extractor
                .extract(source_text)
                .await
                .map_err(|err| anyhow::anyhow!("Relationship profile extraction failed: {}", err))
        }

        async fn sync_relationship_profiles_from_text(
            &self,
            user_id: String,
            source_text: String,
        ) -> Result<()> {
            if source_text.trim().is_empty() {
                return Ok(());
            }

            let existing_profiles = self.list_relationship_profiles(user_id.clone()).await?;
            let delta = self
                .extract_relationship_profiles_from_text(source_text, &existing_profiles)
                .await?;

            if delta.is_empty() {
                return Ok(());
            }

            let mut existing_by_slug: HashMap<String, RelationshipProfile> = existing_profiles
                .into_iter()
                .map(|profile| (profile.slug.clone(), profile))
                .collect();

            for extracted in delta.profiles {
                let slug = canonical_relationship_slug(
                    &extracted.slug,
                    &extracted.relationship_type,
                    &extracted.display_name,
                );
                let existing = existing_by_slug.remove(&slug);
                let merged = merge_relationship_profile(user_id.clone(), existing, extracted);
                existing_by_slug.insert(merged.slug.clone(), merged.clone());
                self.upsert_relationship_profile(merged).await?;
            }

            Ok(())
        }

        fn spawn_relationship_profile_update(
            self: &Arc<Self>,
            user_id: String,
            prompt: String,
            reply: String,
        ) {
            let runtime = Arc::clone(self);
            tokio::spawn(async move {
                let source_text = format!("User: {}\nAssistant: {}", prompt, reply);
                if let Err(err) = runtime
                    .sync_relationship_profiles_from_text(user_id, source_text)
                    .await
                {
                    eprintln!("[relationship_profile_update] {}", err);
                }
            });
        }

        async fn ensure_relationship_profile_bootstrapped(
            &self,
            user_id: String,
            relationship_slug: String,
        ) -> Result<Option<RelationshipProfile>> {
            let canonical_slug = canonical_relationship_slug(&relationship_slug, "", "");
            if let Some(profile) = self
                .get_relationship_profile(user_id.clone(), canonical_slug.clone())
                .await?
            {
                return Ok(Some(profile));
            }

            let logs = self.get_user_memory_logs(user_id.clone(), 180).await?;
            let source_text = compress_logs_for_profile_bootstrap(&logs, 16_000);
            self.sync_relationship_profiles_from_text(user_id.clone(), source_text)
                .await?;

            self.get_relationship_profile(user_id, canonical_slug).await
        }

        async fn build_draft_context(
            &self,
            user_id: String,
            session_id: String,
            relationship_slug: String,
            user_request: String,
            intent: String,
        ) -> Result<String> {
            let normalized_slug = normalize_slug(&relationship_slug);
            let profile = self
                .ensure_relationship_profile_bootstrapped(user_id.clone(), normalized_slug.clone())
                .await?;
            let recent_history = self.get_history(session_id).await.unwrap_or_default();
            let all_logs = self.get_user_memory_logs(user_id.clone(), 250).await?;
            let graph = self.read_patient_graph(user_id).await.unwrap_or_default();

            let display_name = profile
                .as_ref()
                .map(|item| item.display_name.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&normalized_slug);
            let relationship_type = profile
                .as_ref()
                .map(|item| item.relationship_type.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&normalized_slug);
            let display_name_owned = display_name.to_string();
            let relationship_type_owned = relationship_type.to_string();
            let aliases = relationship_aliases(&normalized_slug, relationship_type, display_name);

            let mut query_terms = tokenize(&format!(
                "{} {} {} {}",
                user_request, intent, display_name, relationship_type
            ));
            for alias in &aliases {
                query_terms.extend(tokenize(alias));
            }

            let recent_lines: Vec<String> = recent_history
                .iter()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|log| format!("{}: {}", log.role, log.content))
                .collect();

            let mut memory_candidates = Vec::new();
            for (role, content, title, created_at) in all_logs {
                if content.trim().is_empty() {
                    continue;
                }
                let mut score = overlap_score(&content, &query_terms);
                let lowered = content.to_lowercase();
                for alias in &aliases {
                    if lowered.contains(alias) {
                        score += 4;
                    }
                }
                if role == "user" {
                    score += 1;
                }
                if score <= 0 {
                    continue;
                }
                memory_candidates.push(MemoryCandidate {
                    score,
                    summary: format!("[{} | {}] {}: {}", created_at, title, role, content),
                });
            }
            memory_candidates.sort_by(|left, right| right.score.cmp(&left.score));
            let broader_memories: Vec<String> = memory_candidates
                .into_iter()
                .take(8)
                .map(|item| item.summary)
                .collect();

            let mut graph_candidates = Vec::new();
            for node in &graph.nodes {
                let text = format!("{} {} {}", node.id, node.label, node.category);
                let mut score = overlap_score(&text, &query_terms);
                let lowered = text.to_lowercase();
                for alias in &aliases {
                    if lowered.contains(alias) {
                        score += 4;
                    }
                }
                if score > 0 {
                    graph_candidates.push(MemoryCandidate {
                        score,
                        summary: format!("Node: {} [{}]", node.label, node.category),
                    });
                }
            }
            for edge in &graph.edges {
                let text = format!("{} {} {}", edge.from, edge.to, edge.relation);
                let mut score = overlap_score(&text, &query_terms);
                let lowered = text.to_lowercase();
                for alias in &aliases {
                    if lowered.contains(alias) {
                        score += 4;
                    }
                }
                if score > 0 {
                    graph_candidates.push(MemoryCandidate {
                        score,
                        summary: format!("Edge: {} -> {} ({})", edge.from, edge.to, edge.relation),
                    });
                }
            }
            graph_candidates.sort_by(|left, right| right.score.cmp(&left.score));
            let graph_hits: Vec<String> = graph_candidates
                .into_iter()
                .take(6)
                .map(|item| item.summary)
                .collect();

            let profile_section = if let Some(profile) = profile {
                format!(
                    "Display name: {}\nRelationship type: {}\nBackground: {}\nGoals: {}\nTriggers: {}\nAvoid: {}\nEffective tone: {}\nRecent events: {}\nBoundaries: {}",
                    profile.display_name,
                    profile.relationship_type,
                    if profile.background.trim().is_empty() { "none" } else { profile.background.trim() },
                    join_items(&profile.goals),
                    join_items(&profile.triggers),
                    join_items(&profile.do_not_say),
                    join_items(&profile.effective_tone),
                    join_items(&profile.recent_events),
                    join_items(&profile.boundaries),
                )
            } else {
                format!(
                    "No saved relationship profile yet. Infer context from the broader memory and recent conversation. Relationship target: {}.",
                    display_name_owned
                )
            };

            Ok(format!(
                "Relationship target: {} ({})\nIntent: {}\nUser request: {}\n\nRelationship profile:\n{}\n\nRecent conversation:\n{}\n\nBroader autobiographical memories:\n{}\n\nRelevant graph memories:\n{}",
                display_name_owned,
                relationship_type_owned,
                intent,
                user_request,
                profile_section,
                if recent_lines.is_empty() { "none".to_string() } else { recent_lines.join("\n") },
                if broader_memories.is_empty() { "none".to_string() } else { broader_memories.join("\n") },
                if graph_hits.is_empty() { "none".to_string() } else { graph_hits.join("\n") },
            ))
        }

        async fn read_patient_graph(&self, user_id: String) -> Result<PatientGraph> {
            self.graph_reader
                .call(GraphReadArgs { user_id })
                .await
                .context("Reading patient graph")
        }

        fn spawn_graph_update(self: &Arc<Self>, user_id: String, prompt: String, reply: String) {
            let runtime = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(err) = runtime
                    .update_graph_from_exchange(user_id, prompt, reply)
                    .await
                {
                    eprintln!("[graph_update] {}", err);
                }
            });
        }

        async fn update_graph_from_exchange(
            &self,
            user_id: String,
            prompt: String,
            reply: String,
        ) -> Result<()> {
            let current_graph = self.read_patient_graph(user_id.clone()).await?;
            let context = graph_context(&current_graph);

            let model = std::env::var("GRAPH_EXTRACTOR_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string());
            let extractor = self
                .openai_client
                .extractor::<ConversationGraphDelta>(model)
                .preamble(GRAPH_DELTA_PROMPT)
                .context(&context)
                .build();

            let transcript = format!("User: {}\nAssistant: {}", prompt, reply);
            let delta = extractor
                .extract(transcript)
                .await
                .map_err(|err| anyhow::anyhow!("Extractor failed: {}", err))?;

            if delta.is_empty() {
                return Ok(());
            }

            let update = GraphUpdateArgs {
                user_id,
                new_nodes: delta.new_concepts,
                new_edges: delta.new_connections,
                nodes_to_remove_ids: delta.obsolete_concept_ids,
                edges_to_remove: delta.obsolete_connections,
            };
            let summary = self.graph_writer.call(update).await?;
            eprintln!(
                "[graph_update] +{} nodes +{} edges -{} nodes -{} edges ({} nodes, {} edges)",
                summary.added_nodes,
                summary.added_edges,
                summary.removed_nodes,
                summary.removed_edges,
                summary.total_nodes,
                summary.total_edges
            );
            Ok(())
        }

        // --- Agent Logic ---

        pub async fn respond(
            self: &Arc<Self>,
            user_id: &str,
            session_id: &str,
            prompt: String,
        ) -> Result<String> {
            self.require_session_ownership(user_id, session_id).await?;
            self.save_message(session_id.to_string(), "user".into(), prompt.clone())
                .await?;

            let mut history = {
                let mut guard = self.histories.write().await;
                if !guard.contains_key(session_id) {
                    let db_logs = self.get_history(session_id.to_string()).await?;
                    let mut msgs = Vec::new();
                    for log in db_logs {
                        if log.role == "user" {
                            msgs.push(Message::user(log.content));
                        } else {
                            msgs.push(Message::Assistant {
                                id: None,
                                content: rig::OneOrMany::one(AssistantContent::Text(Text {
                                    text: log.content,
                                })),
                            });
                        }
                    }
                    guard.insert(session_id.to_string(), msgs);
                }
                guard.remove(session_id).unwrap_or_default()
            };

            let reply = self
                .therapist_agent
                .prompt(Message::user(prompt.clone())) // Prompt is user message
                .with_history(&mut history)
                .multi_turn(2)
                .await
                .context("Running agent prompt")?;

            self.save_message(session_id.to_string(), "assistant".into(), reply.clone())
                .await?;

            let mut guard = self.histories.write().await;
            guard.insert(session_id.to_string(), history);

            self.spawn_session_summary_update(session_id.to_string());
            self.spawn_graph_update(user_id.to_string(), prompt.clone(), reply.clone());
            self.spawn_relationship_profile_update(user_id.to_string(), prompt, reply.clone());
            Ok(reply)
        }

        pub async fn draft_message(
            self: &Arc<Self>,
            user_id: &str,
            session_id: &str,
            relationship_slug: String,
            intent: String,
            prompt: String,
            accountability: i32,
            spirituality: i32,
            directness: i32,
        ) -> Result<String> {
            self.require_session_ownership(user_id, session_id).await?;
            let request_label = format!(
                "Draft request [{} / {}]: {}",
                relationship_slug, intent, prompt
            );
            self.save_message(session_id.to_string(), "user".into(), request_label.clone())
                .await?;

            let draft_context = self
                .build_draft_context(
                    user_id.to_string(),
                    session_id.to_string(),
                    relationship_slug.clone(),
                    prompt.clone(),
                    intent.clone(),
                )
                .await?;

            let mut history = {
                let mut guard = self.histories.write().await;
                if !guard.contains_key(session_id) {
                    let db_logs = self.get_history(session_id.to_string()).await?;
                    let mut msgs = Vec::new();
                    for log in db_logs {
                        if log.role == "user" {
                            msgs.push(Message::user(log.content));
                        } else {
                            msgs.push(Message::Assistant {
                                id: None,
                                content: rig::OneOrMany::one(AssistantContent::Text(Text {
                                    text: log.content,
                                })),
                            });
                        }
                    }
                    guard.insert(session_id.to_string(), msgs);
                }
                guard.remove(session_id).unwrap_or_default()
            };

            let draft_prompt = format!(
                "{}\n\nControls -> accountability: {}, spirituality: {}, directness: {}.",
                draft_context, accountability, spirituality, directness
            );

            let reply = self
                .draft_agent
                .prompt(Message::user(draft_prompt))
                .with_history(&mut history)
                .multi_turn(2)
                .await
                .context("Running draft prompt")?;

            self.save_message(session_id.to_string(), "assistant".into(), reply.clone())
                .await?;

            let mut guard = self.histories.write().await;
            guard.insert(session_id.to_string(), history);

            self.spawn_session_summary_update(session_id.to_string());
            self.spawn_graph_update(user_id.to_string(), request_label.clone(), reply.clone());
            self.spawn_relationship_profile_update(
                user_id.to_string(),
                request_label,
                reply.clone(),
            );
            Ok(reply)
        }

        pub async fn draft_stream(
            self: &Arc<Self>,
            user_id: String,
            session_id: String,
            relationship_slug: String,
            intent: String,
            prompt: String,
            accountability: i32,
            spirituality: i32,
            directness: i32,
        ) -> Result<ReceiverStream<Result<String, std::convert::Infallible>>> {
            self.require_session_ownership(&user_id, &session_id)
                .await?;
            let request_label = format!(
                "Draft request [{} / {}]: {}",
                relationship_slug, intent, prompt
            );
            self.save_message(session_id.clone(), "user".into(), request_label.clone())
                .await?;

            let draft_context = self
                .build_draft_context(
                    user_id.clone(),
                    session_id.clone(),
                    relationship_slug.clone(),
                    prompt.clone(),
                    intent.clone(),
                )
                .await?;

            let mut history = {
                let mut guard = self.histories.write().await;
                if !guard.contains_key(&session_id) {
                    let db_logs = self.get_history(session_id.clone()).await?;
                    let mut msgs = Vec::new();
                    for log in db_logs {
                        if log.role == "user" {
                            msgs.push(Message::user(log.content));
                        } else {
                            msgs.push(Message::Assistant {
                                id: None,
                                content: rig::OneOrMany::one(AssistantContent::Text(Text {
                                    text: log.content,
                                })),
                            });
                        }
                    }
                    guard.insert(session_id.clone(), msgs);
                }
                guard.remove(&session_id).unwrap_or_default()
            };

            let draft_prompt = format!(
                "{}\n\nControls -> accountability: {}, spirituality: {}, directness: {}.",
                draft_context, accountability, spirituality, directness
            );

            let mut stream = self
                .draft_agent
                .stream_prompt(Message::user(draft_prompt.clone()))
                .with_history(history.clone())
                .multi_turn(2)
                .await;

            let (tx, rx) = mpsc::channel(16);
            let runtime = Arc::clone(self);
            let session_id_clone = session_id.clone();
            let user_id_clone = user_id.clone();

            tokio::spawn(async move {
                let mut assembled = String::new();
                let mut final_text = None;
                loop {
                    let next = timeout(Duration::from_secs(10), stream.next()).await;
                    let chunk = match next {
                        Ok(val) => val,
                        Err(_) => {
                            eprintln!("[draft-stream:timeout] no chunk within 10s, falling back");
                            break;
                        }
                    };

                    match chunk {
                        Some(Ok(rig::agent::MultiTurnStreamItem::StreamAssistantItem(
                            rig::streaming::StreamedAssistantContent::Text(delta),
                        ))) => {
                            assembled.push_str(&delta.text);
                            let _ = tx.send(Ok(delta.text)).await;
                        }
                        Some(Ok(rig::agent::MultiTurnStreamItem::FinalResponse(resp))) => {
                            final_text.get_or_insert_with(|| resp.response().to_string());
                            break;
                        }
                        Some(Err(e)) => {
                            eprintln!("[draft-stream:error] {}", e);
                            let _ = tx.send(Ok(format!("[error:{}]", e))).await;
                            break;
                        }
                        Some(_) => {}
                        None => break,
                    }
                }

                let final_content = if let Some(text) = final_text {
                    text
                } else if !assembled.is_empty() {
                    assembled
                } else {
                    String::new()
                };

                if !final_content.is_empty() {
                    let _ = runtime
                        .save_message(
                            session_id_clone.clone(),
                            "assistant".into(),
                            final_content.clone(),
                        )
                        .await;
                    runtime.spawn_session_summary_update(session_id_clone.clone());
                }

                runtime.spawn_graph_update(
                    user_id_clone.clone(),
                    request_label.clone(),
                    final_content.clone(),
                );
                runtime.spawn_relationship_profile_update(
                    user_id_clone,
                    request_label.clone(),
                    final_content.clone(),
                );
                let _ = tx.send(Ok("[DONE]".to_string())).await;

                history.push(Message::user(draft_prompt));
                if !final_content.is_empty() {
                    history.push(Message::Assistant {
                        id: None,
                        content: rig::OneOrMany::one(AssistantContent::Text(Text {
                            text: final_content,
                        })),
                    });
                }
                let mut guard = runtime.histories.write().await;
                guard.insert(session_id_clone, history);
            });

            Ok(ReceiverStream::new(rx))
        }

        pub async fn stream(
            self: &Arc<Self>,
            user_id: String,
            session_id: String,
            prompt: String,
        ) -> Result<ReceiverStream<Result<String, std::convert::Infallible>>> {
            self.require_session_ownership(&user_id, &session_id)
                .await?;
            self.save_message(session_id.clone(), "user".into(), prompt.clone())
                .await?;

            let mut history = {
                let mut guard = self.histories.write().await;
                if !guard.contains_key(&session_id) {
                    let db_logs = self.get_history(session_id.clone()).await?;
                    let mut msgs = Vec::new();
                    for log in db_logs {
                        if log.role == "user" {
                            msgs.push(Message::user(log.content));
                        } else {
                            msgs.push(Message::Assistant {
                                id: None,
                                content: rig::OneOrMany::one(AssistantContent::Text(Text {
                                    text: log.content,
                                })),
                            });
                        }
                    }
                    guard.insert(session_id.clone(), msgs);
                }
                guard.remove(&session_id).unwrap_or_default()
            };

            let mut stream = self
                .therapist_agent
                .stream_prompt(Message::user(prompt.clone()))
                .with_history(history.clone())
                .multi_turn(2)
                .await;

            let (tx, rx) = mpsc::channel(16);
            let runtime = Arc::clone(self);
            let session_id_clone = session_id.clone();
            let user_id_clone = user_id.clone();

            tokio::spawn(async move {
                let mut assembled = String::new();
                let mut final_text = None;
                loop {
                    let next = timeout(Duration::from_secs(10), stream.next()).await;
                    let chunk = match next {
                        Ok(val) => val,
                        Err(_) => {
                            eprintln!("[agent-stream:timeout] no chunk within 10s, falling back");
                            break;
                        }
                    };

                    match chunk {
                        Some(Ok(rig::agent::MultiTurnStreamItem::StreamAssistantItem(
                            rig::streaming::StreamedAssistantContent::Text(delta),
                        ))) => {
                            assembled.push_str(&delta.text);
                            let _ = tx.send(Ok(delta.text)).await;
                        }
                        Some(Ok(rig::agent::MultiTurnStreamItem::FinalResponse(resp))) => {
                            final_text.get_or_insert_with(|| resp.response().to_string());
                            break;
                        }
                        Some(Err(e)) => {
                            eprintln!("[agent-stream:error] {}", e);
                            let _ = tx.send(Ok(format!("[error:{}]", e))).await;
                            break;
                        }
                        Some(_) => {} // Ignore other stream items
                        None => break,
                    }
                }

                let final_content = if let Some(text) = final_text {
                    text
                } else if !assembled.is_empty() {
                    assembled
                } else {
                    String::new()
                };

                if !final_content.is_empty() {
                    let _ = runtime
                        .save_message(
                            session_id_clone.clone(),
                            "assistant".into(),
                            final_content.clone(),
                        )
                        .await;
                    runtime.spawn_session_summary_update(session_id_clone.clone());
                }

                runtime.spawn_graph_update(
                    user_id_clone.clone(),
                    prompt.clone(),
                    final_content.clone(),
                );
                runtime.spawn_relationship_profile_update(
                    user_id_clone,
                    prompt.clone(),
                    final_content.clone(),
                );
                let _ = tx.send(Ok("[DONE]".to_string())).await;

                history.push(Message::user(prompt));
                if !final_content.is_empty() {
                    history.push(Message::Assistant {
                        id: None,
                        content: rig::OneOrMany::one(AssistantContent::Text(Text {
                            text: final_content,
                        })),
                    });
                }
                let mut guard = runtime.histories.write().await;
                guard.insert(session_id_clone, history);
            });

            Ok(ReceiverStream::new(rx))
        }

        // --- Public Helpers ---
        pub async fn list_sessions(&self, user_id: String) -> Result<Vec<Session>> {
            self.get_sessions(user_id).await
        }

        pub async fn create_new_session(&self, user_id: String, title: String) -> Result<Session> {
            self.create_session(user_id, title).await
        }

        pub async fn get_session_history(
            &self,
            user_id: String,
            id: String,
        ) -> Result<Vec<ChatLog>> {
            self.require_session_ownership(&user_id, &id).await?;
            self.get_history(id).await
        }

        pub async fn get_patient_graph(&self, user_id: String) -> Result<PatientGraph> {
            self.read_patient_graph(user_id).await
        }

        pub async fn get_relationship_profiles(
            &self,
            user_id: String,
        ) -> Result<Vec<RelationshipProfile>> {
            self.list_relationship_profiles(user_id).await
        }

        pub async fn save_relationship_profile(&self, profile: RelationshipProfile) -> Result<()> {
            self.upsert_relationship_profile(profile).await
        }

        // Auth Helpers
        pub async fn signup(&self, u: String, p: String) -> Result<User> {
            self.create_user(u, p).await
        }
        pub async fn login(&self, u: String, p: String) -> Result<User> {
            self.verify_user(u, p).await
        }
        pub async fn get_user(&self, id: String) -> Result<User> {
            self.get_user_by_id(id).await
        }
    }

    static GLOBAL_AGENT: OnceCell<Arc<AgentRuntime>> = OnceCell::const_new();

    pub async fn agent_runtime() -> Result<Arc<AgentRuntime>> {
        GLOBAL_AGENT
            .get_or_try_init(|| async { AgentRuntime::new().await.map(Arc::new) })
            .await
            .cloned()
    }

    // -- Auth Middleware Helpers --

    pub async fn get_current_user_id() -> Result<String, ServerFnError> {
        let headers: HeaderMap = leptos_axum::extract().await?;
        let key = cookie_key();
        let jar = PrivateCookieJar::from_headers(&headers, key);

        if let Some(cookie) = jar.get(AUTH_COOKIE_NAME) {
            Ok(cookie.value().to_string())
        } else {
            Err(ServerFnError::ServerError("Unauthorized".into()))
        }
    }

    pub fn cookie_is_secure(headers: &HeaderMap) -> bool {
        if let Some(proto) = headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
        {
            return proto.eq_ignore_ascii_case("https");
        }

        if let Some(forwarded) = headers
            .get("forwarded")
            .and_then(|value| value.to_str().ok())
        {
            return forwarded.to_ascii_lowercase().contains("proto=https");
        }

        false
    }

    #[derive(Deserialize)]
    pub struct StreamParams {
        pub prompt: String,
        pub session_id: String,
        pub user_id: String,
    }

    #[derive(Deserialize)]
    pub struct DraftStreamParams {
        pub prompt: String,
        pub session_id: String,
        pub user_id: String,
        pub relationship_slug: String,
        pub intent: String,
        pub accountability: i32,
        pub spirituality: i32,
        pub directness: i32,
    }

    pub async fn stream_handler(
        Query(params): Query<StreamParams>,
        headers: axum::http::HeaderMap,
    ) -> Result<
        Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>,
        (StatusCode, String),
    > {
        let key = cookie_key();
        let jar = PrivateCookieJar::from_headers(&headers, key);

        if let Some(cookie) = jar.get(AUTH_COOKIE_NAME) {
            if cookie.value() != params.user_id {
                return Err((StatusCode::UNAUTHORIZED, "User mismatch".into()));
            }
        } else {
            return Err((StatusCode::UNAUTHORIZED, "No auth cookie".into()));
        }

        let runtime = agent_runtime().await.map_err(internal_err)?;
        let stream = runtime
            .stream(params.user_id, params.session_id, params.prompt)
            .await
            .map_err(internal_err)?;

        let mapped = stream.map(|res| {
            let data = res.unwrap_or_else(|_| "[error]".to_string());
            Ok(Event::default().data(data))
        });

        Ok(Sse::new(mapped))
    }

    pub async fn graph_handler(
        Path(user_id): Path<String>,
        headers: axum::http::HeaderMap,
    ) -> Result<Json<PatientGraph>, (StatusCode, String)> {
        let key = cookie_key();
        let jar = PrivateCookieJar::from_headers(&headers, key);

        if let Some(cookie) = jar.get(AUTH_COOKIE_NAME) {
            if cookie.value() != user_id {
                return Err((StatusCode::UNAUTHORIZED, "User mismatch".into()));
            }
        } else {
            return Err((StatusCode::UNAUTHORIZED, "No auth cookie".into()));
        }

        let runtime = agent_runtime().await.map_err(internal_err)?;
        let graph = runtime
            .get_patient_graph(user_id)
            .await
            .map_err(internal_err)?;
        Ok(Json(graph))
    }

    pub async fn draft_stream_handler(
        Query(params): Query<DraftStreamParams>,
        headers: axum::http::HeaderMap,
    ) -> Result<
        Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>,
        (StatusCode, String),
    > {
        let key = cookie_key();
        let jar = PrivateCookieJar::from_headers(&headers, key);

        if let Some(cookie) = jar.get(AUTH_COOKIE_NAME) {
            if cookie.value() != params.user_id {
                return Err((StatusCode::UNAUTHORIZED, "User mismatch".into()));
            }
        } else {
            return Err((StatusCode::UNAUTHORIZED, "No auth cookie".into()));
        }

        let runtime = agent_runtime().await.map_err(internal_err)?;
        let stream = runtime
            .draft_stream(
                params.user_id,
                params.session_id,
                params.relationship_slug,
                params.intent,
                params.prompt,
                params.accountability,
                params.spirituality,
                params.directness,
            )
            .await
            .map_err(internal_err)?;

        let mapped = stream.map(|res| {
            let data = res.unwrap_or_else(|_| "[error]".to_string());
            Ok(Event::default().data(data))
        });

        Ok(Sse::new(mapped))
    }

    fn internal_err(e: impl std::fmt::Display) -> (StatusCode, String) {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

#[cfg(feature = "ssr")]
pub use runtime::{agent_runtime, cookie_key, draft_stream_handler, graph_handler, stream_handler};

#[cfg(feature = "ssr")]
fn server_error(err: impl std::fmt::Display) -> ServerFnError {
    let message = format!("{:#}", err);
    eprintln!("[agent_serverfn] {}", message);
    ServerFnError::ServerError(message)
}

#[server(GetContextUser, "/api")]
pub async fn get_context_user() -> Result<Option<User>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use axum_extra::extract::cookie::{Key, PrivateCookieJar};
        let headers: axum::http::HeaderMap = leptos_axum::extract().await?;
        let key = runtime::cookie_key();
        let jar = PrivateCookieJar::from_headers(&headers, key);

        if let Some(cookie) = jar.get(runtime::AUTH_COOKIE_NAME) {
            let user_id = cookie.value().to_string();
            let agent = agent_runtime().await.map_err(server_error)?;
            match agent.get_user(user_id).await {
                Ok(u) => Ok(Some(u)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
    #[cfg(not(feature = "ssr"))]
    Err(ServerFnError::ServerError("SSR only".into()))
}

#[server(Login, "/api")]
pub async fn login(username: String, pass: String) -> Result<User, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        let user = agent
            .login(username, pass)
            .await
            .map_err(|_| server_error("Invalid credentials"))?;

        use axum_extra::extract::cookie::{Cookie, Key};
        use cookie::CookieJar;
        use leptos_axum::ResponseOptions;

        let headers: axum::http::HeaderMap = leptos_axum::extract().await?;
        let secure_cookie = runtime::cookie_is_secure(&headers);
        let key = runtime::cookie_key();

        let cookie = Cookie::build((runtime::AUTH_COOKIE_NAME, user.id.clone()))
            .path("/")
            .secure(secure_cookie)
            .http_only(true)
            .max_age(time::Duration::days(30))
            .build();

        let mut jar = CookieJar::new();
        jar.private_mut(&key).add(cookie);

        if let Some(opts) = leptos::use_context::<ResponseOptions>() {
            for cookie in jar.delta() {
                let header_val = cookie.encoded().to_string();
                opts.append_header(
                    axum::http::header::SET_COOKIE,
                    axum::http::HeaderValue::from_str(&header_val).map_err(server_error)?,
                );
            }
        }

        Ok(user)
    }
    #[cfg(not(feature = "ssr"))]
    Err(ServerFnError::ServerError("SSR only".into()))
}

#[server(Signup, "/api")]
pub async fn signup(username: String, pass: String) -> Result<User, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        match agent.signup(username, pass).await {
            Ok(user) => {
                use axum_extra::extract::cookie::{Cookie, Key};
                use cookie::CookieJar;
                use leptos_axum::ResponseOptions;

                let headers: axum::http::HeaderMap = leptos_axum::extract().await?;
                let secure_cookie = runtime::cookie_is_secure(&headers);
                let key = runtime::cookie_key();

                let cookie = Cookie::build((runtime::AUTH_COOKIE_NAME, user.id.clone()))
                    .path("/")
                    .secure(secure_cookie)
                    .http_only(true)
                    .max_age(time::Duration::days(30))
                    .build();

                let mut jar = CookieJar::new();
                jar.private_mut(&key).add(cookie);

                if let Some(opts) = leptos::use_context::<ResponseOptions>() {
                    for cookie in jar.delta() {
                        let header_val = cookie.encoded().to_string();
                        opts.append_header(
                            axum::http::header::SET_COOKIE,
                            axum::http::HeaderValue::from_str(&header_val).map_err(server_error)?,
                        );
                    }
                }
                Ok(user)
            }
            Err(e) => Err(server_error(e)),
        }
    }
    #[cfg(not(feature = "ssr"))]
    Err(ServerFnError::ServerError("SSR only".into()))
}

#[server(Logout, "/api")]
pub async fn logout() -> Result<(), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        use axum_extra::extract::cookie::{Cookie, Key};
        use cookie::CookieJar;
        use leptos_axum::ResponseOptions;

        let headers: axum::http::HeaderMap = leptos_axum::extract().await?;
        let secure_cookie = runtime::cookie_is_secure(&headers);
        let key = runtime::cookie_key();

        let mut jar = CookieJar::new();
        let mut removal = Cookie::build((runtime::AUTH_COOKIE_NAME, ""))
            .path("/")
            .secure(secure_cookie)
            .http_only(true)
            .max_age(time::Duration::seconds(0))
            .build();
        removal.make_removal();
        jar.private_mut(&key).add(removal);

        if let Some(opts) = leptos::use_context::<ResponseOptions>() {
            for cookie in jar.delta() {
                let header_val = cookie.encoded().to_string();
                opts.append_header(
                    axum::http::header::SET_COOKIE,
                    axum::http::HeaderValue::from_str(&header_val).map_err(server_error)?,
                );
            }
        }

        Ok(())
    }
    #[cfg(not(feature = "ssr"))]
    Err(ServerFnError::ServerError("SSR only".into()))
}

#[server(AgentChat, "/api")]
pub async fn agent_chat(prompt: String, session_id: String) -> Result<String, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        return match agent.respond(&user_id, &session_id, prompt.clone()).await {
            Ok(resp) => Ok(resp),
            Err(e) => Err(server_error(e)),
        };
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (prompt, session_id);
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(DraftMessage, "/api")]
pub async fn draft_message(
    prompt: String,
    session_id: String,
    relationship_slug: String,
    intent: String,
    accountability: i32,
    spirituality: i32,
    directness: i32,
) -> Result<String, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        return agent
            .draft_message(
                &user_id,
                &session_id,
                relationship_slug,
                intent,
                prompt,
                accountability,
                spirituality,
                directness,
            )
            .await
            .map_err(server_error);
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (
            prompt,
            session_id,
            relationship_slug,
            intent,
            accountability,
            spirituality,
            directness,
        );
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(GetSessions, "/api")]
pub async fn get_sessions() -> Result<Vec<Session>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent.list_sessions(user_id).await.map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(CreateSession, "/api")]
pub async fn create_session(title: String) -> Result<Session, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .create_new_session(user_id, title)
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = title;
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(GetChatHistory, "/api")]
pub async fn get_chat_history(session_id: String) -> Result<Vec<ChatLog>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .get_session_history(user_id, session_id)
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = session_id;
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(GetPatientGraph, "/api")]
pub async fn get_patient_graph(user_id: String) -> Result<PatientGraph, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        // Ensure requestor is the user
        let current_uid = runtime::get_current_user_id().await?;
        if current_uid != user_id {
            return Err(ServerFnError::ServerError("Unauthorized".into()));
        }

        let agent = agent_runtime().await.map_err(server_error)?;
        agent.get_patient_graph(user_id).await.map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = user_id;
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(GetRelationshipProfiles, "/api")]
pub async fn get_relationship_profiles() -> Result<Vec<RelationshipProfile>, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .get_relationship_profiles(user_id)
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(SaveRelationshipProfile, "/api")]
pub async fn save_relationship_profile(
    slug: String,
    display_name: String,
    relationship_type: String,
    background: String,
    goals: Vec<String>,
    triggers: Vec<String>,
    do_not_say: Vec<String>,
    effective_tone: Vec<String>,
    recent_events: Vec<String>,
    boundaries: Vec<String>,
) -> Result<(), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .save_relationship_profile(RelationshipProfile {
                user_id,
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
            })
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (
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
        );
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

// --- Passkey Server Functions ---

#[server(StartPasskeyRegister, "/api")]
pub async fn start_passkey_register(
) -> Result<(String, webauthn_rs_proto::CreationChallengeResponse), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let user_id = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .start_passkey_registration(user_id)
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    Err(ServerFnError::ServerError("SSR only".into()))
}

#[server(FinishPasskeyRegister, "/api")]
pub async fn finish_passkey_register(
    req_id: String,
    response: webauthn_rs_proto::RegisterPublicKeyCredential,
) -> Result<(), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let _ = runtime::get_current_user_id().await?;
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .finish_passkey_registration(req_id, response)
            .await
            .map_err(server_error)?;
        Ok(())
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (req_id, response);
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(StartPasskeyRegisterEmail, "/api")]
pub async fn start_passkey_register_email(
    email: String,
) -> Result<(String, webauthn_rs_proto::CreationChallengeResponse), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .start_passkey_registration_email(email)
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = email;
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(FinishPasskeyRegisterEmail, "/api")]
pub async fn finish_passkey_register_email(
    req_id: String,
    response: webauthn_rs_proto::RegisterPublicKeyCredential,
) -> Result<(), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        let user = agent
            .finish_passkey_registration(req_id, response)
            .await
            .map_err(server_error)?;

        use axum_extra::extract::cookie::{Cookie, Key};
        use cookie::CookieJar;
        use leptos_axum::ResponseOptions;

        let headers: axum::http::HeaderMap = leptos_axum::extract().await?;
        let secure_cookie = runtime::cookie_is_secure(&headers);
        let key = runtime::cookie_key();

        let cookie = Cookie::build((runtime::AUTH_COOKIE_NAME, user.id.clone()))
            .path("/")
            .secure(secure_cookie)
            .http_only(true)
            .max_age(time::Duration::days(30))
            .build();

        let mut jar = CookieJar::new();
        jar.private_mut(&key).add(cookie);

        if let Some(opts) = leptos::use_context::<ResponseOptions>() {
            for cookie in jar.delta() {
                let header_val = cookie.encoded().to_string();
                opts.append_header(
                    axum::http::header::SET_COOKIE,
                    axum::http::HeaderValue::from_str(&header_val).map_err(server_error)?,
                );
            }
        }

        Ok(())
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (req_id, response);
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(StartPasskeyLogin, "/api")]
pub async fn start_passkey_login(
    username: String,
) -> Result<(String, webauthn_rs_proto::RequestChallengeResponse), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        agent
            .start_passkey_login(username)
            .await
            .map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = username;
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}

#[server(FinishPasskeyLogin, "/api")]
pub async fn finish_passkey_login(
    req_id: String,
    response: webauthn_rs_proto::PublicKeyCredential,
) -> Result<User, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        let user = agent
            .finish_passkey_login(req_id, response)
            .await
            .map_err(server_error)?;

        // Auto-login (Cookie) logic copied from login/signup
        use axum_extra::extract::cookie::{Cookie, Key};
        use cookie::CookieJar;
        use leptos_axum::ResponseOptions;

        let headers: axum::http::HeaderMap = leptos_axum::extract().await?;
        let secure_cookie = runtime::cookie_is_secure(&headers);
        let key = runtime::cookie_key();

        let cookie = Cookie::build((runtime::AUTH_COOKIE_NAME, user.id.clone()))
            .path("/")
            .secure(secure_cookie)
            .http_only(true)
            .max_age(time::Duration::days(30))
            .build();

        let mut jar = CookieJar::new();
        jar.private_mut(&key).add(cookie);

        if let Some(opts) = leptos::use_context::<ResponseOptions>() {
            for cookie in jar.delta() {
                let header_val = cookie.encoded().to_string();
                opts.append_header(
                    axum::http::header::SET_COOKIE,
                    axum::http::HeaderValue::from_str(&header_val).map_err(server_error)?,
                );
            }
        }
        Ok(user)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = (req_id, response);
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}
