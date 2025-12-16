use leptos::*;

#[cfg(feature = "ssr")]
mod runtime {
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
        histories: RwLock<HashMap<String, Vec<Message>>>,
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
            })
        }

        pub async fn respond(&self, session_id: &str, prompt: String) -> Result<String> {
            let mut history = {
                let mut guard = self.histories.write().await;
                guard.remove(session_id).unwrap_or_default()
            };
            println!(
                "[agent] session={} prompt_len={} history_len={}",
                session_id,
                prompt.len(),
                history.len()
            );

            let reply = self
                .agent
                .prompt(Message::user(prompt))
                .with_history(&mut history)
                .multi_turn(2)
                .await
                .context("Running agent prompt")?;

            let mut guard = self.histories.write().await;
            guard.insert(session_id.to_string(), history);

            Ok(reply)
        }

        pub async fn stream(
            self: &Arc<Self>,
            session_id: String,
            prompt: String,
        ) -> Result<ReceiverStream<Result<String, std::convert::Infallible>>> {
            let mut history = {
                let mut guard = self.histories.write().await;
                guard.remove(&session_id).unwrap_or_default()
            };
            println!(
                "[agent-stream] session={} prompt_len={} history_len={}",
                session_id,
                prompt.len(),
                history.len()
            );

            let mut stream = self
                .agent
                .stream_prompt(Message::user(prompt.clone()))
                .with_history(history.clone())
                .multi_turn(2)
                .await;

            let (tx, rx) = mpsc::channel(16);
            let runtime = Arc::clone(self);
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
                            eprintln!("[agent-stream:delta] {}", delta.text);
                            assembled.push_str(&delta.text);
                            let _ = tx.send(Ok(delta.text)).await;
                        }
                        Some(Ok(rig::agent::MultiTurnStreamItem::FinalResponse(resp))) => {
                            eprintln!("[agent-stream:final]");
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
                if assembled.is_empty() {
                    if let Some(text) = final_text.clone() {
                        if !text.is_empty() {
                            let _ = tx.send(Ok(text.clone())).await;
                            assembled = text;
                        }
                    } else {
                        eprintln!("[agent-stream:fallback-call]");
                        match runtime
                            .agent
                            .prompt(Message::user(prompt.clone()))
                            .with_history(&mut history)
                            .multi_turn(2)
                            .await
                        {
                            Ok(text) => {
                                assembled = text.clone();
                                let _ = tx.send(Ok(text)).await;
                            }
                            Err(e) => {
                                eprintln!("[agent-stream:fallback-error] {}", e);
                                let _ = tx.send(Ok(format!("[error:{}]", e))).await;
                            }
                        }
                    }
                }
                let _ = tx.send(Ok("[DONE]".to_string())).await;
                history.push(Message::user(prompt));
                if !assembled.is_empty() {
                    history.push(Message::Assistant {
                        id: None,
                        content: rig::OneOrMany::one(AssistantContent::Text(Text {
                            text: assembled,
                        })),
                    });
                }
                let mut guard = runtime.histories.write().await;
                guard.insert(session_id, history);
            });

            Ok(ReceiverStream::new(rx))
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
