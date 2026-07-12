use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_GRAPH_USER_ID: &str = "local-user";
pub const AUTH_COOKIE_NAME: &str = "auth_token";

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub category: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SocialGraph {
    pub user_id: String,
    pub nodes: Vec<SocialGraphNode>,
    pub edges: Vec<SocialGraphEdge>,
}

impl Default for SocialGraph {
    fn default() -> Self {
        Self {
            user_id: DEFAULT_GRAPH_USER_ID.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SocialGraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub detail: String,
    pub weight: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SocialGraphEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub weight: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub mind_nodes: usize,
    pub mind_edges: usize,
    pub mind_signature: String,
    pub social_nodes: usize,
    pub social_edges: usize,
    pub social_signature: String,
    #[serde(default)]
    pub episode_count: usize,
    #[serde(default)]
    pub memory_signature: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    pub user_id: String,
    pub id: String,
    pub title: String,
    pub narrative: String,
    pub occurred_at: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub user_quotes: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryLink {
    pub user_id: String,
    pub from_kind: String,
    pub from_id: String,
    pub relation: String,
    pub to_kind: String,
    pub to_id: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default = "default_memory_link_weight")]
    pub weight: usize,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EpisodeWithLinks {
    pub episode: Episode,
    pub links: Vec<MemoryLink>,
}

fn default_memory_link_weight() -> usize {
    1
}

mod runtime {
    use super::{
        ChatLog, Episode, EpisodeWithLinks, GraphEdge, GraphNode, MemoryLink, MemoryStatus,
        PatientGraph, RelationshipProfile, Session, SocialGraph, SocialGraphEdge, SocialGraphNode,
        User,
    };
    use std::{
        collections::{hash_map::DefaultHasher, HashMap, HashSet},
        hash::{Hash, Hasher},
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
    use rig::streaming::StreamingPrompt;
    use rig::{
        agent::AgentBuilder,
        client::{CompletionClient, EmbeddingsClient},
        completion::{message::Text, AssistantContent, Message, Prompt},
        embeddings::EmbeddingsBuilder,
        providers::{openai, openrouter},
        Embed,
    };
    use rig::{completion::ToolDefinition, tool::Tool};
    use rig_sqlite::{Column, ColumnValue, SqliteVectorStore, SqliteVectorStoreTable};
    use rusqlite::ffi::{sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
    use rusqlite::OptionalExtension;
    use schemars::{schema_for, JsonSchema};
    use serde::{Deserialize, Serialize};
    use sqlite_vec::sqlite3_vec_init;
    use tokio::sync::{mpsc, OnceCell, RwLock};
    use tokio::time::{sleep, timeout, Duration};
    use tokio_rusqlite::Connection;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;
    use webauthn_rs::prelude::*;

    type SqliteExtensionFn =
        unsafe extern "C" fn(*mut sqlite3, *mut *mut i8, *const sqlite3_api_routines) -> i32;

    const THERAPIST_SYSTEM_PROMPT: &str = r###"
        You are IndividuateAI, a Jungian, gestalt-informed, somatic-aware therapist. Keep responses grounded and practical, usually under ~180 words; go longer only when the user brings heavy material that deserves room. If the user shares safety-critical content, encourage professional or emergency support.

        Stance:
        - Awareness before action. Change comes from fully contacting what is, not from being pushed toward what should be. Stay with the user's experience; do not rush to fix it.
        - Mirroring, naming a pattern, offering a practice, and asking a question are tools, not a template. Use the ones this moment calls for. It is fine to simply reflect and ask nothing.
        - Ask before advising: when you want to suggest an action, first check whether the user wants reflection or a suggestion, unless they explicitly asked for one.
        - One live practice at a time. Before proposing a new practice, ask about the last one. Never stack practices across consecutive turns.
        - Ambivalence is signal, not resistance. When the user hesitates about a major life choice, explore what the hesitation protects before treating it as avoidance.
        - Somatic pacing with trauma content: slow down, invite a breath or a body check-in, and titrate rather than interpret and prescribe.

        High-stakes guardrails:
        - Never draft or script messages for the user to send to another person from this therapist role, and never urge same-day or irreversible interpersonal actions (ultimatums, confrontations, cutoffs, announcements). When the user faces one, slow the pace: name the stakes, explore it across more than one exchange, suggest sleeping on it, and let any wording be the user's own.
        - When the material involves a marriage crisis, an ultimatum, potential estrangement, or decisions about having children, recommend involving a human therapist (individual or couples) alongside this work. Do this plainly, without abandoning the user.

        Perspective discipline:
        - Every person the user mentions has an inner world. Hold reported speech and complaints as that person's (or the user's) perspective, not established fact. Do not villainize absent parties; you may validate the user's feelings without prosecuting the other person.
        - Reflect the user's metaphors, but do not amplify them into escalation or build your case on them. Keep your own ground.

        You have access to persistent autobiographical memory, a mind map, a social graph, and relevant episodes that survive across sessions and conversations. Each user message is preceded by a <persistent_memory> block containing relevant prior memories, mind map nodes/edges, social graph relationships, and episodes. Treat these as your own recall, not as external data. Episodes are the ground truth of what actually happened; prefer citing them over abstract patterns when recalling events. Patterns and concepts are interpretations linked to the people and episodes they arose from. When the user asks whether you remember something, whether it was saved, or whether it is in the mind map or social graph, consult that block and answer truthfully from it. Never claim you have no memory or that nothing is saved when the <persistent_memory> block is present and non-empty. If the block is empty for a topic, say you do not have that specific detail recorded yet rather than denying memory entirely.

        Memory honesty: memory extraction runs in the background after you reply, so never claim you have already stored something mid-conversation; say it will be saved shortly. The app does have visible memory pages the user can open: the mind map at /mind-map, the social graph at /social-graph, and per-person profiles in the profile drawer. Refer the user to those instead of claiming no visible memory exists.
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
        You may receive a Known people roster in the context. Also propose concept-to-person links when the conversation supports them.
        person_slug MUST come from the Known people roster or be "self".
        concept_id MUST be an existing node id or one of the new_concepts you return.
        Use concept-to-person relations only from: originates_from, manifests_with, directed_at, triggered_by_person.
        If nothing changes, return empty arrays.
    "###;
    const EPISODE_PROMPT: &str = r###"
        Extract episodic memory from the user's message only.
        Extract only concrete events, scenes, moments, specific messages, or conversations that happened at a time/place or in a specific interaction.
        NEVER extract abstractions, patterns, personality traits, general feelings, interpretations, or recurring dynamics unless the user described a specific occurrence.
        Ground narrative in what the USER said. Do not include assistant interpretations.
        user_quotes must be short verbatim quotes from the user's message only.
        Reuse an existing episode id when new details describe the same event; that output means merge/update the saved episode.
        Use stable snake_case ids, lowercase with underscores.
        participants MUST use slugs from the Known people roster or "self"; drop anything else.
        concepts MUST use node ids from the provided mind-map node list; drop anything else.
        Return an empty episodes array if the exchange contains no concrete episode.
    "###;
    const RELATIONSHIP_PROFILE_PROMPT: &str = r###"
        Extract close-relationship memory from the text.
        Focus on people like mother, mom, dad, father, brother, partner, spouse, girlfriend, boyfriend, and close friends.
        You may receive a Known people roster in the context. MUST reuse an existing slug when the person is already in that roster, matching by name or role.
        Only create a new slug for a genuinely new person.
        Slugs must refer to actual people, never abstractions, events, choices, situations, problems, objects, or concepts.
        Prefer the person's first name as the slug when known, else use the kin role.
        Return only profiles that are explicitly mentioned or strongly implied.
        Use stable slugs like mother, dad, brother, partner, friend, or a simple snake_case name if a specific friend is repeatedly named.
        Keep fields concise and grounded in what the user actually said.
        Put only the most relevant facts in background.
        Include only actionable goals, triggers, boundaries, tone preferences, and recent events that are clearly supported by the text.
        If nothing useful is present, return an empty profiles array.
    "###;
    const SOCIAL_RELATIONSHIP_PROMPT: &str = r###"
        Extract person-to-person relationship edges from the text.
        Capture how people relate to, affect, pressure, protect, avoid, support, blame, triangulate, or conflict with each other.
        Capture third-party to third-party edges when stated, including structural kinship such as parent_of, sibling_of, partner_of, married_to, and estranged_from.
        Include the user as slug "self" when the relationship is between the user and another person.
        You may receive a Known people roster in the context. MUST reuse an existing slug when the person is already in that roster, matching by name or role.
        Only create a new slug for a genuinely new person.
        Slugs must refer to actual people, never abstractions, events, choices, situations, problems, objects, babies, deadlines, or concepts.
        Prefer the person's first name as the slug when known, else use the kin role.
        Use stable slugs like self, mother, father, parents, partner, wife, husband, brother, sister, in_laws, or a simple snake_case name.
        Keep relation as a short verb phrase, for example: pressures, avoids, protects, feels_unsupported_by, triangulates, absorbs_conflict_from.
        Include a short evidence phrase grounded in the text.
        Return only explicitly stated or strongly implied social dynamics. If nothing useful is present, return an empty relationships array.
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

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct CurrentDateTimeArgs {}

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct CurrentDateTime {
        pub timezone: String,
        pub local_datetime: String,
        pub utc_datetime: String,
        pub utc_offset: String,
        pub unix_timestamp: i64,
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

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct SocialRelationshipRecord {
        pub from_slug: String,
        pub from_label: String,
        pub to_slug: String,
        pub to_label: String,
        pub relation: String,
        pub evidence: String,
        pub weight: usize,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct ExtractedSocialRelationship {
        pub from_slug: String,
        pub from_label: String,
        pub to_slug: String,
        pub to_label: String,
        pub relation: String,
        pub evidence: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct SocialRelationshipDelta {
        #[serde(default)]
        pub relationships: Vec<ExtractedSocialRelationship>,
    }

    impl SocialRelationshipDelta {
        fn is_empty(&self) -> bool {
            self.relationships.is_empty()
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct SessionSummaryData {
        pub title: String,
        pub preview: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct ExtractedPersonLink {
        pub concept_id: String,
        pub person_slug: String,
        pub relation: String,
        pub evidence: String,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct ConversationGraphDelta {
        #[serde(default)]
        pub new_concepts: Vec<GraphNode>,
        #[serde(default)]
        pub new_connections: Vec<GraphEdge>,
        #[serde(default)]
        pub obsolete_concept_ids: Vec<String>,
        #[serde(default)]
        pub obsolete_connections: Vec<GraphEdge>,
        #[serde(default)]
        pub person_links: Vec<ExtractedPersonLink>,
    }

    impl ConversationGraphDelta {
        fn is_empty(&self) -> bool {
            self.new_concepts.is_empty()
                && self.new_connections.is_empty()
                && self.obsolete_concept_ids.is_empty()
                && self.obsolete_connections.is_empty()
                && self.person_links.is_empty()
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct ExtractedEpisode {
        pub id: String,
        pub title: String,
        pub narrative: String,
        pub occurred_at: Option<String>,
        #[serde(default)]
        pub participants: Vec<String>,
        #[serde(default)]
        pub concepts: Vec<String>,
        #[serde(default)]
        pub user_quotes: Vec<String>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
    struct EpisodeDelta {
        #[serde(default)]
        pub episodes: Vec<ExtractedEpisode>,
    }

    impl EpisodeDelta {
        fn is_empty(&self) -> bool {
            self.episodes.is_empty()
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

    #[derive(Clone)]
    struct CurrentDateTimeTool;

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

    fn memory_db_key() -> Option<String> {
        std::env::var("MEMORY_DB_KEY")
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
    }

    async fn apply_memory_db_key(conn: &Connection, key: &str) -> Result<()> {
        let key = key.to_string();
        conn.call(move |conn| {
            conn.pragma_update(None, "key", &key)
                .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await
        .context("Applying SQLCipher key to memory store")
    }

    async fn db_is_readable(conn: &Connection) -> bool {
        conn.call(|conn| {
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await
        .is_ok()
    }

    /// One-time migration: if the database on disk is plaintext SQLite, rewrite
    /// it encrypted with `key` via sqlcipher_export, keeping a `.plaintext.bak`
    /// copy. No-op for new or already-encrypted databases.
    fn encrypt_plaintext_db(db_path: &str, key: &str) -> Result<()> {
        let file_len = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
        if file_len == 0 {
            return Ok(());
        }
        let readable_plaintext = rusqlite::Connection::open(db_path)
            .and_then(|conn| {
                conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .is_ok();
        if !readable_plaintext {
            return Ok(());
        }

        tracing::info!("Encrypting plaintext memory store at {db_path}");
        let tmp_path = format!("{db_path}.encrypting");
        let _ = std::fs::remove_file(&tmp_path);
        let conn = rusqlite::Connection::open(db_path)
            .context("Opening plaintext memory store for encryption")?;
        conn.execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            rusqlite::params![tmp_path, key],
        )
        .context("Attaching encrypted database")?;
        conn.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))
            .context("Exporting memory store into encrypted database")?;
        conn.execute("DETACH DATABASE encrypted", [])
            .context("Detaching encrypted database")?;
        conn.close()
            .map_err(|(_, e)| e)
            .context("Closing plaintext memory store")?;

        let backup_path = format!("{db_path}.plaintext.bak");
        std::fs::rename(db_path, &backup_path).context("Backing up plaintext memory store")?;
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{db_path}{suffix}"));
        }
        std::fs::rename(&tmp_path, db_path).context("Moving encrypted database into place")?;
        tracing::warn!(
            "Plaintext backup kept at {backup_path}; delete it once the encrypted store is verified"
        );
        Ok(())
    }

    async fn open_memory_db(db_path: &str) -> Result<Connection> {
        let Some(key) = memory_db_key() else {
            tracing::warn!(
                "MEMORY_DB_KEY is not set; memory store at {db_path} is stored UNENCRYPTED"
            );
            return Connection::open(db_path)
                .await
                .context("Opening sqlite memory store");
        };

        let path = db_path.to_string();
        let migrate_key = key.clone();
        tokio::task::spawn_blocking(move || encrypt_plaintext_db(&path, &migrate_key))
            .await
            .context("Joining memory store encryption task")??;

        let conn = Connection::open(db_path)
            .await
            .context("Opening sqlite memory store")?;
        apply_memory_db_key(&conn, &key).await?;
        if !db_is_readable(&conn).await {
            anyhow::bail!(
                "Cannot read memory store at {db_path}: MEMORY_DB_KEY does not match the key it was encrypted with (or the file is corrupt)"
            );
        }
        Ok(conn)
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
                CREATE TABLE IF NOT EXISTS social_graphs (
                    user_id TEXT PRIMARY KEY,
                    graph_json TEXT NOT NULL,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS social_relationships (
                    user_id TEXT NOT NULL,
                    from_slug TEXT NOT NULL,
                    to_slug TEXT NOT NULL,
                    relation TEXT NOT NULL,
                    from_label TEXT NOT NULL,
                    to_label TEXT NOT NULL,
                    evidence TEXT NOT NULL,
                    weight INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (user_id, from_slug, to_slug, relation),
                    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
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
                CREATE TABLE IF NOT EXISTS episodes (
                    user_id TEXT NOT NULL,
                    id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    narrative TEXT NOT NULL,
                    occurred_at TEXT,
                    session_id TEXT,
                    user_quotes TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (user_id, id)
                );
                CREATE TABLE IF NOT EXISTS memory_links (
                    user_id TEXT NOT NULL,
                    from_kind TEXT NOT NULL,
                    from_id TEXT NOT NULL,
                    relation TEXT NOT NULL,
                    to_kind TEXT NOT NULL,
                    to_id TEXT NOT NULL,
                    evidence TEXT NOT NULL DEFAULT '',
                    weight INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (user_id, from_kind, from_id, relation, to_kind, to_id)
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
                CREATE TABLE IF NOT EXISTS password_reset_tokens (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL,
                    token TEXT NOT NULL UNIQUE,
                    expires_at TEXT NOT NULL,
                    used INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_reset_tokens_token ON password_reset_tokens(token);
                "###,
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)?;

            if table_exists(conn, "users").map_err(tokio_rusqlite::Error::Rusqlite)?
                && !table_has_column(conn, "users", "email_verified")
                    .map_err(tokio_rusqlite::Error::Rusqlite)?
            {
                conn.execute(
                    "ALTER TABLE users ADD COLUMN email_verified INTEGER NOT NULL DEFAULT 0",
                    [],
                )
                .map_err(tokio_rusqlite::Error::Rusqlite)?;
            }

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

    async fn ensure_local_test_user(conn: &Connection) -> Result<()> {
        let email = std::env::var("LOCAL_TEST_EMAIL").unwrap_or_default();
        let password = std::env::var("LOCAL_TEST_PASSWORD").unwrap_or_default();
        if email.is_empty() || password.is_empty() {
            return Ok(());
        }

        let id = Uuid::new_v4().to_string();
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Hashing failed: {}", e))?
            .to_string();

        conn.call(move |conn| {
            conn.execute(
                r###"
                INSERT INTO users (id, username, password_hash)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(username) DO UPDATE SET password_hash = excluded.password_hash
                "###,
                rusqlite::params![id, email, password_hash],
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await?;

        Ok(())
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

    async fn read_social_graph(conn: &Connection, user_id: &str) -> Result<SocialGraph> {
        let user_id_owned = user_id.to_string();
        let stored: Option<String> = conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT graph_json FROM social_graphs WHERE user_id = ?1",
                    [user_id_owned],
                    |row| row.get(0),
                )
                .optional()
                .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .context("Fetching social graph")?;

        if let Some(raw) = stored {
            let graph =
                serde_json::from_str::<SocialGraph>(&raw).context("Parsing social graph JSON")?;
            return Ok(graph);
        }

        Ok(SocialGraph {
            user_id: user_id.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        })
    }

    async fn read_patient_graph_snapshot(conn: &Connection, user_id: &str) -> Result<PatientGraph> {
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
            .context("Fetching patient graph snapshot")?;

        if let Some(raw) = stored {
            let graph =
                serde_json::from_str::<PatientGraph>(&raw).context("Parsing patient graph JSON")?;
            return Ok(graph);
        }

        Ok(PatientGraph {
            user_id: user_id.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        })
    }

    async fn upsert_episode_record(conn: &Connection, mut episode: Episode) -> Result<()> {
        episode.id = normalize_slug(&episode.id);
        if episode.user_id.trim().is_empty()
            || episode.id.is_empty()
            || episode.title.trim().is_empty()
            || episode.narrative.trim().is_empty()
        {
            return Ok(());
        }

        episode.title = episode.title.trim().chars().take(160).collect::<String>();
        episode.narrative = episode
            .narrative
            .trim()
            .chars()
            .take(2400)
            .collect::<String>();
        episode.user_quotes = merge_unique_strings(&[], &episode.user_quotes, 12);
        let existing_quotes = existing_episode_quotes(conn, &episode.user_id, &episode.id).await?;
        let user_quotes = serde_json::to_string(&merge_unique_strings(
            &existing_quotes,
            &episode.user_quotes,
            16,
        ))
        .context("Serializing episode quotes")?;

        conn.call(move |conn| {
            conn.execute(
                r###"
                INSERT INTO episodes
                    (user_id, id, title, narrative, occurred_at, session_id, user_quotes)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(user_id, id)
                DO UPDATE SET
                    title = excluded.title,
                    narrative = excluded.narrative,
                    occurred_at = COALESCE(excluded.occurred_at, episodes.occurred_at),
                    session_id = COALESCE(excluded.session_id, episodes.session_id),
                    user_quotes = excluded.user_quotes,
                    updated_at = strftime('%Y-%m-%d %H:%M:%f', 'now')
                "###,
                rusqlite::params![
                    episode.user_id,
                    episode.id,
                    episode.title,
                    episode.narrative,
                    episode.occurred_at,
                    episode.session_id,
                    user_quotes
                ],
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await
        .context("Persisting episode")?;

        Ok(())
    }

    async fn existing_episode_quotes(
        conn: &Connection,
        user_id: &str,
        id: &str,
    ) -> Result<Vec<String>> {
        let user_id = user_id.to_string();
        let id = id.to_string();
        let raw: Option<String> = conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT user_quotes FROM episodes WHERE user_id = ?1 AND id = ?2",
                    rusqlite::params![user_id, id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .context("Fetching episode quotes")?;
        Ok(raw
            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
            .unwrap_or_default())
    }

    async fn list_episode_records(conn: &Connection, user_id: String) -> Result<Vec<Episode>> {
        conn.call(move |conn| {
            let mut stmt = conn.prepare(
                r###"
                SELECT user_id, id, title, narrative, occurred_at, session_id, user_quotes, created_at, updated_at
                FROM episodes
                WHERE user_id = ?1
                ORDER BY updated_at DESC
                "###,
            )?;
            let rows = stmt.query_map([user_id], |row| {
                let raw_quotes: String = row.get(6)?;
                Ok(Episode {
                    user_id: row.get(0)?,
                    id: row.get(1)?,
                    title: row.get(2)?,
                    narrative: row.get(3)?,
                    occurred_at: row.get(4)?,
                    session_id: row.get(5)?,
                    user_quotes: serde_json::from_str(&raw_quotes).unwrap_or_default(),
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?;
            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
        .await
        .context("Listing episodes")
    }

    async fn upsert_memory_link_record(conn: &Connection, mut link: MemoryLink) -> Result<()> {
        link.from_kind = normalize_slug(&link.from_kind);
        link.from_id = normalize_slug(&link.from_id);
        link.relation = normalize_slug(&link.relation);
        link.to_kind = normalize_slug(&link.to_kind);
        link.to_id = normalize_slug(&link.to_id);
        if link.user_id.trim().is_empty()
            || link.from_kind.is_empty()
            || link.from_id.is_empty()
            || link.relation.is_empty()
            || link.to_kind.is_empty()
            || link.to_id.is_empty()
        {
            return Ok(());
        }
        let evidence = link.evidence.trim().chars().take(240).collect::<String>();
        let weight = link.weight.max(1) as i64;

        conn.call(move |conn| {
            conn.execute(
                r###"
                INSERT INTO memory_links
                    (user_id, from_kind, from_id, relation, to_kind, to_id, evidence, weight)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(user_id, from_kind, from_id, relation, to_kind, to_id)
                DO UPDATE SET
                    evidence = excluded.evidence,
                    weight = memory_links.weight + excluded.weight,
                    updated_at = strftime('%Y-%m-%d %H:%M:%f', 'now')
                "###,
                rusqlite::params![
                    link.user_id,
                    link.from_kind,
                    link.from_id,
                    link.relation,
                    link.to_kind,
                    link.to_id,
                    evidence,
                    weight
                ],
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await
        .context("Persisting memory link")?;

        Ok(())
    }

    async fn list_memory_link_records(
        conn: &Connection,
        user_id: String,
    ) -> Result<Vec<MemoryLink>> {
        conn.call(move |conn| {
            let mut stmt = conn.prepare(
                r###"
                SELECT user_id, from_kind, from_id, relation, to_kind, to_id, evidence, weight, created_at, updated_at
                FROM memory_links
                WHERE user_id = ?1
                ORDER BY updated_at DESC
                "###,
            )?;
            let rows = stmt.query_map([user_id], |row| {
                Ok(MemoryLink {
                    user_id: row.get(0)?,
                    from_kind: row.get(1)?,
                    from_id: row.get(2)?,
                    relation: row.get(3)?,
                    to_kind: row.get(4)?,
                    to_id: row.get(5)?,
                    evidence: row.get(6)?,
                    weight: row.get::<_, i64>(7)?.max(1) as usize,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?;
            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        })
        .await
        .context("Listing memory links")
    }

    fn private_signature<T: Serialize>(value: &T) -> String {
        let payload = serde_json::to_string(value).unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        payload.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    fn memory_headline_from_text(text: &str, fallback: &str) -> String {
        let cleaned: String = text
            .chars()
            .map(|ch| {
                if ch.is_alphanumeric() || ch.is_whitespace() || ch == '-' {
                    ch
                } else {
                    ' '
                }
            })
            .collect();
        let words: Vec<&str> = cleaned
            .split_whitespace()
            .filter(|word| word.len() > 1)
            .take(5)
            .collect();

        if words.len() >= 2 {
            words.join(" ")
        } else {
            fallback.to_string()
        }
    }

    fn visible_stream_chunks(text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            current.push(ch);
            let should_flush = current.len() >= 18 || (ch.is_whitespace() && current.len() >= 6);
            if should_flush {
                chunks.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }

    async fn send_visible_stream(
        tx: &mpsc::Sender<Result<String, std::convert::Infallible>>,
        text: &str,
    ) {
        for chunk in visible_stream_chunks(text) {
            if tx.send(Ok(chunk)).await.is_err() {
                break;
            }
            sleep(Duration::from_millis(28)).await;
        }
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

    fn social_graph_concept_id(kind: &str, label: &str) -> String {
        format!("{}:{}", kind, normalize_slug(label))
    }

    fn social_person_id(slug: &str) -> String {
        let normalized = normalize_slug(slug);
        if normalized == "self" || normalized == "user" || normalized == "me" {
            "self".to_string()
        } else {
            format!("person:{}", normalized)
        }
    }

    fn social_actor_label(slug: &str, label: &str) -> String {
        if label.trim().is_empty() {
            normalize_slug(slug).replace('_', " ")
        } else {
            label.trim().to_string()
        }
    }

    fn social_actor_is_self(slug: &str, label: &str) -> bool {
        matches!(
            normalize_slug(slug).as_str(),
            "self" | "user" | "me" | "myself"
        ) || matches!(
            normalize_slug(label).as_str(),
            "self" | "user" | "me" | "myself" | "you"
        )
    }

    fn kin_synonym_key(value: &str) -> Option<&'static str> {
        match normalize_slug(value).as_str() {
            "father" | "dad" | "daddy" | "papa" | "pops" => Some("dad"),
            "mother" | "mom" | "mum" | "mama" => Some("mother"),
            "partner" | "spouse" | "wife" | "husband" | "girlfriend" | "boyfriend" => {
                Some("partner")
            }
            "brother" | "bro" => Some("brother"),
            "sister" | "sis" => Some("sister"),
            "parents" | "parent" => Some("parents"),
            "mother_in_law" | "mum_in_law" | "mom_in_law" => Some("mother_in_law"),
            "father_in_law" | "dad_in_law" => Some("father_in_law"),
            "in_laws" | "inlaws" => Some("in_laws"),
            _ => None,
        }
    }

    fn relationship_specificity(detail: &str) -> usize {
        let normalized = normalize_slug(detail);
        if normalized.is_empty() || normalized == "extracted_social_actor" {
            return 0;
        }
        if normalized.contains("mother_in_law") || normalized.contains("father_in_law") {
            90
        } else if normalized.contains("partner") || normalized.contains("spouse") {
            80
        } else if normalized.contains("wife") || normalized.contains("husband") {
            78
        } else if normalized.contains("brother")
            || normalized.contains("sister")
            || normalized.contains("sibling")
        {
            75
        } else if normalized.contains("mother")
            || normalized.contains("father")
            || normalized.contains("dad")
            || normalized.contains("parent")
        {
            70
        } else if normalized.contains("friend") {
            40
        } else {
            55
        }
    }

    fn abstract_social_actor_term(value: &str) -> bool {
        matches!(
            normalize_slug(value).as_str(),
            "" | "choice"
                | "binary_choice"
                | "decision"
                | "situation"
                | "problem"
                | "issue"
                | "boundary"
                | "boundaries"
                | "conflict"
                | "argument"
                | "event"
                | "pattern"
                | "need"
                | "goal"
                | "trigger"
                | "emotion"
                | "feeling"
                | "feelings"
                | "baby"
                | "deadline"
                | "work"
                | "job"
                | "therapy"
                | "conversation"
                | "text"
                | "message"
                | "plan"
                | "future"
                | "past"
                | "family"
                | "support"
                | "pressure"
                | "stress"
                | "anxiety"
                | "relationship"
                | "marriage"
                | "village"
                | "community"
        )
    }

    fn valid_social_person_actor(slug: &str, label: &str) -> bool {
        if social_actor_is_self(slug, label) {
            return true;
        }
        let slug = normalize_slug(slug);
        let label = normalize_slug(label);
        if abstract_social_actor_term(&slug) || abstract_social_actor_term(&label) {
            return false;
        }
        if slug.starts_with("concept_")
            || slug.starts_with("pattern_")
            || slug.starts_with("goal_")
            || slug.starts_with("trigger_")
        {
            return false;
        }
        true
    }

    #[derive(Clone, Debug)]
    struct SocialPersonCandidate {
        source_slug: String,
        aliases: HashSet<String>,
        label: String,
        detail: String,
        weight: usize,
        profile_backed: bool,
        order: usize,
    }

    impl SocialPersonCandidate {
        fn node_id(&self) -> String {
            social_person_id(&self.source_slug)
        }
    }

    fn social_person_preferred(candidate: &SocialPersonCandidate) -> (usize, usize, usize) {
        (
            usize::from(candidate.profile_backed),
            relationship_specificity(&candidate.detail),
            candidate.weight,
        )
    }

    fn merge_social_person_candidate(
        target: &mut SocialPersonCandidate,
        incoming: &SocialPersonCandidate,
    ) {
        let target_preference = social_person_preferred(target);
        let incoming_preference = social_person_preferred(incoming);
        target.weight += incoming.weight.max(1);
        target.aliases.extend(incoming.aliases.iter().cloned());
        target.aliases.insert(normalize_slug(&incoming.source_slug));
        if incoming_preference > target_preference {
            target.source_slug = incoming.source_slug.clone();
            if !incoming.label.trim().is_empty() {
                target.label = incoming.label.clone();
            }
            if !incoming.detail.trim().is_empty() {
                target.detail = incoming.detail.clone();
            }
            target.profile_backed = incoming.profile_backed;
            target.order = target.order.min(incoming.order);
        } else {
            if target.label.trim().is_empty() && !incoming.label.trim().is_empty() {
                target.label = incoming.label.clone();
            }
            if relationship_specificity(&target.detail) == 0
                && relationship_specificity(&incoming.detail) > 0
            {
                target.detail = incoming.detail.clone();
            }
        }
    }

    fn candidate_display_key(candidate: &SocialPersonCandidate) -> Option<String> {
        let key = normalize_slug(&candidate.label);
        if key.is_empty() || abstract_social_actor_term(&key) || key == "you" {
            None
        } else {
            Some(key)
        }
    }

    fn canonical_social_people(
        profiles: &[RelationshipProfile],
        social_relationships: &[SocialRelationshipRecord],
    ) -> (HashMap<String, SocialGraphNode>, HashMap<String, String>) {
        let mut candidates = Vec::new();
        for (index, profile) in profiles.iter().enumerate() {
            let slug = normalize_slug(&profile.slug);
            if slug.is_empty() || social_actor_is_self(&slug, &profile.display_name) {
                continue;
            }
            let label = if profile.display_name.trim().is_empty() {
                slug.replace('_', " ")
            } else {
                profile.display_name.trim().to_string()
            };
            candidates.push(SocialPersonCandidate {
                source_slug: slug,
                aliases: HashSet::from([normalize_slug(&profile.slug)]),
                label,
                detail: profile.relationship_type.trim().to_string(),
                weight: 4 + profile.recent_events.len() + profile.triggers.len(),
                profile_backed: true,
                order: index,
            });
        }

        for (index, relationship) in social_relationships.iter().enumerate() {
            for (slug, label) in [
                (&relationship.from_slug, &relationship.from_label),
                (&relationship.to_slug, &relationship.to_label),
            ] {
                if !valid_social_person_actor(slug, label) || social_actor_is_self(slug, label) {
                    continue;
                }
                let normalized_slug = normalize_slug(slug);
                if normalized_slug.is_empty() {
                    continue;
                }
                candidates.push(SocialPersonCandidate {
                    source_slug: normalized_slug,
                    aliases: HashSet::from([normalize_slug(slug)]),
                    label: social_actor_label(slug, label),
                    detail: "Extracted social actor".to_string(),
                    weight: relationship.weight.max(1),
                    profile_backed: false,
                    order: profiles.len() + index,
                });
            }
        }

        let mut groups: Vec<SocialPersonCandidate> = Vec::new();
        let mut exact_index = HashMap::<String, usize>::new();
        for candidate in candidates {
            let key = normalize_slug(&candidate.source_slug);
            if let Some(index) = exact_index.get(&key).copied() {
                merge_social_person_candidate(&mut groups[index], &candidate);
            } else {
                exact_index.insert(key, groups.len());
                groups.push(candidate);
            }
        }

        let mut kin_index = HashMap::<String, usize>::new();
        let mut merged_by_kin: Vec<SocialPersonCandidate> = Vec::new();
        for mut candidate in groups {
            let canonical_kin_key = kin_synonym_key(&candidate.source_slug).map(str::to_string);
            if let Some(key) = &canonical_kin_key {
                candidate
                    .aliases
                    .insert(normalize_slug(&candidate.source_slug));
                candidate.source_slug = key.clone();
            }
            let key = canonical_kin_key
                .clone()
                .unwrap_or_else(|| format!("slug:{}", normalize_slug(&candidate.source_slug)));
            if let Some(index) = kin_index.get(&key).copied() {
                merge_social_person_candidate(&mut merged_by_kin[index], &candidate);
            } else {
                kin_index.insert(key, merged_by_kin.len());
                merged_by_kin.push(candidate);
            }
        }

        let mut display_index = HashMap::<String, usize>::new();
        let mut merged_by_display: Vec<SocialPersonCandidate> = Vec::new();
        for candidate in merged_by_kin {
            if let Some(key) = candidate_display_key(&candidate) {
                if let Some(index) = display_index.get(&key).copied() {
                    merge_social_person_candidate(&mut merged_by_display[index], &candidate);
                    continue;
                }
                display_index.insert(key, merged_by_display.len());
            }
            merged_by_display.push(candidate);
        }

        let mut nodes = HashMap::new();
        let mut resolution = HashMap::new();
        for candidate in merged_by_display {
            let node_id = candidate.node_id();
            let label = if candidate.label.trim().is_empty() {
                candidate.source_slug.replace('_', " ")
            } else {
                candidate.label.clone()
            };
            nodes.insert(
                node_id.clone(),
                SocialGraphNode {
                    id: node_id.clone(),
                    label: label.clone(),
                    kind: "person".to_string(),
                    detail: candidate.detail,
                    weight: candidate.weight.max(1),
                },
            );
            resolution.insert(normalize_slug(&candidate.source_slug), node_id.clone());
            for alias in candidate.aliases {
                if !alias.is_empty() {
                    resolution.insert(alias, node_id.clone());
                }
            }
            if let Some(key) = kin_synonym_key(&candidate.source_slug) {
                resolution.insert(key.to_string(), node_id.clone());
            }
            let label_key = normalize_slug(&label);
            if !label_key.is_empty() && !abstract_social_actor_term(&label_key) {
                resolution.insert(label_key, node_id.clone());
            }
        }

        for profile in profiles {
            let slug = normalize_slug(&profile.slug);
            if slug.is_empty() {
                continue;
            }
            if let Some(node_id) = nodes
                .values()
                .find(|node| {
                    node.label.eq_ignore_ascii_case(profile.display_name.trim())
                        && !profile.display_name.trim().is_empty()
                })
                .map(|node| node.id.clone())
            {
                resolution.insert(slug, node_id);
            }
        }

        (nodes, resolution)
    }

    fn resolve_social_actor(
        slug: &str,
        label: &str,
        resolution: &HashMap<String, String>,
    ) -> Option<String> {
        if social_actor_is_self(slug, label) {
            return Some("self".to_string());
        }
        if !valid_social_person_actor(slug, label) {
            return None;
        }
        let normalized_slug = normalize_slug(slug);
        if let Some(id) = resolution.get(&normalized_slug) {
            return Some(id.clone());
        }
        if let Some(key) = kin_synonym_key(&normalized_slug) {
            if let Some(id) = resolution.get(key) {
                return Some(id.clone());
            }
        }
        let label_key = normalize_slug(label);
        resolution.get(&label_key).cloned()
    }

    fn known_people_roster(profiles: &[RelationshipProfile]) -> String {
        if profiles.is_empty() {
            return "Known people:\n- none yet".to_string();
        }
        let mut lines = vec!["Known people:".to_string()];
        for profile in profiles {
            let slug = normalize_slug(&profile.slug);
            if slug.is_empty() {
                continue;
            }
            let display_name = if profile.display_name.trim().is_empty() {
                slug.replace('_', " ")
            } else {
                profile.display_name.trim().to_string()
            };
            let relationship_type = if profile.relationship_type.trim().is_empty() {
                "relationship unspecified".to_string()
            } else {
                profile.relationship_type.trim().to_string()
            };
            lines.push(format!(
                "- {} — {} ({})",
                slug, display_name, relationship_type
            ));
        }
        lines.join("\n")
    }

    fn known_person_slugs(profiles: &[RelationshipProfile]) -> HashSet<String> {
        let mut slugs = HashSet::from(["self".to_string()]);
        for profile in profiles {
            let slug = normalize_slug(&profile.slug);
            if !slug.is_empty() {
                slugs.insert(slug);
            }
        }
        slugs
    }

    fn graph_node_context(graph: &PatientGraph) -> String {
        if graph.nodes.is_empty() {
            return "Mind-map nodes:\n- none yet".to_string();
        }
        let mut lines = vec!["Mind-map nodes (id: label [category]):".to_string()];
        for node in graph.nodes.iter().take(80) {
            lines.push(format!("- {}: {} [{}]", node.id, node.label, node.category));
        }
        lines.join("\n")
    }

    fn episode_roster(episodes: &[Episode]) -> String {
        if episodes.is_empty() {
            return "Existing episodes:\n- none yet".to_string();
        }
        let mut lines = vec!["Existing episodes (id: title):".to_string()];
        for episode in episodes.iter().take(80) {
            lines.push(format!("- {}: {}", episode.id, episode.title));
        }
        lines.join("\n")
    }

    fn episode_and_links_from_extracted(
        user_id: &str,
        session_id: Option<&str>,
        extracted: ExtractedEpisode,
        valid_people: &HashSet<String>,
        valid_concepts: &HashSet<String>,
    ) -> Option<(Episode, Vec<MemoryLink>)> {
        let id = normalize_slug(&extracted.id);
        if id.is_empty()
            || extracted.title.trim().is_empty()
            || extracted.narrative.trim().is_empty()
        {
            return None;
        }

        let episode = Episode {
            user_id: user_id.to_string(),
            id: id.clone(),
            title: extracted.title.trim().to_string(),
            narrative: extracted.narrative.trim().to_string(),
            occurred_at: extracted
                .occurred_at
                .and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string())),
            session_id: session_id.map(ToOwned::to_owned),
            user_quotes: merge_unique_strings(&[], &extracted.user_quotes, 12),
            created_at: None,
            updated_at: None,
        };

        let mut links = Vec::new();
        let mut seen = HashSet::new();
        for participant in extracted.participants {
            let slug = normalize_slug(&participant);
            if slug.is_empty() || !valid_people.contains(&slug) {
                continue;
            }
            let key = ("person".to_string(), slug.clone(), "involves".to_string());
            if seen.insert(key) {
                links.push(MemoryLink {
                    user_id: user_id.to_string(),
                    from_kind: "episode".to_string(),
                    from_id: id.clone(),
                    relation: "involves".to_string(),
                    to_kind: "person".to_string(),
                    to_id: slug,
                    evidence: episode.title.clone(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                });
            }
        }
        for concept in extracted.concepts {
            let concept_id = normalize_slug(&concept);
            if concept_id.is_empty() || !valid_concepts.contains(&concept_id) {
                continue;
            }
            let key = (
                "concept".to_string(),
                concept_id.clone(),
                "evidences".to_string(),
            );
            if seen.insert(key) {
                links.push(MemoryLink {
                    user_id: user_id.to_string(),
                    from_kind: "episode".to_string(),
                    from_id: id.clone(),
                    relation: "evidences".to_string(),
                    to_kind: "concept".to_string(),
                    to_id: concept_id,
                    evidence: episode.title.clone(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                });
            }
        }

        Some((episode, links))
    }

    fn valid_graph_person_link_relations() -> HashSet<&'static str> {
        HashSet::from([
            "originates_from",
            "manifests_with",
            "directed_at",
            "triggered_by_person",
        ])
    }

    fn memory_links_from_person_links(
        user_id: &str,
        links: Vec<ExtractedPersonLink>,
        valid_people: &HashSet<String>,
        valid_concepts: &HashSet<String>,
    ) -> Vec<MemoryLink> {
        let valid_relations = valid_graph_person_link_relations();
        let mut seen = HashSet::new();
        let mut memory_links = Vec::new();
        for link in links {
            let concept_id = normalize_slug(&link.concept_id);
            let person_slug = normalize_slug(&link.person_slug);
            let relation = normalize_slug(&link.relation);
            if !valid_concepts.contains(&concept_id)
                || !valid_people.contains(&person_slug)
                || !valid_relations.contains(relation.as_str())
            {
                continue;
            }
            let key = (concept_id.clone(), relation.clone(), person_slug.clone());
            if !seen.insert(key) {
                continue;
            }
            memory_links.push(MemoryLink {
                user_id: user_id.to_string(),
                from_kind: "concept".to_string(),
                from_id: concept_id,
                relation,
                to_kind: "person".to_string(),
                to_id: person_slug,
                evidence: link.evidence.trim().to_string(),
                weight: 1,
                created_at: None,
                updated_at: None,
            });
        }
        memory_links
    }

    fn relevant_mind_map_social_nodes<'a>(
        patient_graph: &'a PatientGraph,
        profiles: &[RelationshipProfile],
    ) -> Vec<&'a GraphNode> {
        let mut terms = HashSet::new();
        for profile in profiles {
            for item in profile.goals.iter().chain(profile.triggers.iter()) {
                let normalized = normalize_slug(item);
                if !normalized.is_empty() {
                    terms.insert(normalized);
                }
            }
        }

        let candidates = patient_graph
            .nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.category.as_str(),
                    "Pattern" | "Need" | "Goal" | "Trigger"
                )
            })
            .collect::<Vec<_>>();

        let mut selected = Vec::new();
        for node in &candidates {
            let node_key = normalize_slug(&node.label);
            if terms
                .iter()
                .any(|term| term.contains(&node_key) || node_key.contains(term))
            {
                selected.push(*node);
                if selected.len() >= 10 {
                    return selected;
                }
            }
        }
        for node in candidates {
            if selected
                .iter()
                .any(|selected_node| selected_node.id == node.id)
            {
                continue;
            }
            selected.push(node);
            if selected.len() >= 10 {
                break;
            }
        }
        selected
    }

    fn social_graph_concept_node_id(node: &GraphNode) -> String {
        social_graph_concept_id(&node.category.to_lowercase(), &node.label)
    }

    fn memory_link_connects(link: &MemoryLink, left_kind: &str, right_kind: &str) -> bool {
        (link.from_kind == left_kind && link.to_kind == right_kind)
            || (link.from_kind == right_kind && link.to_kind == left_kind)
    }

    fn memory_link_id_for_kind<'a>(link: &'a MemoryLink, kind: &str) -> Option<&'a str> {
        if link.from_kind == kind {
            Some(link.from_id.as_str())
        } else if link.to_kind == kind {
            Some(link.to_id.as_str())
        } else {
            None
        }
    }

    fn memory_link_relation(link: &MemoryLink, fallback: &str) -> String {
        let relation = link.relation.trim();
        if relation.is_empty() {
            fallback.to_string()
        } else {
            relation.to_string()
        }
    }

    fn social_graph_add_concept(
        nodes: &mut HashMap<String, SocialGraphNode>,
        edges: &mut HashMap<(String, String, String), usize>,
        from: &str,
        kind: &str,
        label: &str,
        relation: &str,
    ) {
        let label = label.trim();
        if label.is_empty() {
            return;
        }
        let id = social_graph_concept_id(kind, label);
        nodes
            .entry(id.clone())
            .and_modify(|node| node.weight += 1)
            .or_insert_with(|| SocialGraphNode {
                id: id.clone(),
                label: label.to_string(),
                kind: kind.to_string(),
                detail: String::new(),
                weight: 1,
            });
        let key = (from.to_string(), id, relation.to_string());
        *edges.entry(key).or_insert(0) += 1;
    }

    fn build_social_graph(
        user_id: String,
        profiles: &[RelationshipProfile],
        patient_graph: &PatientGraph,
        social_relationships: &[SocialRelationshipRecord],
        memory_links: &[MemoryLink],
        episodes: &[Episode],
    ) -> SocialGraph {
        let mut nodes = HashMap::new();
        let mut edges: HashMap<(String, String, String), usize> = HashMap::new();

        nodes.insert(
            "self".to_string(),
            SocialGraphNode {
                id: "self".to_string(),
                label: "You".to_string(),
                kind: "self".to_string(),
                detail: "The current account holder".to_string(),
                weight: profiles.len().max(1),
            },
        );

        let (canonical_people, person_resolution) =
            canonical_social_people(profiles, social_relationships);

        for node in canonical_people.into_values() {
            nodes.insert(node.id.clone(), node);
        }

        for profile in profiles {
            let Some(person_id) =
                resolve_social_actor(&profile.slug, &profile.display_name, &person_resolution)
            else {
                continue;
            };
            edges.insert(
                (
                    "self".to_string(),
                    person_id.clone(),
                    profile.relationship_type.clone(),
                ),
                3,
            );

            for item in &profile.recent_events {
                social_graph_add_concept(
                    &mut nodes,
                    &mut edges,
                    &person_id,
                    "event",
                    item,
                    "experienced",
                );
            }
            for item in &profile.triggers {
                social_graph_add_concept(
                    &mut nodes,
                    &mut edges,
                    &person_id,
                    "trigger",
                    item,
                    "triggered by",
                );
            }
            for item in &profile.goals {
                social_graph_add_concept(&mut nodes, &mut edges, &person_id, "goal", item, "needs");
            }
            for item in &profile.boundaries {
                social_graph_add_concept(
                    &mut nodes, &mut edges, &person_id, "boundary", item, "boundary",
                );
            }
        }

        for relationship in social_relationships {
            let Some(from_id) = resolve_social_actor(
                &relationship.from_slug,
                &relationship.from_label,
                &person_resolution,
            ) else {
                continue;
            };
            let Some(to_id) = resolve_social_actor(
                &relationship.to_slug,
                &relationship.to_label,
                &person_resolution,
            ) else {
                continue;
            };
            if from_id == to_id {
                continue;
            }

            let relation = if relationship.relation.trim().is_empty() {
                "relates_to".to_string()
            } else {
                relationship.relation.trim().to_string()
            };
            let key = (from_id, to_id, relation);
            *edges.entry(key).or_insert(0) += relationship.weight.max(1);
        }

        let graph_node_by_id = patient_graph
            .nodes
            .iter()
            .map(|node| (normalize_slug(&node.id), node))
            .collect::<HashMap<_, _>>();
        let mut selected_concepts = Vec::<&GraphNode>::new();
        let mut linked_concepts = HashSet::<String>::new();

        for link in memory_links
            .iter()
            .filter(|link| link.from_kind == "concept" && link.to_kind == "person")
        {
            let Some(node) = graph_node_by_id
                .get(&normalize_slug(&link.from_id))
                .copied()
            else {
                continue;
            };
            let Some(person_id) =
                resolve_social_actor(&link.to_id, &link.to_id, &person_resolution)
            else {
                continue;
            };
            if !nodes.contains_key(&person_id) {
                continue;
            }
            if linked_concepts.insert(node.id.clone()) {
                selected_concepts.push(node);
            }
            if selected_concepts.len() >= 10 {
                break;
            }
        }

        for node in relevant_mind_map_social_nodes(patient_graph, profiles) {
            if selected_concepts
                .iter()
                .any(|selected| selected.id == node.id)
            {
                continue;
            }
            selected_concepts.push(node);
            if selected_concepts.len() >= 10 {
                break;
            }
        }

        for node in selected_concepts {
            let concept_id = social_graph_concept_node_id(node);
            nodes
                .entry(concept_id.clone())
                .or_insert_with(|| SocialGraphNode {
                    id: concept_id.clone(),
                    label: node.label.clone(),
                    kind: node.category.to_lowercase(),
                    detail: "Mind map signal".to_string(),
                    weight: 1,
                });
            if !linked_concepts.contains(&node.id) {
                edges.insert(("self".to_string(), concept_id, "pattern".to_string()), 1);
            }
        }

        for link in memory_links
            .iter()
            .filter(|link| link.from_kind == "concept" && link.to_kind == "person")
        {
            let Some(node) = graph_node_by_id
                .get(&normalize_slug(&link.from_id))
                .copied()
            else {
                continue;
            };
            let concept_id = social_graph_concept_node_id(node);
            if !nodes.contains_key(&concept_id) {
                continue;
            }
            let Some(person_id) =
                resolve_social_actor(&link.to_id, &link.to_id, &person_resolution)
            else {
                continue;
            };
            if !nodes.contains_key(&person_id) {
                continue;
            }
            let key = (
                concept_id,
                person_id,
                memory_link_relation(link, "relates_to"),
            );
            *edges.entry(key).or_insert(0) += link.weight.max(1);
        }

        let episode_by_id = episodes
            .iter()
            .map(|episode| (normalize_slug(&episode.id), episode))
            .collect::<HashMap<_, _>>();
        let mut episode_links = HashMap::<String, Vec<&MemoryLink>>::new();
        for link in memory_links {
            if let Some(episode_id) = memory_link_id_for_kind(link, "episode") {
                episode_links
                    .entry(normalize_slug(episode_id))
                    .or_default()
                    .push(link);
            }
        }
        let mut projected_episodes = episodes.iter().collect::<Vec<_>>();
        projected_episodes.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then(right.created_at.cmp(&left.created_at))
                .then(right.id.cmp(&left.id))
        });

        for episode in projected_episodes.into_iter().take(12) {
            let episode_key = normalize_slug(&episode.id);
            if !episode_by_id.contains_key(&episode_key) {
                continue;
            }
            let links = episode_links.get(&episode_key).cloned().unwrap_or_default();
            let involves_count = links
                .iter()
                .filter(|link| memory_link_connects(link, "episode", "person"))
                .count();
            let episode_node_id = format!("episode:{}", episode.id);
            nodes.insert(
                episode_node_id.clone(),
                SocialGraphNode {
                    id: episode_node_id.clone(),
                    label: episode.title.clone(),
                    kind: "episode".to_string(),
                    detail: episode
                        .occurred_at
                        .clone()
                        .unwrap_or_else(|| "episode".to_string()),
                    weight: 1 + involves_count,
                },
            );

            for link in links {
                if memory_link_connects(link, "episode", "person") {
                    let Some(person_slug) = memory_link_id_for_kind(link, "person") else {
                        continue;
                    };
                    let Some(person_id) =
                        resolve_social_actor(person_slug, person_slug, &person_resolution)
                    else {
                        continue;
                    };
                    if nodes.contains_key(&person_id) {
                        let key = (episode_node_id.clone(), person_id, "involves".to_string());
                        *edges.entry(key).or_insert(0) += link.weight.max(1);
                    }
                } else if memory_link_connects(link, "episode", "concept") {
                    let Some(concept_key) = memory_link_id_for_kind(link, "concept") else {
                        continue;
                    };
                    let Some(node) = graph_node_by_id.get(&normalize_slug(concept_key)).copied()
                    else {
                        continue;
                    };
                    let concept_id = social_graph_concept_node_id(node);
                    if nodes.contains_key(&concept_id) {
                        let key = (episode_node_id.clone(), concept_id, "evidences".to_string());
                        *edges.entry(key).or_insert(0) += link.weight.max(1);
                    }
                }
            }
        }

        let mut nodes = nodes.into_values().collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.label.cmp(&b.label)));
        let mut edges = edges
            .into_iter()
            .map(|((from, to, relation), weight)| SocialGraphEdge {
                from,
                to,
                relation,
                weight,
            })
            .collect::<Vec<_>>();
        edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.to.cmp(&b.to))
                .then(a.relation.cmp(&b.relation))
        });

        SocialGraph {
            user_id,
            nodes,
            edges,
        }
    }

    fn person_label_lookup(profiles: &[RelationshipProfile]) -> HashMap<String, String> {
        let mut labels = HashMap::new();
        for profile in profiles {
            let slug = normalize_slug(&profile.slug);
            if slug.is_empty() {
                continue;
            }
            labels.insert(
                slug,
                if profile.display_name.trim().is_empty() {
                    profile.slug.replace('_', " ")
                } else {
                    profile.display_name.trim().to_string()
                },
            );
        }
        labels
    }

    fn build_mind_map_payload(
        graph: &PatientGraph,
        profiles: &[RelationshipProfile],
        episodes: &[Episode],
        memory_links: &[MemoryLink],
    ) -> serde_json::Value {
        let graph_node_ids = graph
            .nodes
            .iter()
            .map(|node| normalize_slug(&node.id))
            .collect::<HashSet<_>>();
        let episode_by_id = episodes
            .iter()
            .map(|episode| (normalize_slug(&episode.id), episode))
            .collect::<HashMap<_, _>>();
        let labels = person_label_lookup(profiles);
        let mut people = HashMap::<String, String>::new();
        let mut episode_ids = HashSet::<String>::new();
        let mut cross_edges = Vec::<serde_json::Value>::new();
        let mut seen_edges = HashSet::<(String, String, String, String)>::new();

        for link in memory_links {
            if memory_link_connects(link, "concept", "person") {
                let Some(concept_id) = memory_link_id_for_kind(link, "concept") else {
                    continue;
                };
                let concept_id = normalize_slug(concept_id);
                if !graph_node_ids.contains(&concept_id) {
                    continue;
                }
                let Some(person_slug) = memory_link_id_for_kind(link, "person") else {
                    continue;
                };
                let person_slug = normalize_slug(person_slug);
                if person_slug.is_empty() {
                    continue;
                }
                people.entry(person_slug.clone()).or_insert_with(|| {
                    labels
                        .get(&person_slug)
                        .cloned()
                        .unwrap_or_else(|| person_slug.replace('_', " "))
                });
                let relation = memory_link_relation(link, "relates_to");
                let key = (
                    concept_id.clone(),
                    person_slug.clone(),
                    relation.clone(),
                    "concept_person".to_string(),
                );
                if seen_edges.insert(key) {
                    cross_edges.push(serde_json::json!({
                        "from": concept_id,
                        "to": person_slug,
                        "relation": relation,
                        "kind": "concept_person",
                    }));
                }
            } else if memory_link_connects(link, "episode", "concept") {
                let Some(episode_id) = memory_link_id_for_kind(link, "episode") else {
                    continue;
                };
                let episode_id = normalize_slug(episode_id);
                if !episode_by_id.contains_key(&episode_id) {
                    continue;
                }
                let Some(concept_id) = memory_link_id_for_kind(link, "concept") else {
                    continue;
                };
                let concept_id = normalize_slug(concept_id);
                if !graph_node_ids.contains(&concept_id) {
                    continue;
                }
                episode_ids.insert(episode_id.clone());
                let relation = memory_link_relation(link, "evidences");
                let from = format!("episode:{}", episode_id);
                let key = (
                    from.clone(),
                    concept_id.clone(),
                    relation.clone(),
                    "episode_concept".to_string(),
                );
                if seen_edges.insert(key) {
                    cross_edges.push(serde_json::json!({
                        "from": from,
                        "to": concept_id,
                        "relation": relation,
                        "kind": "episode_concept",
                    }));
                }
            }
        }

        let mut people = people.into_iter().collect::<Vec<_>>();
        people.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));

        let mut episode_values = episode_ids
            .into_iter()
            .filter_map(|id| episode_by_id.get(&id).copied())
            .collect::<Vec<_>>();
        episode_values.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then(right.created_at.cmp(&left.created_at))
                .then(right.id.cmp(&left.id))
        });

        serde_json::json!({
            "user_id": graph.user_id,
            "nodes": graph.nodes,
            "edges": graph.edges,
            "people": people.into_iter().map(|(slug, label)| {
                serde_json::json!({ "slug": slug, "label": label })
            }).collect::<Vec<_>>(),
            "episodes": episode_values.into_iter().map(|episode| {
                serde_json::json!({
                    "id": episode.id,
                    "title": episode.title,
                    "occurred_at": episode.occurred_at,
                })
            }).collect::<Vec<_>>(),
            "cross_edges": cross_edges,
        })
    }

    fn graph_walk_memory_scores(
        prompt: &str,
        graph: &PatientGraph,
        profiles: &[RelationshipProfile],
        memory_links: &[MemoryLink],
    ) -> (
        HashMap<String, i32>,
        HashMap<String, i32>,
        HashMap<String, i32>,
    ) {
        let query_terms = tokenize(prompt);
        let prompt_lower = prompt.to_lowercase();
        let mut concept_scores = HashMap::<String, i32>::new();
        let mut person_scores = HashMap::<String, i32>::new();
        let mut episode_scores = HashMap::<String, i32>::new();

        for profile in profiles {
            let slug = normalize_slug(&profile.slug);
            if slug.is_empty() {
                continue;
            }
            let display = profile.display_name.trim().to_lowercase();
            let slug_text = slug.replace('_', " ");
            if (!display.is_empty() && prompt_lower.contains(&display))
                || prompt_lower.contains(&slug_text)
            {
                *person_scores.entry(slug).or_insert(0) += 5;
            }
        }

        for node in &graph.nodes {
            let text = format!("{} {} {}", node.id, node.label, node.category);
            let score = overlap_score(&text, &query_terms);
            if score > 0 {
                *concept_scores.entry(normalize_slug(&node.id)).or_insert(0) += score;
            }
        }

        let is_seeded = |kind: &str,
                         id: &str,
                         concept_scores: &HashMap<String, i32>,
                         person_scores: &HashMap<String, i32>| {
            let id = normalize_slug(id);
            match kind {
                "concept" => concept_scores.get(&id).copied().unwrap_or_default() > 0,
                "person" => person_scores.get(&id).copied().unwrap_or_default() > 0,
                _ => false,
            }
        };

        for link in memory_links {
            let from_seeded = is_seeded(
                &link.from_kind,
                &link.from_id,
                &concept_scores,
                &person_scores,
            );
            let to_seeded = is_seeded(&link.to_kind, &link.to_id, &concept_scores, &person_scores);
            if !from_seeded && !to_seeded {
                continue;
            }

            let reached = if from_seeded {
                Some((link.to_kind.as_str(), link.to_id.as_str()))
            } else {
                Some((link.from_kind.as_str(), link.from_id.as_str()))
            };
            if let Some((kind, id)) = reached {
                let id = normalize_slug(id);
                if id.is_empty() {
                    continue;
                }
                let boost = 2 + link.weight.min(3) as i32;
                match kind {
                    "concept" => *concept_scores.entry(id).or_insert(1) += boost,
                    "person" => *person_scores.entry(id).or_insert(1) += boost,
                    "episode" => *episode_scores.entry(id).or_insert(1) += boost,
                    _ => {}
                }
            }
        }

        (concept_scores, person_scores, episode_scores)
    }

    fn episode_memory_excerpt(narrative: &str) -> String {
        let trimmed = narrative.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        let first_sentence = trimmed
            .split_terminator(['.', '!', '?'])
            .next()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .unwrap_or(trimmed);
        let source = if first_sentence.len() >= 40 {
            first_sentence
        } else {
            trimmed
        };
        source.chars().take(200).collect()
    }

    fn relevant_episode_lines(
        episodes: &[Episode],
        episode_scores: &HashMap<String, i32>,
        limit: usize,
    ) -> Vec<String> {
        let mut scored = episodes
            .iter()
            .filter_map(|episode| {
                let score = episode_scores
                    .get(&normalize_slug(&episode.id))
                    .copied()
                    .unwrap_or_default();
                if score <= 0 {
                    return None;
                }
                Some((episode, score))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|(left, left_score), (right, right_score)| {
            right_score
                .cmp(left_score)
                .then(right.updated_at.cmp(&left.updated_at))
                .then(right.created_at.cmp(&left.created_at))
                .then(right.id.cmp(&left.id))
        });
        scored
            .into_iter()
            .take(limit)
            .map(|(episode, _)| {
                format!(
                    "{} ({}): {} — {}",
                    episode.title,
                    episode
                        .occurred_at
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("episode"),
                    episode.narrative.trim(),
                    episode_memory_excerpt(&episode.narrative),
                )
            })
            .collect()
    }

    fn format_persistent_memory_block(
        broader_memories: &[String],
        graph_hits: &[String],
        social_hits: &[String],
        episode_hits: &[String],
    ) -> String {
        format!(
            "<persistent_memory>\nBroader autobiographical memories:\n{}\n\nRelevant mind map nodes/edges:\n{}\n\nRelevant social graph relationships:\n{}\n\nRelevant episodes:\n{}\n</persistent_memory>",
            if broader_memories.is_empty() { "none".to_string() } else { broader_memories.join("\n") },
            if graph_hits.is_empty() { "none".to_string() } else { graph_hits.join("\n") },
            if social_hits.is_empty() { "none".to_string() } else { social_hits.join("\n") },
            if episode_hits.is_empty() { "none".to_string() } else { episode_hits.join("\n") },
        )
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

    fn format_offset_datetime(value: ::time::OffsetDateTime) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            value.year(),
            value.month() as u8,
            value.day(),
            value.hour(),
            value.minute(),
            value.second()
        )
    }

    fn format_utc_offset(offset: ::time::UtcOffset) -> String {
        let seconds = offset.whole_seconds();
        let sign = if seconds < 0 { '-' } else { '+' };
        let abs = seconds.abs();
        format!("{}{:02}:{:02}", sign, abs / 3600, (abs % 3600) / 60)
    }

    fn current_datetime() -> CurrentDateTime {
        let utc = ::time::OffsetDateTime::now_utc();
        let offset = ::time::UtcOffset::current_local_offset().unwrap_or(::time::UtcOffset::UTC);
        let local = utc.to_offset(offset);
        CurrentDateTime {
            timezone: std::env::var("TZ").unwrap_or_else(|_| "server local timezone".to_string()),
            local_datetime: format_offset_datetime(local),
            utc_datetime: format!("{} UTC", format_offset_datetime(utc)),
            utc_offset: format_utc_offset(offset),
            unix_timestamp: utc.unix_timestamp(),
        }
    }

    impl Tool for CurrentDateTimeTool {
        const NAME: &'static str = "current_datetime";
        type Error = GraphToolError;
        type Args = CurrentDateTimeArgs;
        type Output = CurrentDateTime;

        async fn definition(&self, _prompt: String) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Return the current server-local date/time, UTC time, UTC offset, timezone label, and Unix timestamp. Use this when relative dates like today, tomorrow, yesterday, this week, or now matter.".to_string(),
                parameters: serde_json::json!(schema_for!(CurrentDateTimeArgs)),
            }
        }

        async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
            Ok(current_datetime())
        }
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
        #[allow(dead_code)]
        openai_client: openai::CompletionsClient,
        openrouter_client: openrouter::Client,
        #[allow(dead_code)]
        embedding_client: openai::Client,
        graph_reader: GraphReaderTool,
        graph_writer: GraphManagerTool,
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
            let conn = open_memory_db(&db_path).await?;

            // Init Chat + Graph Schema + Passkeys
            ensure_schema(&conn)
                .await
                .with_context(|| format!("Initializing chat schema at {db_path_display}"))?;

            ensure_local_test_user(&conn).await?;

            let openai_key =
                std::env::var("OPENAI_API_KEY").context("Set OPENAI_API_KEY for embeddings")?;
            let openai_client: openai::CompletionsClient = openai::CompletionsClient::builder()
                .api_key(openai_key.clone())
                .build()
                .context("Building OpenAI completions client")?;
            let embedding_client: openai::Client = openai::Client::builder()
                .api_key(openai_key)
                .build()
                .context("Building OpenAI embedding client")?;
            let embedding_model_name = std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| openai::TEXT_EMBEDDING_ADA_002.to_string());
            let embedding_model = embedding_client.embedding_model(embedding_model_name);

            let vector_store: SqliteVectorStore<_, MemoryFragment> =
                SqliteVectorStore::new(conn.clone(), &embedding_model)
                    .await
                    .context("Initializing sqlite vector store")?;

            if !table_has_rows(&conn, MemoryFragment::name())
                .await
                .unwrap_or(true)
            {
                let builder_result =
                    EmbeddingsBuilder::new(embedding_model.clone()).documents(seed_memory());
                match builder_result {
                    Ok(builder) => match builder.build().await {
                        Ok(embeddings) => {
                            if let Err(e) = vector_store.add_rows(embeddings).await {
                                tracing::warn!("Failed to seed vector store: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Vector store build failed: {}", e);
                        }
                    },
                    Err(e) => {
                        tracing::warn!("Vector store documents failed: {}", e);
                    }
                }
            }

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
                    .tool(CurrentDateTimeTool)
                    .build();
            let draft_agent =
                AgentBuilder::new(openrouter_client.completion_model(openrouter_model))
                    .name("individuateai_drafter")
                    .preamble(DRAFT_SYSTEM_PROMPT)
                    .build();

            let graph_reader = GraphReaderTool { conn: conn.clone() };
            let graph_writer = GraphManagerTool { conn: conn.clone() };
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
                openrouter_client,
                embedding_client,
                graph_reader,
                graph_writer,
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
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());
            let extractor = self
                .openrouter_client
                .extractor::<SessionSummaryData>(&model)
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
            let refresh_user_id = profile.user_id.clone();
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

            self.refresh_social_graph(refresh_user_id).await?;

            Ok(())
        }

        async fn write_social_graph(&self, graph: &SocialGraph) -> Result<()> {
            let user_id = graph.user_id.clone();
            let payload = serde_json::to_string(graph).context("Serializing social graph")?;
            self.conn
                .call(move |conn| {
                    conn.execute(
                        r###"
                        INSERT INTO social_graphs (user_id, graph_json)
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
                .context("Persisting social graph")?;
            Ok(())
        }

        async fn list_social_relationships(
            &self,
            user_id: String,
        ) -> Result<Vec<SocialRelationshipRecord>> {
            self.conn
                .call(move |conn| {
                    let mut stmt = conn.prepare(
                        r###"
                        SELECT from_slug, from_label, to_slug, to_label, relation, evidence, weight
                        FROM social_relationships
                        WHERE user_id = ?1
                        ORDER BY updated_at DESC
                        "###,
                    )?;
                    let rows = stmt.query_map([user_id], |row| {
                        Ok(SocialRelationshipRecord {
                            from_slug: row.get(0)?,
                            from_label: row.get(1)?,
                            to_slug: row.get(2)?,
                            to_label: row.get(3)?,
                            relation: row.get(4)?,
                            evidence: row.get(5)?,
                            weight: row.get::<_, i64>(6)?.max(1) as usize,
                        })
                    })?;
                    let mut items = Vec::new();
                    for row in rows {
                        items.push(row?);
                    }
                    Ok(items)
                })
                .await
                .context("Listing social relationships")
        }

        async fn upsert_social_relationship(
            &self,
            user_id: String,
            relationship: SocialRelationshipRecord,
        ) -> Result<()> {
            let from_slug = normalize_slug(&relationship.from_slug);
            let to_slug = normalize_slug(&relationship.to_slug);
            let relation = normalize_slug(&relationship.relation);
            if from_slug.is_empty()
                || to_slug.is_empty()
                || relation.is_empty()
                || from_slug == to_slug
            {
                return Ok(());
            }
            let from_label = relationship.from_label.trim().to_string();
            let to_label = relationship.to_label.trim().to_string();
            let evidence = relationship
                .evidence
                .trim()
                .chars()
                .take(240)
                .collect::<String>();
            let weight = relationship.weight.max(1) as i64;

            self.conn
                .call(move |conn| {
                    conn.execute(
                        r###"
                        INSERT INTO social_relationships
                            (user_id, from_slug, to_slug, relation, from_label, to_label, evidence, weight)
                        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                        ON CONFLICT(user_id, from_slug, to_slug, relation)
                        DO UPDATE SET
                            from_label = excluded.from_label,
                            to_label = excluded.to_label,
                            evidence = excluded.evidence,
                            weight = social_relationships.weight + excluded.weight,
                            updated_at = CURRENT_TIMESTAMP
                        "###,
                        rusqlite::params![
                            user_id,
                            from_slug,
                            to_slug,
                            relation,
                            from_label,
                            to_label,
                            evidence,
                            weight
                        ],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Persisting social relationship")?;

            Ok(())
        }

        async fn upsert_episode(&self, episode: Episode) -> Result<()> {
            upsert_episode_record(&self.conn, episode.clone()).await?;
            if let Err(err) = self.index_episode_memory(&episode).await {
                tracing::warn!("Episode vector indexing failed: {}", err);
            }
            Ok(())
        }

        async fn list_episodes(&self, user_id: String) -> Result<Vec<Episode>> {
            list_episode_records(&self.conn, user_id).await
        }

        async fn upsert_memory_link(&self, link: MemoryLink) -> Result<()> {
            upsert_memory_link_record(&self.conn, link).await
        }

        async fn list_memory_links(&self, user_id: String) -> Result<Vec<MemoryLink>> {
            list_memory_link_records(&self.conn, user_id).await
        }

        async fn index_episode_memory(&self, episode: &Episode) -> Result<()> {
            let episode_id = normalize_slug(&episode.id);
            if episode_id.is_empty() {
                return Ok(());
            }
            let memory_id = format!("episode:{}:{}", episode.user_id, episode_id);
            let user_id = episode.user_id.clone();
            let memory_id_for_delete = memory_id.clone();
            self.conn
                .call(move |conn| {
                    conn.execute(
                        "DELETE FROM therapy_memory_embeddings WHERE rowid IN (SELECT rowid FROM therapy_memory WHERE id = ?1)",
                        rusqlite::params![memory_id_for_delete],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
                    conn.execute(
                        "DELETE FROM therapy_memory WHERE id = ?1",
                        rusqlite::params![memory_id],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await
                .context("Deleting previous episode vector row")?;

            let embedding_model_name = std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| openai::TEXT_EMBEDDING_ADA_002.to_string());
            let embedding_model = self.embedding_client.embedding_model(embedding_model_name);
            let vector_store: SqliteVectorStore<_, MemoryFragment> =
                SqliteVectorStore::new(self.conn.clone(), &embedding_model)
                    .await
                    .context("Initializing vector store for episode")?;
            let fragment = MemoryFragment {
                id: format!("episode:{}:{}", user_id, episode_id),
                title: episode.title.clone(),
                content: format!("{}\n\n{}", episode.title, episode.narrative),
                tags: format!("episode,user:{}", user_id),
            };
            let embeddings = EmbeddingsBuilder::new(embedding_model)
                .documents(vec![fragment])
                .context("Building episode embedding document")?
                .build()
                .await
                .context("Embedding episode memory")?;
            vector_store
                .add_rows(embeddings)
                .await
                .context("Writing episode vector row")?;
            Ok(())
        }

        async fn refresh_social_graph(&self, user_id: String) -> Result<SocialGraph> {
            let profiles = self.list_relationship_profiles(user_id.clone()).await?;
            let social_relationships = self
                .list_social_relationships(user_id.clone())
                .await
                .unwrap_or_default();
            let patient_graph = self
                .read_patient_graph(user_id.clone())
                .await
                .unwrap_or_else(|_| PatientGraph {
                    user_id: user_id.clone(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                });
            let memory_links = self
                .list_memory_links(user_id.clone())
                .await
                .unwrap_or_default();
            let episodes = self
                .list_episodes(user_id.clone())
                .await
                .unwrap_or_default();
            let graph = build_social_graph(
                user_id,
                &profiles,
                &patient_graph,
                &social_relationships,
                &memory_links,
                &episodes,
            );
            self.write_social_graph(&graph).await?;
            Ok(graph)
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
            let roster = known_people_roster(existing_profiles);
            let existing_context = if existing_profiles.is_empty() {
                format!("{}\n\nNo saved relationship profiles yet.", roster)
            } else {
                let existing_details = existing_profiles
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
                    .join("\n");
                format!(
                    "{}\n\nExisting relationship profile details:\n{}",
                    roster, existing_details
                )
            };

            let model = std::env::var("RELATIONSHIP_PROFILE_MODEL")
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());
            let extractor = self
                .openrouter_client
                .extractor::<RelationshipProfileDelta>(&model)
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
        ) -> Result<Option<String>> {
            if source_text.trim().is_empty() {
                return Ok(None);
            }

            let existing_profiles = self.list_relationship_profiles(user_id.clone()).await?;
            let delta = self
                .extract_relationship_profiles_from_text(source_text, &existing_profiles)
                .await?;

            if delta.is_empty() {
                return Ok(None);
            }

            let headline = delta.profiles.first().map(|profile| {
                let label = if profile.display_name.trim().is_empty() {
                    &profile.relationship_type
                } else {
                    &profile.display_name
                };
                memory_headline_from_text(label, "Relationship context")
            });

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

            Ok(headline)
        }

        async fn extract_social_relationships_from_text(
            &self,
            source_text: String,
            existing_relationships: &[SocialRelationshipRecord],
            existing_profiles: &[RelationshipProfile],
        ) -> Result<SocialRelationshipDelta> {
            let roster = known_people_roster(existing_profiles);
            let existing_context = if existing_relationships.is_empty() {
                format!("{}\n\nNo saved social relationships yet.", roster)
            } else {
                let existing_details = existing_relationships
                    .iter()
                    .take(40)
                    .map(|item| {
                        format!(
                            "{} -> {} ({}) evidence: {}",
                            item.from_label, item.to_label, item.relation, item.evidence
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "{}\n\nExisting social relationships:\n{}",
                    roster, existing_details
                )
            };

            let model = std::env::var("SOCIAL_RELATIONSHIP_MODEL")
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());
            let extractor = self
                .openrouter_client
                .extractor::<SocialRelationshipDelta>(&model)
                .preamble(SOCIAL_RELATIONSHIP_PROMPT)
                .context(&existing_context)
                .build();

            extractor
                .extract(source_text)
                .await
                .map_err(|err| anyhow::anyhow!("Social relationship extraction failed: {}", err))
        }

        async fn sync_social_relationships_from_text(
            &self,
            user_id: String,
            source_text: String,
        ) -> Result<Option<String>> {
            if source_text.trim().is_empty() {
                return Ok(None);
            }

            let existing_relationships = self.list_social_relationships(user_id.clone()).await?;
            let existing_profiles = self.list_relationship_profiles(user_id.clone()).await?;
            let delta = self
                .extract_social_relationships_from_text(
                    source_text,
                    &existing_relationships,
                    &existing_profiles,
                )
                .await?;

            if delta.is_empty() {
                return Ok(None);
            }

            let headline = delta.relationships.first().map(|relationship| {
                memory_headline_from_text(
                    &format!("{} {}", relationship.from_label, relationship.to_label),
                    "Social connection",
                )
            });

            for extracted in delta.relationships {
                if !valid_social_person_actor(&extracted.from_slug, &extracted.from_label)
                    || !valid_social_person_actor(&extracted.to_slug, &extracted.to_label)
                {
                    continue;
                }
                let relationship = SocialRelationshipRecord {
                    from_slug: extracted.from_slug,
                    from_label: extracted.from_label,
                    to_slug: extracted.to_slug,
                    to_label: extracted.to_label,
                    relation: extracted.relation,
                    evidence: extracted.evidence,
                    weight: 1,
                };
                self.upsert_social_relationship(user_id.clone(), relationship)
                    .await?;
            }

            self.refresh_social_graph(user_id.clone()).await?;
            Ok(headline)
        }

        async fn extract_episodes_from_text(
            &self,
            user_text: String,
            existing_profiles: &[RelationshipProfile],
            existing_episodes: &[Episode],
            patient_graph: &PatientGraph,
        ) -> Result<EpisodeDelta> {
            let context = format!(
                "{}\n\n{}\n\n{}",
                known_people_roster(existing_profiles),
                episode_roster(existing_episodes),
                graph_node_context(patient_graph)
            );
            let model = std::env::var("EPISODE_EXTRACTOR_MODEL")
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());
            let extractor = self
                .openrouter_client
                .extractor::<EpisodeDelta>(&model)
                .preamble(EPISODE_PROMPT)
                .context(&context)
                .build();

            extractor
                .extract(user_text)
                .await
                .map_err(|err| anyhow::anyhow!("Episode extraction failed: {}", err))
        }

        async fn sync_episodes_from_text(
            &self,
            user_id: String,
            session_id: Option<String>,
            user_text: String,
        ) -> Result<Option<String>> {
            if user_text.trim().is_empty() {
                return Ok(None);
            }

            let existing_profiles = self.list_relationship_profiles(user_id.clone()).await?;
            let existing_episodes = self.list_episodes(user_id.clone()).await?;
            let patient_graph = self.read_patient_graph(user_id.clone()).await?;
            let delta = self
                .extract_episodes_from_text(
                    user_text,
                    &existing_profiles,
                    &existing_episodes,
                    &patient_graph,
                )
                .await?;

            if delta.is_empty() {
                return Ok(None);
            }

            let headline = delta
                .episodes
                .first()
                .map(|episode| memory_headline_from_text(&episode.title, "Episode memory"));
            let valid_people = known_person_slugs(&existing_profiles);
            let valid_concepts: HashSet<String> = patient_graph
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect();

            for extracted in delta.episodes {
                if let Some((episode, links)) = episode_and_links_from_extracted(
                    &user_id,
                    session_id.as_deref(),
                    extracted,
                    &valid_people,
                    &valid_concepts,
                ) {
                    self.upsert_episode(episode).await?;
                    for link in links {
                        self.upsert_memory_link(link).await?;
                    }
                }
            }

            Ok(headline)
        }

        async fn bootstrap_social_relationships_if_empty(&self, user_id: String) -> Result<()> {
            if !self
                .list_social_relationships(user_id.clone())
                .await?
                .is_empty()
            {
                return Ok(());
            }
            let logs = self.get_user_memory_logs(user_id.clone(), 180).await?;
            let source_text = compress_logs_for_profile_bootstrap(&logs, 16_000);
            self.sync_social_relationships_from_text(user_id, source_text)
                .await?;
            Ok(())
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

        async fn build_therapist_context(&self, user_id: String, prompt: String) -> Result<String> {
            let graph = self
                .read_patient_graph(user_id.clone())
                .await
                .unwrap_or_default();
            let social_relationships = self
                .list_social_relationships(user_id.clone())
                .await
                .unwrap_or_default();
            let profiles = self
                .list_relationship_profiles(user_id.clone())
                .await
                .unwrap_or_default();
            let episodes = self
                .list_episodes(user_id.clone())
                .await
                .unwrap_or_default();
            let memory_links = self
                .list_memory_links(user_id.clone())
                .await
                .unwrap_or_default();
            let all_logs = self.get_user_memory_logs(user_id.clone(), 250).await?;
            let query_terms = tokenize(&prompt);
            let (walk_concept_scores, _walk_person_scores, walk_episode_scores) =
                graph_walk_memory_scores(&prompt, &graph, &profiles, &memory_links);

            let mut memory_candidates = Vec::new();
            for (role, content, title, created_at) in all_logs {
                if content.trim().is_empty() {
                    continue;
                }
                let score = overlap_score(&content, &query_terms);
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
                let score = overlap_score(&text, &query_terms)
                    + walk_concept_scores
                        .get(&normalize_slug(&node.id))
                        .copied()
                        .unwrap_or_default();
                if score > 0 {
                    graph_candidates.push(MemoryCandidate {
                        score,
                        summary: format!("{} [{}]", node.label, node.category),
                    });
                }
            }
            for edge in &graph.edges {
                let text = format!("{} {} {}", edge.from, edge.to, edge.relation);
                let score = overlap_score(&text, &query_terms);
                if score > 0 {
                    graph_candidates.push(MemoryCandidate {
                        score,
                        summary: format!("{} -> {} ({})", edge.from, edge.to, edge.relation),
                    });
                }
            }
            for link in &memory_links {
                if !memory_link_connects(link, "concept", "person") {
                    continue;
                }
                let Some(concept_id) = memory_link_id_for_kind(link, "concept") else {
                    continue;
                };
                let concept_score = walk_concept_scores
                    .get(&normalize_slug(concept_id))
                    .copied()
                    .unwrap_or_default();
                if concept_score <= 0 {
                    continue;
                }
                let Some(person_id) = memory_link_id_for_kind(link, "person") else {
                    continue;
                };
                graph_candidates.push(MemoryCandidate {
                    score: concept_score + link.weight.min(4) as i32,
                    summary: format!(
                        "{} -> {} ({})",
                        concept_id,
                        person_id,
                        memory_link_relation(link, "relates_to")
                    ),
                });
            }
            graph_candidates.sort_by(|left, right| right.score.cmp(&left.score));
            let graph_hits: Vec<String> = graph_candidates
                .into_iter()
                .take(6)
                .map(|item| item.summary)
                .collect();

            let mut social_candidates = Vec::new();
            for relationship in &social_relationships {
                let text = format!(
                    "{} {} {} {} {}",
                    relationship.from_label,
                    relationship.from_slug,
                    relationship.relation,
                    relationship.to_label,
                    relationship.evidence
                );
                let mut score = overlap_score(&text, &query_terms);
                score += relationship.weight.min(4) as i32;
                if score > 0 {
                    social_candidates.push(MemoryCandidate {
                        score,
                        summary: format!(
                            "{} -> {} ({}) evidence: {}",
                            relationship.from_label,
                            relationship.to_label,
                            relationship.relation,
                            relationship.evidence
                        ),
                    });
                }
            }
            social_candidates.sort_by(|left, right| right.score.cmp(&left.score));
            let social_hits: Vec<String> = social_candidates
                .into_iter()
                .take(8)
                .map(|item| item.summary)
                .collect();
            let episode_hits = relevant_episode_lines(&episodes, &walk_episode_scores, 4);

            Ok(format_persistent_memory_block(
                &broader_memories,
                &graph_hits,
                &social_hits,
                &episode_hits,
            ))
        }

        async fn read_patient_graph(&self, user_id: String) -> Result<PatientGraph> {
            self.graph_reader
                .call(GraphReadArgs { user_id })
                .await
                .context("Reading patient graph")
        }

        fn spawn_memory_update(
            self: &Arc<Self>,
            user_id: String,
            session_id: Option<String>,
            prompt: String,
            reply: String,
        ) {
            let runtime = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(err) = runtime
                    .update_memory_from_exchange(user_id, session_id, prompt, reply)
                    .await
                {
                    eprintln!("[memory_update] {}", err);
                }
            });
        }

        async fn update_graph_from_exchange(
            &self,
            user_id: String,
            prompt: String,
            reply: String,
        ) -> Result<Option<String>> {
            let current_graph = self.read_patient_graph(user_id.clone()).await?;
            let profiles = self
                .list_relationship_profiles(user_id.clone())
                .await
                .unwrap_or_default();
            let context = format!(
                "{}\n\n{}",
                graph_context(&current_graph),
                known_people_roster(&profiles)
            );

            let model = std::env::var("GRAPH_EXTRACTOR_MODEL")
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());
            let extractor = self
                .openrouter_client
                .extractor::<ConversationGraphDelta>(&model)
                .preamble(GRAPH_DELTA_PROMPT)
                .context(&context)
                .build();

            let transcript = format!("User: {}\nAssistant: {}", prompt, reply);
            let delta = extractor
                .extract(transcript)
                .await
                .map_err(|err| anyhow::anyhow!("Extractor failed: {}", err))?;

            if delta.is_empty() {
                return Ok(None);
            }
            let person_links = delta.person_links;

            let headline = delta
                .new_concepts
                .first()
                .map(|node| memory_headline_from_text(&node.label, "Mind map update"))
                .or_else(|| {
                    delta
                        .new_connections
                        .first()
                        .map(|edge| memory_headline_from_text(&edge.relation, "Mind map update"))
                });

            let update = GraphUpdateArgs {
                user_id: user_id.clone(),
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
            self.refresh_social_graph(user_id.clone()).await?;
            let updated_graph = self.read_patient_graph(user_id.clone()).await?;
            let valid_people = known_person_slugs(&profiles);
            let valid_concepts: HashSet<String> = updated_graph
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect();
            let graph_person_links = memory_links_from_person_links(
                &user_id,
                person_links,
                &valid_people,
                &valid_concepts,
            );
            let wrote_person_links = !graph_person_links.is_empty();
            for link in graph_person_links {
                self.upsert_memory_link(link).await?;
            }
            if summary.added_nodes > 0
                || summary.added_edges > 0
                || summary.removed_nodes > 0
                || summary.removed_edges > 0
                || wrote_person_links
            {
                Ok(headline.or_else(|| Some("Mind map update".to_string())))
            } else {
                Ok(None)
            }
        }

        async fn update_memory_from_exchange(
            &self,
            user_id: String,
            session_id: Option<String>,
            prompt: String,
            reply: String,
        ) -> Result<Option<String>> {
            let source_text = format!("User: {}\nAssistant: {}", prompt, reply);
            let graph_headline = self
                .update_graph_from_exchange(user_id.clone(), prompt.clone(), reply)
                .await?;
            let profile_headline = self
                .sync_relationship_profiles_from_text(user_id.clone(), source_text.clone())
                .await?;
            let episode_headline = self
                .sync_episodes_from_text(user_id.clone(), session_id, prompt)
                .await?;
            let social_headline = self
                .sync_social_relationships_from_text(user_id, source_text)
                .await?;
            Ok(graph_headline
                .or(profile_headline)
                .or(episode_headline)
                .or(social_headline))
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

            let memory_context = self
                .build_therapist_context(user_id.to_string(), prompt.clone())
                .await
                .unwrap_or_default();
            let enriched_prompt = format!("{}\n\n{}", memory_context, prompt);

            let mut history_clone = history.clone();
            let reply = self
                .therapist_agent
                .prompt(Message::user(enriched_prompt))
                .with_history(&mut history_clone)
                .multi_turn(2)
                .await
                .inspect_err(|e| tracing::error!("Therapist agent API error: {}", e))
                .context("Running agent prompt")?;

            history.push(Message::user(prompt.clone()));
            if !reply.is_empty() {
                history.push(Message::Assistant {
                    id: None,
                    content: rig::OneOrMany::one(AssistantContent::Text(Text {
                        text: reply.clone(),
                    })),
                });
            }

            self.save_message(session_id.to_string(), "assistant".into(), reply.clone())
                .await?;

            let mut guard = self.histories.write().await;
            guard.insert(session_id.to_string(), history);

            self.spawn_session_summary_update(session_id.to_string());
            self.spawn_memory_update(
                user_id.to_string(),
                Some(session_id.to_string()),
                prompt,
                reply.clone(),
            );
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
            self.spawn_memory_update(
                user_id.to_string(),
                Some(session_id.to_string()),
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
                            let text = resp.response().to_string();
                            if assembled.is_empty() {
                                send_visible_stream(&tx, &text).await;
                                assembled.push_str(&text);
                            } else if let Some(suffix) = text.strip_prefix(&assembled) {
                                if !suffix.is_empty() {
                                    send_visible_stream(&tx, suffix).await;
                                    assembled.push_str(suffix);
                                }
                            }
                            final_text.get_or_insert(text);
                            break;
                        }
                        Some(Err(e)) => {
                            eprintln!("[draft-stream:error] {}", e);
                            let _ = tx.send(Ok(format!("error:{e}"))).await;
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
                let _ = tx.send(Ok("[RESPONSE_DONE]".to_string())).await;

                if !final_content.is_empty() {
                    match runtime
                        .update_memory_from_exchange(
                            user_id_clone.clone(),
                            Some(session_id_clone.clone()),
                            request_label.clone(),
                            final_content.clone(),
                        )
                        .await
                    {
                        Ok(Some(headline)) => {
                            let _ = tx.send(Ok(format!("[MEMORY_UPDATED]{}", headline))).await;
                        }
                        Ok(None) => {}
                        Err(err) => eprintln!("[memory_update] {}", err),
                    }
                }
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

            let memory_context = self
                .build_therapist_context(user_id.clone(), prompt.clone())
                .await
                .unwrap_or_default();
            let enriched_prompt = format!("{}\n\n{}", memory_context, prompt);

            let mut stream = self
                .therapist_agent
                .stream_prompt(Message::user(enriched_prompt))
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
                            let text = resp.response().to_string();
                            if assembled.is_empty() {
                                send_visible_stream(&tx, &text).await;
                                assembled.push_str(&text);
                            } else if let Some(suffix) = text.strip_prefix(&assembled) {
                                if !suffix.is_empty() {
                                    send_visible_stream(&tx, suffix).await;
                                    assembled.push_str(suffix);
                                }
                            }
                            final_text.get_or_insert(text);
                            break;
                        }
                        Some(Err(e)) => {
                            eprintln!("[agent-stream:error] {}", e);
                            let _ = tx.send(Ok(format!("error:{e}"))).await;
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
                let _ = tx.send(Ok("[RESPONSE_DONE]".to_string())).await;

                if !final_content.is_empty() {
                    match runtime
                        .update_memory_from_exchange(
                            user_id_clone.clone(),
                            Some(session_id_clone.clone()),
                            prompt.clone(),
                            final_content.clone(),
                        )
                        .await
                    {
                        Ok(Some(headline)) => {
                            let _ = tx.send(Ok(format!("[MEMORY_UPDATED]{}", headline))).await;
                        }
                        Ok(None) => {}
                        Err(err) => eprintln!("[memory_update] {}", err),
                    }
                }
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

        pub async fn get_mind_map_payload(&self, user_id: String) -> Result<serde_json::Value> {
            let graph = self.read_patient_graph(user_id.clone()).await?;
            let profiles = self
                .list_relationship_profiles(user_id.clone())
                .await
                .unwrap_or_default();
            let episodes = self
                .list_episodes(user_id.clone())
                .await
                .unwrap_or_default();
            let memory_links = self.list_memory_links(user_id).await.unwrap_or_default();
            Ok(build_mind_map_payload(
                &graph,
                &profiles,
                &episodes,
                &memory_links,
            ))
        }

        pub async fn get_social_graph(&self, user_id: String) -> Result<SocialGraph> {
            if let Err(err) = self
                .bootstrap_social_relationships_if_empty(user_id.clone())
                .await
            {
                tracing::warn!("Social graph bootstrap failed: {}", err);
            }
            self.refresh_social_graph(user_id).await
        }

        pub async fn get_episodes_with_links(
            &self,
            user_id: String,
        ) -> Result<Vec<EpisodeWithLinks>> {
            let episodes = self.list_episodes(user_id.clone()).await?;
            let memory_links = self.list_memory_links(user_id).await?;
            Ok(episodes
                .into_iter()
                .map(|episode| {
                    let episode_id = normalize_slug(&episode.id);
                    let links = memory_links
                        .iter()
                        .filter(|link| {
                            memory_link_id_for_kind(link, "episode")
                                .map(normalize_slug)
                                .as_deref()
                                == Some(episode_id.as_str())
                        })
                        .cloned()
                        .collect();
                    EpisodeWithLinks { episode, links }
                })
                .collect())
        }

        pub async fn get_memory_status(&self, user_id: String) -> Result<MemoryStatus> {
            let mind = read_patient_graph_snapshot(&self.conn, &user_id)
                .await
                .unwrap_or_else(|_| PatientGraph {
                    user_id: user_id.clone(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                });
            let social = read_social_graph(&self.conn, &user_id)
                .await
                .unwrap_or_else(|_| SocialGraph {
                    user_id: user_id.clone(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                });
            let episodes = self
                .list_episodes(user_id.clone())
                .await
                .unwrap_or_default();
            let memory_links = self
                .list_memory_links(user_id.clone())
                .await
                .unwrap_or_default();
            let memory_payload = (&episodes, &memory_links);

            Ok(MemoryStatus {
                mind_nodes: mind.nodes.len(),
                mind_edges: mind.edges.len(),
                mind_signature: private_signature(&mind),
                social_nodes: social.nodes.len(),
                social_edges: social.edges.len(),
                social_signature: private_signature(&social),
                episode_count: episodes.len(),
                memory_signature: private_signature(&memory_payload),
            })
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

        // --- Password Reset ---

        pub async fn generate_password_reset_token(&self, email: &str) -> Result<String> {
            let user = self
                .get_user_by_username(email)
                .await?
                .ok_or_else(|| anyhow::anyhow!("No account found with that email"))?;

            let token = Uuid::new_v4().to_string();
            let user_id = user.id.clone();
            let token_clone = token.clone();

            self.conn
                .call(move |conn| {
                    conn.execute(
                        r###"
                        INSERT INTO password_reset_tokens (user_id, token, expires_at)
                        VALUES (?1, ?2, datetime('now', '+1 hours'))
                        "###,
                        rusqlite::params![user_id, token_clone],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            Ok(token)
        }

        pub async fn verify_and_reset_password(
            &self,
            token: &str,
            new_password: &str,
        ) -> Result<()> {
            let token_owned = token.to_string();
            let (user_id,) = self
                .conn
                .call(move |conn| {
                    let result: Option<(String,)> = conn
                        .query_row(
                            r###"
                            SELECT user_id FROM password_reset_tokens
                            WHERE token = ?1 AND used = 0 AND expires_at > datetime('now')
                            "###,
                            [token_owned],
                            |row| Ok((row.get::<_, String>(0)?,)),
                        )
                        .optional()
                        .map_err(tokio_rusqlite::Error::Rusqlite)?;
                    Ok(result)
                })
                .await?
                .ok_or_else(|| anyhow::anyhow!("Invalid or expired reset token"))?;

            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            let password_hash = argon2
                .hash_password(new_password.as_bytes(), &salt)
                .map_err(|e| anyhow::anyhow!("Hashing failed: {}", e))?
                .to_string();

            let user_id_for_update = user_id.clone();
            self.conn
                .call(move |conn| {
                    conn.execute(
                        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
                        rusqlite::params![password_hash, user_id_for_update],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            let token_for_use = token.to_string();
            self.conn
                .call(move |conn| {
                    conn.execute(
                        "UPDATE password_reset_tokens SET used = 1 WHERE token = ?1",
                        [token_for_use],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;

            Ok(())
        }

        // --- Email Verification ---

        pub async fn verify_email(&self, user_id: &str) -> Result<()> {
            let uid = user_id.to_string();
            self.conn
                .call(move |conn| {
                    conn.execute("UPDATE users SET email_verified = 1 WHERE id = ?1", [uid])
                        .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;
            Ok(())
        }

        pub async fn generate_email_verification_token(&self, user_id: &str) -> Result<String> {
            let token = Uuid::new_v4().to_string();
            let uid = user_id.to_string();
            let token_clone = token.clone();
            self.conn
                .call(move |conn| {
                    conn.execute(
                        r###"
                        INSERT INTO password_reset_tokens (user_id, token, expires_at)
                        VALUES (?1, ?2, datetime('now', '+24 hours'))
                        "###,
                        rusqlite::params![uid, token_clone],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;
            Ok(token)
        }

        pub async fn verify_email_with_token(&self, token: &str) -> Result<()> {
            let token_owned = token.to_string();
            let (user_id,): (String,) = self
                .conn
                .call(move |conn| {
                    conn.query_row(
                        r###"
                        SELECT user_id FROM password_reset_tokens
                        WHERE token = ?1 AND used = 0 AND expires_at > datetime('now')
                        "###,
                        [token_owned],
                        |row| Ok((row.get::<_, String>(0)?,)),
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;
            self.verify_email(&user_id).await?;
            let token_for_use = token.to_string();
            self.conn
                .call(move |conn| {
                    conn.execute(
                        "UPDATE password_reset_tokens SET used = 1 WHERE token = ?1",
                        [token_for_use],
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)
                })
                .await?;
            Ok(())
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
    ) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
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
            .get_mind_map_payload(user_id)
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_encrypt_plaintext_db_roundtrip() {
            let dir = std::env::temp_dir().join(format!("sqlcipher-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let db_path = dir.join("memory.sqlite");
            let db_path_str = db_path.to_str().unwrap();

            {
                let conn = rusqlite::Connection::open(db_path_str).unwrap();
                conn.execute_batch(
                    "CREATE TABLE secrets (id INTEGER PRIMARY KEY, note TEXT);
                     INSERT INTO secrets (note) VALUES ('very private');",
                )
                .unwrap();
            }

            encrypt_plaintext_db(db_path_str, "test-passphrase").unwrap();

            // Plaintext backup should exist; main file should no longer open unkeyed.
            assert!(dir.join("memory.sqlite.plaintext.bak").exists());
            let unkeyed = rusqlite::Connection::open(db_path_str).unwrap();
            assert!(unkeyed
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                    .get::<_, i64>(0))
                .is_err());
            drop(unkeyed);

            // Keyed open should read the migrated data.
            let keyed = rusqlite::Connection::open(db_path_str).unwrap();
            keyed.pragma_update(None, "key", "test-passphrase").unwrap();
            let note: String = keyed
                .query_row("SELECT note FROM secrets", [], |r| r.get(0))
                .unwrap();
            assert_eq!(note, "very private");

            // Re-running the migration on an already-encrypted db is a no-op.
            encrypt_plaintext_db(db_path_str, "test-passphrase").unwrap();
            let keyed = rusqlite::Connection::open(db_path_str).unwrap();
            keyed.pragma_update(None, "key", "test-passphrase").unwrap();
            let count: i64 = keyed
                .query_row("SELECT count(*) FROM secrets", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);

            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn test_tokenize_basic() {
            let tokens = tokenize("hello world example");
            assert!(tokens.contains("hello"));
            assert!(tokens.contains("world"));
            assert!(tokens.contains("example"));
            assert_eq!(tokens.len(), 3);
        }

        #[test]
        fn test_tokenize_short_words() {
            let tokens = tokenize("a be");
            assert!(
                tokens.is_empty(),
                "Words shorter than 3 chars should be excluded"
            );
        }

        #[test]
        fn test_tokenize_case_insensitive() {
            let tokens = tokenize("Hello WORLD");
            assert!(tokens.contains("hello"));
            assert!(tokens.contains("world"));
        }

        #[test]
        fn test_tokenize_special_chars() {
            let tokens = tokenize("hello-world! example_test");
            assert!(tokens.contains("hello"));
            assert!(tokens.contains("world"));
            assert!(tokens.contains("example"));
            assert!(tokens.contains("test"));
        }

        #[test]
        fn test_normalize_slug() {
            assert_eq!(normalize_slug("Hello World"), "hello_world");
            assert_eq!(normalize_slug("  leading trailing  "), "leading_trailing");
            assert_eq!(normalize_slug("already_snake"), "already_snake");
        }

        #[test]
        fn test_canonical_relationship_slug_mother() {
            assert_eq!(canonical_relationship_slug("mom", "", ""), "mother");
            assert_eq!(canonical_relationship_slug("mother", "", ""), "mother");
            assert_eq!(canonical_relationship_slug("mum", "", ""), "mother");
            assert_eq!(canonical_relationship_slug("mama", "", ""), "mother");
        }

        #[test]
        fn test_canonical_relationship_slug_partner() {
            assert_eq!(canonical_relationship_slug("partner", "", ""), "partner");
            assert_eq!(canonical_relationship_slug("wife", "", ""), "partner");
            assert_eq!(canonical_relationship_slug("girlfriend", "", ""), "partner");
            assert_eq!(canonical_relationship_slug("husband", "", ""), "partner");
        }

        #[test]
        fn test_merge_unique_strings() {
            let existing = vec!["a".into(), "b".into()];
            let incoming = vec!["b".into(), "c".into()];
            let merged = merge_unique_strings(&existing, &incoming, 10);
            assert_eq!(merged, vec!["a", "b", "c"]);
        }

        #[test]
        fn test_merge_unique_strings_limit() {
            let existing = vec!["a".into(), "b".into()];
            let incoming = vec!["c".into(), "d".into()];
            let merged = merge_unique_strings(&existing, &incoming, 3);
            assert_eq!(merged.len(), 3);
            assert_eq!(merged, vec!["a", "b", "c"]);
        }

        #[test]
        fn test_merge_unique_strings_duplicates() {
            let existing = vec!["hello".into(), "world".into()];
            let incoming = vec!["HELLO".into(), "World".into(), "again".into()];
            let merged = merge_unique_strings(&existing, &incoming, 10);
            assert_eq!(merged.len(), 3);
            assert!(merged.contains(&"hello".to_string()));
        }

        #[test]
        fn test_merge_background_empty() {
            assert_eq!(merge_background("", "new info"), "new info");
            assert_eq!(merge_background("existing", ""), "existing");
            assert_eq!(merge_background("", ""), "");
        }

        #[test]
        fn test_merge_background_duplicate() {
            assert_eq!(merge_background("some text", "some text"), "some text");
        }

        #[test]
        fn test_merge_background_contains() {
            assert_eq!(
                merge_background("longer text here", "text"),
                "longer text here"
            );
            assert_eq!(
                merge_background("text", "longer text here"),
                "longer text here"
            );
        }

        #[test]
        fn test_merge_background_combine() {
            let result = merge_background("first part", "second part");
            assert_eq!(result, "first part second part");
        }

        #[test]
        fn test_overlap_score() {
            let query = tokenize("hello world test");
            let score = overlap_score("hello world", &query);
            assert_eq!(score, 2);
        }

        #[test]
        fn test_overlap_score_no_match() {
            let query = tokenize("hello world");
            let score = overlap_score("completely different", &query);
            assert_eq!(score, 0);
        }

        #[test]
        fn test_fallback_session_preview() {
            let logs = vec![
                ChatLog {
                    role: "user".into(),
                    content: "first".into(),
                },
                ChatLog {
                    role: "assistant".into(),
                    content: "I am helping you explore".into(),
                },
                ChatLog {
                    role: "user".into(),
                    content: "last message".into(),
                },
            ];
            let preview = fallback_session_preview(&logs);
            assert_eq!(preview, "last message");
        }

        #[test]
        fn test_fallback_session_preview_empty() {
            let logs: Vec<ChatLog> = vec![];
            let preview = fallback_session_preview(&logs);
            assert_eq!(preview, "Begin exploring what's here.");
        }

        #[test]
        fn test_compress_chat_logs() {
            let logs = vec![
                ChatLog {
                    role: "user".into(),
                    content: "hello".into(),
                },
                ChatLog {
                    role: "assistant".into(),
                    content: "hi there".into(),
                },
            ];
            let compressed = compress_chat_logs(&logs, 1000);
            assert!(compressed.contains("user: hello"));
            assert!(compressed.contains("assistant: hi there"));
        }

        #[test]
        fn test_compress_chat_logs_max_chars() {
            let logs = vec![
                ChatLog {
                    role: "user".into(),
                    content: "very long message that should be truncated".into(),
                },
                ChatLog {
                    role: "assistant".into(),
                    content: "short".into(),
                },
            ];
            let compressed = compress_chat_logs(&logs, 20);
            assert!(compressed.len() <= 20);
        }

        #[test]
        fn test_join_items() {
            assert_eq!(join_items(&[]), "none");
            assert_eq!(join_items(&["a".into()]), "a");
            assert_eq!(join_items(&["a".into(), "b".into()]), "a, b");
        }

        #[tokio::test]
        async fn test_episode_upsert_merges_quotes_and_updates_record() {
            let conn = Connection::open_in_memory().await.unwrap();
            ensure_schema(&conn).await.unwrap();

            upsert_episode_record(
                &conn,
                Episode {
                    user_id: "user-1".to_string(),
                    id: "Test Phone Incident".to_string(),
                    title: "Test phone incident".to_string(),
                    narrative: "The first account.".to_string(),
                    occurred_at: Some("Test date".to_string()),
                    session_id: Some("session-a".to_string()),
                    user_quotes: vec!["first quote".to_string()],
                    created_at: None,
                    updated_at: None,
                },
            )
            .await
            .unwrap();

            let before = list_episode_records(&conn, "user-1".to_string())
                .await
                .unwrap()
                .remove(0);

            upsert_episode_record(
                &conn,
                Episode {
                    user_id: "user-1".to_string(),
                    id: "test_phone_incident".to_string(),
                    title: "Test phone call".to_string(),
                    narrative: "The updated account.".to_string(),
                    occurred_at: None,
                    session_id: Some("session-b".to_string()),
                    user_quotes: vec!["first quote".to_string(), "second quote".to_string()],
                    created_at: None,
                    updated_at: None,
                },
            )
            .await
            .unwrap();

            let after = list_episode_records(&conn, "user-1".to_string())
                .await
                .unwrap()
                .remove(0);

            assert_eq!(after.id, "test_phone_incident");
            assert_eq!(after.title, "Test phone call");
            assert_eq!(after.narrative, "The updated account.");
            assert_eq!(after.occurred_at.as_deref(), Some("Test date"));
            assert_eq!(after.session_id.as_deref(), Some("session-b"));
            assert_eq!(
                after.user_quotes,
                vec!["first quote".to_string(), "second quote".to_string()]
            );
            assert_eq!(after.created_at, before.created_at);
            assert_ne!(after.updated_at, before.updated_at);
        }

        #[tokio::test]
        async fn test_memory_link_duplicate_upsert_increments_weight() {
            let conn = Connection::open_in_memory().await.unwrap();
            ensure_schema(&conn).await.unwrap();

            let link = MemoryLink {
                user_id: "user-1".to_string(),
                from_kind: "episode".to_string(),
                from_id: "test_phone_incident".to_string(),
                relation: "involves".to_string(),
                to_kind: "person".to_string(),
                to_id: "mother".to_string(),
                evidence: "Test phone call".to_string(),
                weight: 1,
                created_at: None,
                updated_at: None,
            };

            upsert_memory_link_record(&conn, link.clone())
                .await
                .unwrap();
            upsert_memory_link_record(&conn, link).await.unwrap();

            let links = list_memory_link_records(&conn, "user-1".to_string())
                .await
                .unwrap();
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].weight, 2);
        }

        #[test]
        fn test_person_link_validation_drops_unknown_slugs() {
            let valid_people = HashSet::from(["self".to_string(), "mother".to_string()]);
            let valid_concepts = HashSet::from(["fear_of_rejection".to_string()]);
            let links = memory_links_from_person_links(
                "user-1",
                vec![
                    ExtractedPersonLink {
                        concept_id: "fear_of_rejection".to_string(),
                        person_slug: "mother".to_string(),
                        relation: "originates_from".to_string(),
                        evidence: "supported".to_string(),
                    },
                    ExtractedPersonLink {
                        concept_id: "fear_of_rejection".to_string(),
                        person_slug: "unknown_friend".to_string(),
                        relation: "originates_from".to_string(),
                        evidence: "unsupported".to_string(),
                    },
                ],
                &valid_people,
                &valid_concepts,
            );

            assert_eq!(links.len(), 1);
            assert_eq!(links[0].to_id, "mother");
        }

        #[test]
        fn test_episode_payload_maps_to_episode_and_memory_links() {
            let valid_people = HashSet::from(["self".to_string(), "mother".to_string()]);
            let valid_concepts = HashSet::from(["fear_of_rejection".to_string()]);
            let extracted = ExtractedEpisode {
                id: "Test Phone Incident".to_string(),
                title: "Test phone incident".to_string(),
                narrative: "The user said their a test participant reported a disagreement them on a Test phone call."
                    .to_string(),
                occurred_at: Some("Test date".to_string()),
                participants: vec!["mother".to_string(), "unknown".to_string()],
                concepts: vec!["fear_of_rejection".to_string(), "missing_node".to_string()],
                user_quotes: vec!["she criticized me".to_string()],
            };

            let (episode, links) = episode_and_links_from_extracted(
                "user-1",
                Some("session-a"),
                extracted,
                &valid_people,
                &valid_concepts,
            )
            .unwrap();

            assert_eq!(episode.id, "test_phone_incident");
            assert_eq!(episode.session_id.as_deref(), Some("session-a"));
            assert_eq!(links.len(), 2);
            assert!(links.iter().any(|link| {
                link.relation == "involves" && link.to_kind == "person" && link.to_id == "mother"
            }));
            assert!(links.iter().any(|link| {
                link.relation == "evidences"
                    && link.to_kind == "concept"
                    && link.to_id == "fear_of_rejection"
            }));
        }

        fn test_profile(
            slug: &str,
            display_name: &str,
            relationship_type: &str,
        ) -> RelationshipProfile {
            RelationshipProfile {
                user_id: "test-user".to_string(),
                slug: slug.to_string(),
                display_name: display_name.to_string(),
                relationship_type: relationship_type.to_string(),
                background: String::new(),
                goals: Vec::new(),
                triggers: Vec::new(),
                do_not_say: Vec::new(),
                effective_tone: Vec::new(),
                recent_events: Vec::new(),
                boundaries: Vec::new(),
            }
        }

        fn test_relationship(
            from_slug: &str,
            from_label: &str,
            to_slug: &str,
            to_label: &str,
            relation: &str,
            weight: usize,
        ) -> SocialRelationshipRecord {
            SocialRelationshipRecord {
                from_slug: from_slug.to_string(),
                from_label: from_label.to_string(),
                to_slug: to_slug.to_string(),
                to_label: to_label.to_string(),
                relation: relation.to_string(),
                evidence: String::new(),
                weight,
            }
        }

        #[test]
        fn test_social_graph_merges_test_partner_into_partner_by_display_name() {
            let profiles = vec![
                test_profile("partner", "Test Partner", "partner"),
                test_profile("test_partner", "Test Partner", "friend"),
            ];
            let graph = build_social_graph(
                "test-user".to_string(),
                &profiles,
                &PatientGraph::default(),
                &[],
                &[],
                &[],
            );

            assert!(graph.nodes.iter().any(|node| {
                node.id == "person:partner" && node.label == "Test Partner" && node.detail == "partner"
            }));
            assert!(!graph.nodes.iter().any(|node| node.id == "person:test_partner"));
        }

        #[test]
        fn test_social_graph_merges_father_and_dad_synonyms() {
            let profiles = vec![
                test_profile("father", "Dad", "father"),
                test_profile("dad", "Dad", "father"),
            ];
            let graph = build_social_graph(
                "test-user".to_string(),
                &profiles,
                &PatientGraph::default(),
                &[],
                &[],
                &[],
            );

            let dad_nodes = graph
                .nodes
                .iter()
                .filter(|node| node.id == "person:dad")
                .count();
            assert_eq!(dad_nodes, 1);
            assert!(!graph.nodes.iter().any(|node| node.id == "person:father"));
        }

        #[test]
        fn test_social_graph_collapses_split_edges_after_person_merge() {
            let profiles = vec![
                test_profile("partner", "Test Partner", "partner"),
                test_profile("sue", "Sue", "mother_in_law"),
            ];
            let relationships = vec![
                test_relationship(
                    "partner",
                    "Test Partner",
                    "mother_in_law",
                    "Sue",
                    "feels_unsupported_by",
                    2,
                ),
                test_relationship(
                    "test_partner",
                    "Test Partner",
                    "sue",
                    "Sue",
                    "feels_unsupported_by",
                    3,
                ),
            ];
            let graph = build_social_graph(
                "test-user".to_string(),
                &profiles,
                &PatientGraph::default(),
                &relationships,
                &[],
                &[],
            );

            let edge = graph
                .edges
                .iter()
                .find(|edge| {
                    edge.from == "person:partner"
                        && edge.to == "person:sue"
                        && edge.relation == "feels_unsupported_by"
                })
                .expect("merged third-party edge should exist");
            assert_eq!(edge.weight, 5);
            assert_eq!(
                graph
                    .edges
                    .iter()
                    .filter(|edge| edge.relation == "feels_unsupported_by")
                    .count(),
                1
            );
        }

        #[test]
        fn test_social_graph_rejects_binary_choice_as_person() {
            let relationships = vec![test_relationship(
                "self",
                "You",
                "binary_choice",
                "choice",
                "faces",
                1,
            )];
            let graph = build_social_graph(
                "test-user".to_string(),
                &[],
                &PatientGraph::default(),
                &relationships,
                &[],
                &[],
            );

            assert!(!graph
                .nodes
                .iter()
                .any(|node| node.id == "person:binary_choice"));
            assert!(!graph.edges.iter().any(|edge| edge.relation == "faces"));
        }

        #[test]
        fn test_social_graph_drops_self_loop_created_by_merge() {
            let profiles = vec![test_profile("partner", "Test Partner", "partner")];
            let relationships = vec![test_relationship(
                "test_partner",
                "Test Partner",
                "partner",
                "Test Partner",
                "is_same_person_as",
                1,
            )];
            let graph = build_social_graph(
                "test-user".to_string(),
                &profiles,
                &PatientGraph::default(),
                &relationships,
                &[],
                &[],
            );

            assert!(!graph
                .edges
                .iter()
                .any(|edge| edge.relation == "is_same_person_as"));
        }

        #[test]
        fn test_social_graph_constellates_linked_concept_and_episode() {
            let profiles = vec![test_profile("mother", "Mother", "mother")];
            let mut nodes = vec![GraphNode {
                id: "fear_of_rejection".to_string(),
                label: "Fear of rejection".to_string(),
                category: "Pattern".to_string(),
            }];
            for index in 0..12 {
                nodes.push(GraphNode {
                    id: format!("goal_{index}"),
                    label: format!("Goal {index}"),
                    category: "Goal".to_string(),
                });
            }
            let patient_graph = PatientGraph {
                user_id: "test-user".to_string(),
                nodes,
                edges: Vec::new(),
            };
            let episodes = vec![Episode {
                user_id: "test-user".to_string(),
                id: "test_call".to_string(),
                title: "Test phone call".to_string(),
                narrative: "A test participant reported a disagreement the user during a Test phone call.".to_string(),
                occurred_at: Some("Test date".to_string()),
                session_id: None,
                user_quotes: Vec::new(),
                created_at: Some("2026-01-01".to_string()),
                updated_at: Some("2026-01-02".to_string()),
            }];
            let links = vec![
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "concept".to_string(),
                    from_id: "fear_of_rejection".to_string(),
                    relation: "originates_from".to_string(),
                    to_kind: "person".to_string(),
                    to_id: "mother".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "episode".to_string(),
                    from_id: "test_call".to_string(),
                    relation: "involves".to_string(),
                    to_kind: "person".to_string(),
                    to_id: "mother".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "episode".to_string(),
                    from_id: "test_call".to_string(),
                    relation: "evidences".to_string(),
                    to_kind: "concept".to_string(),
                    to_id: "fear_of_rejection".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
            ];

            let graph = build_social_graph(
                "test-user".to_string(),
                &profiles,
                &patient_graph,
                &[],
                &links,
                &episodes,
            );

            assert!(graph.edges.iter().any(|edge| {
                edge.from == "pattern:fear_of_rejection"
                    && edge.to == "person:mother"
                    && edge.relation == "originates_from"
            }));
            assert!(!graph.edges.iter().any(|edge| {
                edge.from == "self"
                    && edge.to == "pattern:fear_of_rejection"
                    && edge.relation == "pattern"
            }));
            assert!(graph
                .nodes
                .iter()
                .any(|node| node.id == "episode:test_call" && node.kind == "episode"));
            assert!(graph.edges.iter().any(|edge| {
                edge.from == "episode:test_call"
                    && edge.to == "person:mother"
                    && edge.relation == "involves"
            }));
            let concept_count = graph
                .nodes
                .iter()
                .filter(|node| !matches!(node.kind.as_str(), "self" | "person" | "episode"))
                .count();
            assert!(concept_count <= 10);
        }

        #[test]
        fn test_graph_walk_recall_surfaces_linked_concept_and_episode() {
            let profiles = vec![test_profile("mother", "Mother", "mother")];
            let graph = PatientGraph {
                user_id: "test-user".to_string(),
                nodes: vec![GraphNode {
                    id: "fear_of_rejection".to_string(),
                    label: "Fear of rejection".to_string(),
                    category: "Pattern".to_string(),
                }],
                edges: Vec::new(),
            };
            let episode = Episode {
                user_id: "test-user".to_string(),
                id: "test_call".to_string(),
                title: "Test phone call".to_string(),
                narrative:
                    "A test participant reported a disagreement the user during a Test phone call. The user felt small."
                        .to_string(),
                occurred_at: Some("Test date".to_string()),
                session_id: None,
                user_quotes: Vec::new(),
                created_at: Some("2026-01-01".to_string()),
                updated_at: Some("2026-01-02".to_string()),
            };
            let links = vec![
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "concept".to_string(),
                    from_id: "fear_of_rejection".to_string(),
                    relation: "originates_from".to_string(),
                    to_kind: "person".to_string(),
                    to_id: "mother".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "episode".to_string(),
                    from_id: "test_call".to_string(),
                    relation: "involves".to_string(),
                    to_kind: "person".to_string(),
                    to_id: "mother".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
            ];

            let (concept_scores, _, episode_scores) = graph_walk_memory_scores(
                "What do you remember about Mother?",
                &graph,
                &profiles,
                &links,
            );
            assert!(
                concept_scores
                    .get("fear_of_rejection")
                    .copied()
                    .unwrap_or_default()
                    > 0
            );
            let episodes = vec![episode];
            let episode_hits = relevant_episode_lines(&episodes, &episode_scores, 4);
            let block = format_persistent_memory_block(
                &[],
                &["Fear of rejection [Pattern]".to_string()],
                &[],
                &episode_hits,
            );
            assert!(block.contains("Relevant episodes:"));
            assert!(block.contains("Test phone call (Test date)"));
        }

        #[test]
        fn test_mind_map_payload_adds_people_episodes_and_cross_edges() {
            let profiles = vec![test_profile("mother", "Mother", "mother")];
            let graph = PatientGraph {
                user_id: "test-user".to_string(),
                nodes: vec![GraphNode {
                    id: "fear_of_rejection".to_string(),
                    label: "Fear of rejection".to_string(),
                    category: "Pattern".to_string(),
                }],
                edges: Vec::new(),
            };
            let episodes = vec![Episode {
                user_id: "test-user".to_string(),
                id: "test_call".to_string(),
                title: "Test phone call".to_string(),
                narrative: "A test participant reported a disagreement the user during a Test phone call.".to_string(),
                occurred_at: Some("Test date".to_string()),
                session_id: None,
                user_quotes: Vec::new(),
                created_at: None,
                updated_at: None,
            }];
            let links = vec![
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "concept".to_string(),
                    from_id: "fear_of_rejection".to_string(),
                    relation: "originates_from".to_string(),
                    to_kind: "person".to_string(),
                    to_id: "mother".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
                MemoryLink {
                    user_id: "test-user".to_string(),
                    from_kind: "episode".to_string(),
                    from_id: "test_call".to_string(),
                    relation: "evidences".to_string(),
                    to_kind: "concept".to_string(),
                    to_id: "fear_of_rejection".to_string(),
                    evidence: String::new(),
                    weight: 1,
                    created_at: None,
                    updated_at: None,
                },
            ];

            let payload = build_mind_map_payload(&graph, &profiles, &episodes, &links);
            assert_eq!(payload["people"][0]["slug"], "mother");
            assert_eq!(payload["episodes"][0]["id"], "test_call");
            assert!(payload["cross_edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| {
                    edge["kind"] == "concept_person"
                        && edge["from"] == "fear_of_rejection"
                        && edge["to"] == "mother"
                }));
            assert!(payload["cross_edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| {
                    edge["kind"] == "episode_concept"
                        && edge["from"] == "episode:test_call"
                        && edge["to"] == "fear_of_rejection"
                }));
        }

        #[test]
        fn test_current_datetime_shape() {
            let now = current_datetime();
            assert!(now.local_datetime.len() >= 19);
            assert!(now.utc_datetime.ends_with(" UTC"));
            assert_eq!(now.utc_offset.len(), 6);
            assert!(now.utc_offset.starts_with('+') || now.utc_offset.starts_with('-'));
            assert!(now.unix_timestamp > 0);
        }

        #[test]
        fn test_password_reset_token_generation() {
            let token = Uuid::new_v4().to_string();
            assert_eq!(token.len(), 36);
            assert!(token.contains('-'));
        }
    }
}

pub use runtime::{agent_runtime, cookie_key, draft_stream_handler, graph_handler, stream_handler};

pub fn has_auth_cookie(
    headers: &axum::http::HeaderMap,
    key: &axum_extra::extract::cookie::Key,
) -> bool {
    use axum_extra::extract::cookie::PrivateCookieJar;

    let jar = PrivateCookieJar::from_headers(headers, key.clone());
    jar.get(AUTH_COOKIE_NAME).is_some()
}

pub fn cookie_is_secure(headers: &axum::http::HeaderMap) -> bool {
    runtime::cookie_is_secure(headers)
}

pub fn extract_user_id_from_headers(
    headers: &axum::http::HeaderMap,
    key: &axum_extra::extract::cookie::Key,
) -> Result<String, anyhow::Error> {
    use axum_extra::extract::cookie::PrivateCookieJar;

    let jar = PrivateCookieJar::from_headers(headers, key.clone());
    jar.get(AUTH_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or_else(|| anyhow::anyhow!("Unauthorized"))
}
