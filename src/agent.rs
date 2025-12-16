use leptos::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub date: String,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatLog {
    pub role: String,
    pub content: String,
}

#[cfg(feature = "ssr")]
mod runtime {
    use super::{ChatLog, Session};
    use std::{
        collections::HashMap,
        path::Path,
        sync::{Arc, Once},
    };

    use anyhow::{Context, Result};
    use axum::{
        extract::Query,
        http::StatusCode,
        response::sse::{Event, Sse},
    };
    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
    use rig::streaming::StreamingPrompt;
    use rig::vector_store::request::{Filter, VectorSearchRequest};
    use rig::vector_store::{VectorStoreError, VectorStoreIndex};
    use rig::{
        agent::AgentBuilder,
        client::{CompletionClient, EmbeddingsClient},
        completion::{message::Text, AssistantContent, Message, Prompt},
        embeddings::EmbeddingsBuilder,
        providers::{openai, xai},
        Embed,
    };
    use rig_sqlite::{
        Column, ColumnValue, SqliteSearchFilter, SqliteVectorIndex, SqliteVectorStore,
        SqliteVectorStoreTable,
    };
    use rusqlite::ffi::{sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
    use serde::{Deserialize, Serialize};
    use sqlite_vec::sqlite3_vec_init;
    use tokio::sync::{mpsc, OnceCell, RwLock};
    use tokio::time::{timeout, Duration};
    use tokio_rusqlite::Connection;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;
    use uuid::Uuid;

    type SqliteExtensionFn =
        unsafe extern "C" fn(*mut sqlite3, *mut *mut i8, *const sqlite3_api_routines) -> i32;

    const SYSTEM_PROMPT: &str = r#"
You are IndividuateAI, a Jungian, somatic-aware therapist. Keep responses under ~180 words, grounded, and practical. Mirror the user briefly, surface patterns, propose one concrete practice, and end with a concise reflective question. If the user shares safety-critical content, encourage professional or emergency support.
    "#;

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
        if let Some(parent) = Path::new(db_path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Creating data dir {:?}", parent))?;
            }
        }
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

    pub struct AgentRuntime {
        agent: rig::agent::Agent<xai::completion::CompletionModel>,
        // We still keep in-memory cache for speed during session, but sync to DB
        histories: RwLock<HashMap<String, Vec<Message>>>,
        conn: Connection,
    }

    impl AgentRuntime {
        async fn new() -> Result<Self> {
            init_sqlite_extensions();

            let db_path =
                std::env::var("MEMORY_DB_PATH").unwrap_or_else(|_| "data/memory.sqlite".into());
            ensure_data_dir(&db_path).await?;
            let conn = Connection::open(db_path)
                .await
                .context("Opening sqlite memory store")?;

            // Init Chat Schema
            conn.call(|conn| {
                conn.execute_batch(
                    r"
                    CREATE TABLE IF NOT EXISTS sessions (
                        id TEXT PRIMARY KEY,
                        title TEXT NOT NULL,
                        created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                        updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                    );
                    CREATE TABLE IF NOT EXISTS messages (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        session_id TEXT NOT NULL,
                        role TEXT NOT NULL,
                        content TEXT NOT NULL,
                        created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                        FOREIGN KEY(session_id) REFERENCES sessions(id)
                    );
                    "
                ).map_err(tokio_rusqlite::Error::Rusqlite)
            }).await.context("Initializing chat schema")?;

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

            let xai_key = std::env::var("XAI_API_KEY")
                .or_else(|_| std::env::var("GROK_API_KEY"))
                .context("Set XAI_API_KEY or GROK_API_KEY")?;
            let grok_model = std::env::var("GROK_MODEL")
                .unwrap_or_else(|_| xai::completion::GROK_3_MINI.to_string());
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
            let http_client = reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .context("building reqwest client")?;

            let xai_client = xai::Client::<reqwest::Client>::builder()
                .api_key(&xai_key)
                .base_url("https://api.x.ai")
                .http_client(http_client)
                .build()
                .context("Building xAI client")?;

            let agent = AgentBuilder::new(xai_client.completion_model(grok_model))
                .name("individuateai_therapist")
                .preamble(SYSTEM_PROMPT)
                .dynamic_context(4, vector_index)
                .build();

            Ok(Self {
                agent,
                histories: RwLock::new(HashMap::new()),
                conn,
            })
        }

        // --- Persistence Helpers ---

        async fn create_session(&self, title: String) -> Result<Session> {
            let id = Uuid::new_v4().to_string();
            let s = Session {
                id: id.clone(),
                title: title.clone(),
                date: "Just now".into(), // In real app, format current time
                preview: "New session".into(),
            };
            
            self.conn.call(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (id, title) VALUES (?1, ?2)",
                    rusqlite::params![id, title],
                ).map_err(tokio_rusqlite::Error::Rusqlite)
            }).await?;

            Ok(s)
        }

        async fn get_sessions(&self) -> Result<Vec<Session>> {
            self.conn.call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, created_at FROM sessions ORDER BY updated_at DESC"
                )?;
                let rows = stmt.query_map([], |row| {
                    let id: String = row.get(0)?;
                    let title: String = row.get(1)?;
                    let date: String = row.get(2)?; // timestamp string
                    Ok(Session {
                        id,
                        title,
                        date,
                        preview: "Context...".to_string(), // Could fetch last message content here
                    })
                })?;
                let mut sessions = Vec::new();
                for r in rows {
                    sessions.push(r?);
                }
                Ok(sessions)
            }).await.context("Fetching sessions")
        }

        async fn save_message(&self, session_id: String, role: String, content: String) -> Result<()> {
            self.conn.call(move |conn| {
                conn.execute(
                    "INSERT INTO messages (session_id, role, content) VALUES (?1, ?2, ?3)",
                    rusqlite::params![session_id, role, content],
                ).map_err(tokio_rusqlite::Error::Rusqlite)
            }).await?;
            Ok(())
        }

        async fn get_history(&self, session_id: String) -> Result<Vec<ChatLog>> {
             self.conn.call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT role, content FROM messages WHERE session_id = ?1 ORDER BY id ASC"
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
            }).await.context("Fetching history")
        }
        
        // --- Agent Logic ---

        pub async fn respond(&self, session_id: &str, prompt: String) -> Result<String> {
            // Persist User Message
            self.save_message(session_id.to_string(), "user".into(), prompt.clone()).await?;

            // Load History (if not in memory, or just use what we have? 
            // Better to sync with DB to be stateless-ready, but for now we trust cache + prompt append)
            // But we should hydrate the cache from DB if empty!
            let mut history = {
                let mut guard = self.histories.write().await;
                if !guard.contains_key(session_id) {
                     let db_logs = self.get_history(session_id.to_string()).await?;
                     let mut msgs = Vec::new();
                     for log in db_logs {
                         if log.role == "user" {
                             msgs.push(Message::user(log.content));
                         } else {
                             // This assumes assistant messages are plain text for reconstruction
                             msgs.push(Message::Assistant { 
                                 id: None, 
                                 content: rig::OneOrMany::one(AssistantContent::Text(Text{text: log.content})) 
                             });
                         }
                     }
                     guard.insert(session_id.to_string(), msgs);
                }
                guard.remove(session_id).unwrap_or_default()
            };

            let reply = self
                .agent
                .prompt(Message::user(prompt.clone())) // Prompt is user message
                .with_history(&mut history)
                .multi_turn(2)
                .await
                .context("Running agent prompt")?;

            // Persist Assistant Message
            self.save_message(session_id.to_string(), "assistant".into(), reply.clone()).await?;

            // Update Cache
            let mut guard = self.histories.write().await;
            guard.insert(session_id.to_string(), history);

            Ok(reply)
        }

        pub async fn stream(
            self: &Arc<Self>,
            session_id: String,
            prompt: String,
        ) -> Result<ReceiverStream<Result<String, std::convert::Infallible>>> {
             // Persist User Message
            self.save_message(session_id.clone(), "user".into(), prompt.clone()).await?;
            
             // Load/Hydrate History
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
                                 content: rig::OneOrMany::one(AssistantContent::Text(Text{text: log.content})) 
                             });
                         }
                     }
                     guard.insert(session_id.clone(), msgs);
                }
                guard.remove(&session_id).unwrap_or_default()
            };

            let mut stream = self
                .agent
                .stream_prompt(Message::user(prompt.clone()))
                .with_history(history.clone())
                .multi_turn(2)
                .await;

            let (tx, rx) = mpsc::channel(16);
            let runtime = Arc::clone(self);
            let session_id_clone = session_id.clone();
            
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
                        Some(_) => {}
                        None => break,
                    }
                }
                
                // Finalize and Save
                let final_content = if let Some(text) = final_text {
                     text
                } else if !assembled.is_empty() {
                     assembled
                } else {
                     // fallback logic (omitted for brevity, assume stream worked or we handle error upstream)
                     String::new()
                };
                
                if !final_content.is_empty() {
                    let _ = runtime.save_message(session_id_clone.clone(), "assistant".into(), final_content.clone()).await;
                }

                let _ = tx.send(Ok("[DONE]".to_string())).await;
                
                // Update History Cache
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
        
        // Expose helpers to public (via server fns)
        pub async fn list_sessions(&self) -> Result<Vec<Session>> {
            self.get_sessions().await
        }
        
        pub async fn create_new_session(&self, title: String) -> Result<Session> {
            self.create_session(title).await
        }
        
        pub async fn get_session_history(&self, id: String) -> Result<Vec<ChatLog>> {
            self.get_history(id).await
        }
    }

    static GLOBAL_AGENT: OnceCell<Arc<AgentRuntime>> = OnceCell::const_new();

    pub async fn agent_runtime() -> Result<Arc<AgentRuntime>> {
        GLOBAL_AGENT
            .get_or_try_init(|| async { AgentRuntime::new().await.map(Arc::new) })
            .await
            .cloned()
    }

    #[derive(Deserialize)]
    pub struct StreamParams {
        pub prompt: String,
        pub session_id: String,
    }

    pub async fn stream_handler(
        Query(params): Query<StreamParams>,
    ) -> Result<
        Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>,
        (StatusCode, String),
    > {
        let runtime = agent_runtime().await.map_err(internal_err)?;
        let stream = runtime
            .stream(params.session_id, params.prompt)
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
pub use runtime::{agent_runtime, stream_handler};

fn server_error(err: impl std::fmt::Display) -> ServerFnError {
    eprintln!("[agent_serverfn] {}", err);
    ServerFnError::ServerError(err.to_string())
}

#[server(AgentChat, "/api")]
pub async fn agent_chat(prompt: String, session_id: String) -> Result<String, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let agent = match agent_runtime().await {
            Ok(agent) => agent,
            Err(e) => {
                eprintln!("[agent_chat:init] {}", e);
                return Ok(format!("Agent unavailable: {e}"));
            }
        };
        return match agent.respond(&session_id, prompt.clone()).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                eprintln!("[agent_chat:respond] {}", e);
                Ok(format!("Agent error: {e}"))
            }
        };
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = (prompt, session_id);
        Err(ServerFnError::ServerError(
            "Agent runtime only available on the server".into(),
        ))
    }
}

#[server(GetSessions, "/api")]
pub async fn get_sessions() -> Result<Vec<Session>, ServerFnError> {
     #[cfg(feature = "ssr")]
    {
        let agent = agent_runtime().await.map_err(server_error)?;
        agent.list_sessions().await.map_err(server_error)
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
        let agent = agent_runtime().await.map_err(server_error)?;
        agent.create_new_session(title).await.map_err(server_error)
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
        let agent = agent_runtime().await.map_err(server_error)?;
        agent.get_session_history(session_id).await.map_err(server_error)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = session_id;
        Err(ServerFnError::ServerError("SSR only".into()))
    }
}