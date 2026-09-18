use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio_rusqlite::Connection;

use crate::billing::StripeSubscription;

pub const PRODUCT_COPY_VERSION: &str = "2026-09-18.v1";
pub const PRIVACY_NOTICE_VERSION: &str = "2026-09-18";
pub const PRODUCT_DESCRIPTION: &str = "IndividuateAI is a reflective AI service with persistent encrypted history, editable memory and relationship maps, and text/voice conversations. It can generate interpretations, possible connections, patterns, and insights from what a user shares.";
pub const PURCHASE_DISCLOSURE: &str = "IndividuateAI is a reflective AI service. It can generate interpretations, possible connections, patterns and insights from what you share. These outputs are hypotheses, not factual transcripts or clinical findings; they can be incomplete or mistaken and can be inspected, corrected or removed. Subscriptions renew until cancelled through the billing portal.";

const DEFAULT_RETENTION_DAYS: i64 = 550;

#[derive(Clone)]
pub struct EvidenceStore {
    conn: Connection,
    retention_days: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommercialSnapshot {
    pub checkout_session_id: String,
    pub plan_lookup_key: String,
    pub plan_display: String,
    pub product_copy_version: String,
    pub product_description: String,
    pub purchase_disclosure: String,
    pub disclosure_acknowledged: bool,
    pub privacy_notice_version: String,
    pub app_release_sha: Option<String>,
    pub marketing_release_sha: Option<String>,
    pub purchase_page_url: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceEvent {
    pub event_type: String,
    pub external_id: Option<String>,
    pub occurred_at: String,
    pub summary: String,
    pub details: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountEvidence {
    pub user_id: String,
    pub email: String,
    pub created_at: String,
    pub passkey_count: i64,
    pub first_passkey_created_at: Option<String>,
    pub last_passkey_used_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BillingEvidence {
    pub stripe_customer_id: String,
    pub stripe_subscription_id: String,
    pub status: String,
    pub price_id: String,
    pub current_period_end: Option<i64>,
    pub cancel_at_period_end: bool,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageEvidence {
    pub session_count: i64,
    pub user_message_count: i64,
    pub assistant_message_count: i64,
    pub first_message_at: Option<String>,
    pub last_message_at: Option<String>,
    pub metered_chat_responses: i64,
    pub voice_tokens: i64,
    pub tts_characters: i64,
    pub episodes_count: i64,
    pub relationship_profiles_count: i64,
    pub social_relationships_count: i64,
    pub memory_links_count: i64,
    pub core_patterns_count: i64,
    pub mind_map_present: bool,
    pub mind_map_updated_at: Option<String>,
    pub social_graph_present: bool,
    pub social_graph_updated_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveSubscriptionEvidence {
    pub subscription_id: String,
    pub customer_id: String,
    pub status: String,
    pub price_id: String,
    pub created_at: Option<String>,
    pub current_period_end: Option<String>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<String>,
    pub cancellation_comment: Option<String>,
    pub cancellation_feedback: Option<String>,
    pub cancellation_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisputeEvidencePack {
    pub generated_at: String,
    pub account: AccountEvidence,
    pub billing: Option<BillingEvidence>,
    pub usage: UsageEvidence,
    pub commercial_snapshots: Vec<CommercialSnapshot>,
    pub events: Vec<EvidenceEvent>,
    pub live_subscription: Option<LiveSubscriptionEvidence>,
    pub evidence_window_start: Option<String>,
    pub retention_days: i64,
    pub privacy_note: String,
}

impl DisputeEvidencePack {
    pub fn set_live_subscription(&mut self, subscription: StripeSubscription) {
        self.live_subscription = Some(LiveSubscriptionEvidence {
            subscription_id: subscription.id,
            customer_id: subscription.customer_id,
            status: subscription.status,
            price_id: subscription.price_id,
            created_at: format_unix(subscription.created_at),
            current_period_end: format_unix(subscription.current_period_end),
            cancel_at_period_end: subscription.cancel_at_period_end,
            canceled_at: format_unix(subscription.canceled_at),
            cancellation_comment: subscription.cancellation_comment,
            cancellation_feedback: subscription.cancellation_feedback,
            cancellation_reason: subscription.cancellation_reason,
        });
    }
}

impl EvidenceStore {
    pub async fn open_from_env() -> Result<Self> {
        let path = std::env::var("EVIDENCE_DB_PATH")
            .unwrap_or_else(|_| "data/dispute_evidence.sqlite".to_string());
        if let Some(parent) = Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Creating evidence directory {}", parent.display()))?;
            }
        }

        let conn = Connection::open(&path)
            .await
            .with_context(|| format!("Opening dispute evidence store at {path}"))?;

        if let Some(key) = evidence_db_key() {
            conn.call(move |conn| {
                conn.pragma_update(None, "key", &key)
                    .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .context("Applying SQLCipher key to dispute evidence store")?;
        } else {
            tracing::warn!(
                "No EVIDENCE_DB_KEY or MEMORY_DB_KEY is set; dispute evidence store at {path} is unencrypted"
            );
        }

        conn.call(|conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS commercial_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL,
                    checkout_session_id TEXT NOT NULL UNIQUE,
                    plan_lookup_key TEXT NOT NULL,
                    plan_display TEXT NOT NULL,
                    product_copy_version TEXT NOT NULL,
                    product_description TEXT NOT NULL,
                    purchase_disclosure TEXT NOT NULL,
                    disclosure_acknowledged INTEGER NOT NULL DEFAULT 0,
                    privacy_notice_version TEXT NOT NULL,
                    app_release_sha TEXT,
                    marketing_release_sha TEXT,
                    purchase_page_url TEXT,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS idx_commercial_snapshots_user
                    ON commercial_snapshots(user_id, created_at);

                CREATE TABLE IF NOT EXISTS evidence_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    external_id TEXT,
                    details_json TEXT NOT NULL DEFAULT '{}',
                    occurred_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    UNIQUE(event_type, external_id)
                );
                CREATE INDEX IF NOT EXISTS idx_evidence_events_user
                    ON evidence_events(user_id, occurred_at);
                "#,
            )
            .map_err(tokio_rusqlite::Error::Rusqlite)
        })
        .await
        .context("Creating dispute evidence schema")?;

        let store = Self {
            conn,
            retention_days: retention_days(),
        };
        store.purge_expired().await?;
        Ok(store)
    }

    pub async fn record_checkout_snapshot(
        &self,
        user_id: &str,
        checkout_session_id: &str,
        plan_lookup_key: &str,
        plan_display: &str,
        disclosure_acknowledged: bool,
    ) -> Result<()> {
        let user_id = user_id.to_string();
        let checkout_session_id = checkout_session_id.to_string();
        let plan_lookup_key = plan_lookup_key.to_string();
        let plan_display = plan_display.to_string();
        let app_release_sha = env_nonempty("APP_RELEASE_SHA");
        let marketing_release_sha = env_nonempty("MARKETING_RELEASE_SHA");
        let purchase_page_url = env_nonempty("APP_BASE_URL")
            .map(|base| format!("{}/subscribe", base.trim_end_matches('/')));

        let insert_user_id = user_id.clone();
        let insert_checkout_session_id = checkout_session_id.clone();
        let insert_plan_lookup_key = plan_lookup_key.clone();
        let insert_plan_display = plan_display.clone();
        self.conn
            .call(move |conn| {
                conn.execute(
                    r#"
                    INSERT INTO commercial_snapshots (
                        user_id, checkout_session_id, plan_lookup_key, plan_display,
                        product_copy_version, product_description, purchase_disclosure,
                        disclosure_acknowledged, privacy_notice_version, app_release_sha,
                        marketing_release_sha, purchase_page_url
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    ON CONFLICT(checkout_session_id) DO UPDATE SET
                        disclosure_acknowledged = excluded.disclosure_acknowledged
                    "#,
                    params![
                        insert_user_id,
                        insert_checkout_session_id,
                        insert_plan_lookup_key,
                        insert_plan_display,
                        PRODUCT_COPY_VERSION,
                        PRODUCT_DESCRIPTION,
                        PURCHASE_DISCLOSURE,
                        if disclosure_acknowledged { 1 } else { 0 },
                        PRIVACY_NOTICE_VERSION,
                        app_release_sha,
                        marketing_release_sha,
                        purchase_page_url
                    ],
                )
                .map(|_| ())
                .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .context("Recording commercial snapshot")?;

        self.record_event(
            &user_id,
            "checkout.created",
            Some(&checkout_session_id),
            json!({
                "plan_lookup_key": plan_lookup_key,
                "plan_display": plan_display,
                "product_copy_version": PRODUCT_COPY_VERSION,
                "privacy_notice_version": PRIVACY_NOTICE_VERSION,
                "disclosure_acknowledged": disclosure_acknowledged,
            }),
        )
        .await
    }

    pub async fn record_event(
        &self,
        user_id: &str,
        event_type: &str,
        external_id: Option<&str>,
        details: Value,
    ) -> Result<()> {
        let user_id = user_id.to_string();
        let event_type = event_type.to_string();
        let external_id = external_id.map(str::to_string);
        let details_json = serde_json::to_string(&details).context("Serializing evidence event")?;
        self.conn
            .call(move |conn| {
                conn.execute(
                    r#"
                    INSERT INTO evidence_events (user_id, event_type, external_id, details_json)
                    VALUES (?1, ?2, ?3, ?4)
                    ON CONFLICT(event_type, external_id) DO NOTHING
                    "#,
                    params![user_id, event_type, external_id, details_json],
                )
                .map(|_| ())
                .map_err(tokio_rusqlite::Error::Rusqlite)
            })
            .await
            .context("Recording evidence event")
    }

    pub async fn record_stripe_event(
        &self,
        user_id: &str,
        stripe_event_id: &str,
        stripe_event_type: &str,
        object: &Value,
    ) -> Result<()> {
        let details = stripe_event_details(stripe_event_type, object);
        self.record_event(user_id, stripe_event_type, Some(stripe_event_id), details)
            .await
    }

    pub async fn build_pack(&self, user_id: &str) -> Result<DisputeEvidencePack> {
        let snapshots = self.snapshots_for_user(user_id).await?;
        let events = self.events_for_user(user_id).await?;
        let evidence_window_start = snapshots.first().map(|snapshot| snapshot.created_at.clone());
        let user_id_owned = user_id.to_string();
        let since = evidence_window_start.clone();

        let (account, billing, usage) = tokio::task::spawn_blocking(move || {
            read_application_metadata(&user_id_owned, since.as_deref())
        })
        .await
        .context("Joining dispute evidence metadata query")??;

        Ok(DisputeEvidencePack {
            generated_at: now_utc(),
            account,
            billing,
            usage,
            commercial_snapshots: snapshots,
            events,
            live_subscription: None,
            evidence_window_start,
            retention_days: self.retention_days,
            privacy_note: "This pack contains account, transaction and service-use metadata only. It does not read or include private conversation contents.".to_string(),
        })
    }

    async fn snapshots_for_user(&self, user_id: &str) -> Result<Vec<CommercialSnapshot>> {
        let user_id = user_id.to_string();
        self.conn
            .call(move |conn| {
                let mut statement = conn
                    .prepare(
                        r#"
                        SELECT checkout_session_id, plan_lookup_key, plan_display,
                               product_copy_version, product_description, purchase_disclosure,
                               disclosure_acknowledged, privacy_notice_version, app_release_sha,
                               marketing_release_sha, purchase_page_url, created_at
                        FROM commercial_snapshots
                        WHERE user_id = ?1
                        ORDER BY created_at ASC, id ASC
                        "#,
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
                let rows = statement
                    .query_map([user_id], |row| {
                        Ok(CommercialSnapshot {
                            checkout_session_id: row.get(0)?,
                            plan_lookup_key: row.get(1)?,
                            plan_display: row.get(2)?,
                            product_copy_version: row.get(3)?,
                            product_description: row.get(4)?,
                            purchase_disclosure: row.get(5)?,
                            disclosure_acknowledged: row.get::<_, i64>(6)? != 0,
                            privacy_notice_version: row.get(7)?,
                            app_release_sha: row.get(8)?,
                            marketing_release_sha: row.get(9)?,
                            purchase_page_url: row.get(10)?,
                            created_at: row.get(11)?,
                        })
                    })
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
                let mut output = Vec::new();
                for row in rows {
                    output.push(row.map_err(tokio_rusqlite::Error::Rusqlite)?);
                }
                Ok(output)
            })
            .await
            .context("Loading commercial snapshots")
    }

    async fn events_for_user(&self, user_id: &str) -> Result<Vec<EvidenceEvent>> {
        let user_id = user_id.to_string();
        self.conn
            .call(move |conn| {
                let mut statement = conn
                    .prepare(
                        r#"
                        SELECT event_type, external_id, occurred_at, details_json
                        FROM evidence_events
                        WHERE user_id = ?1
                        ORDER BY occurred_at ASC, id ASC
                        "#,
                    )
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
                let rows = statement
                    .query_map([user_id], |row| {
                        let event_type: String = row.get(0)?;
                        let details_json: String = row.get(3)?;
                        let details: Value =
                            serde_json::from_str(&details_json).unwrap_or_else(|_| json!({}));
                        Ok(EvidenceEvent {
                            summary: summarize_event(&event_type, &details),
                            event_type,
                            external_id: row.get(1)?,
                            occurred_at: row.get(2)?,
                            details,
                        })
                    })
                    .map_err(tokio_rusqlite::Error::Rusqlite)?;
                let mut output = Vec::new();
                for row in rows {
                    output.push(row.map_err(tokio_rusqlite::Error::Rusqlite)?);
                }
                Ok(output)
            })
            .await
            .context("Loading evidence events")
    }

    async fn purge_expired(&self) -> Result<()> {
        let modifier = format!("-{} days", self.retention_days);
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM evidence_events WHERE occurred_at < datetime('now', ?1)",
                    [modifier.as_str()],
                )
                .map_err(tokio_rusqlite::Error::Rusqlite)?;
                conn.execute(
                    "DELETE FROM commercial_snapshots WHERE created_at < datetime('now', ?1)",
                    [modifier.as_str()],
                )
                .map_err(tokio_rusqlite::Error::Rusqlite)?;
                Ok(())
            })
            .await
            .context("Purging expired dispute evidence")
    }
}

fn read_application_metadata(
    user_id: &str,
    since: Option<&str>,
) -> Result<(AccountEvidence, Option<BillingEvidence>, UsageEvidence)> {
    let path = std::env::var("MEMORY_DB_PATH").unwrap_or_else(|_| "data/memory.sqlite".to_string());
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("Opening memory database at {path} for metadata-only evidence"))?;

    if let Some(key) = env_nonempty("MEMORY_DB_KEY") {
        conn.pragma_update(None, "key", key)
            .context("Applying SQLCipher key for metadata-only evidence read")?;
    }
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))
        .context("Validating memory database for evidence read")?;

    let (email, created_at): (String, String) = conn
        .query_row(
            "SELECT username, created_at FROM users WHERE id = ?1",
            [user_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .context("Loading account metadata for dispute evidence")?;

    let (passkey_count, first_passkey_created_at, last_passkey_used_at):
        (i64, Option<String>, Option<String>) = conn.query_row(
        "SELECT COUNT(*), MIN(created_at), MAX(last_used_at) FROM passkeys WHERE user_id = ?1",
        [user_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    let billing = conn
        .query_row(
            r#"
            SELECT stripe_customer_id, stripe_subscription_id, status, price_id,
                   current_period_end, cancel_at_period_end, updated_at
            FROM billing_accounts
            WHERE user_id = ?1
            "#,
            [user_id],
            |row| {
                Ok(BillingEvidence {
                    stripe_customer_id: row.get(0)?,
                    stripe_subscription_id: row.get(1)?,
                    status: row.get(2)?,
                    price_id: row.get(3)?,
                    current_period_end: row.get(4)?,
                    cancel_at_period_end: row.get::<_, i64>(5)? != 0,
                    updated_at: row.get(6)?,
                })
            },
        )
        .optional()?;

    let session_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE user_id = ?1 AND (?2 IS NULL OR created_at >= ?2)",
        params![user_id, since],
        |row| row.get(0),
    )?;

    let (user_message_count, assistant_message_count, first_message_at, last_message_at):
        (i64, i64, Option<String>, Option<String>) = conn.query_row(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN m.role = 'user' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN m.role = 'assistant' THEN 1 ELSE 0 END), 0),
            MIN(m.created_at),
            MAX(m.created_at)
        FROM messages m
        JOIN sessions s ON s.id = m.session_id
        WHERE s.user_id = ?1 AND (?2 IS NULL OR m.created_at >= ?2)
        "#,
        params![user_id, since],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;

    let usage_period = since.and_then(|value| value.get(..7));
    let (metered_chat_responses, voice_tokens, tts_characters): (i64, i64, i64) =
        conn.query_row(
            r#"
            SELECT
                COALESCE(SUM(chat_responses), 0),
                COALESCE(SUM(voice_tokens), 0),
                COALESCE(SUM(tts_characters), 0)
            FROM monthly_usage
            WHERE user_id = ?1 AND (?2 IS NULL OR period >= ?2)
            "#,
            params![user_id, usage_period],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

    let usage = UsageEvidence {
        session_count,
        user_message_count,
        assistant_message_count,
        first_message_at,
        last_message_at,
        metered_chat_responses,
        voice_tokens,
        tts_characters,
        episodes_count: count_since(&conn, "episodes", user_id, "created_at", since)?,
        relationship_profiles_count: count_since(
            &conn,
            "relationship_profiles",
            user_id,
            "created_at",
            since,
        )?,
        social_relationships_count: count_since(
            &conn,
            "social_relationships",
            user_id,
            "created_at",
            since,
        )?,
        memory_links_count: count_since(&conn, "memory_links", user_id, "created_at", since)?,
        core_patterns_count: count_since(&conn, "core_patterns", user_id, "created_at", since)?,
        mind_map_present: row_timestamp(&conn, "patient_graphs", user_id)?.is_some(),
        mind_map_updated_at: row_timestamp(&conn, "patient_graphs", user_id)?,
        social_graph_present: row_timestamp(&conn, "social_graphs", user_id)?.is_some(),
        social_graph_updated_at: row_timestamp(&conn, "social_graphs", user_id)?,
    };

    Ok((
        AccountEvidence {
            user_id: user_id.to_string(),
            email,
            created_at,
            passkey_count,
            first_passkey_created_at,
            last_passkey_used_at,
        },
        billing,
        usage,
    ))
}

fn count_since(
    conn: &rusqlite::Connection,
    table: &str,
    user_id: &str,
    created_column: &str,
    since: Option<&str>,
) -> Result<i64> {
    let sql = format!(
        "SELECT COUNT(*) FROM {table} WHERE user_id = ?1 AND (?2 IS NULL OR {created_column} >= ?2)"
    );
    conn.query_row(&sql, params![user_id, since], |row| row.get(0))
        .with_context(|| format!("Counting {table} rows for dispute evidence"))
}

fn row_timestamp(
    conn: &rusqlite::Connection,
    table: &str,
    user_id: &str,
) -> Result<Option<String>> {
    let sql = format!("SELECT updated_at FROM {table} WHERE user_id = ?1");
    conn.query_row(&sql, [user_id], |row| row.get(0))
        .optional()
        .with_context(|| format!("Reading {table} state for dispute evidence"))
}

fn stripe_event_details(event_type: &str, object: &Value) -> Value {
    match event_type {
        "checkout.session.completed" => json!({
            "checkout_session_id": object.get("id").and_then(Value::as_str),
            "customer_id": expandable_id(object.get("customer")),
            "subscription_id": expandable_id(object.get("subscription")),
            "amount_total": object.get("amount_total").and_then(Value::as_i64),
            "currency": object.get("currency").and_then(Value::as_str),
            "payment_status": object.get("payment_status").and_then(Value::as_str),
            "status": object.get("status").and_then(Value::as_str),
        }),
        "customer.subscription.created"
        | "customer.subscription.updated"
        | "customer.subscription.deleted" => json!({
            "subscription_id": object.get("id").and_then(Value::as_str),
            "customer_id": expandable_id(object.get("customer")),
            "status": object.get("status").and_then(Value::as_str),
            "price_id": object.pointer("/items/data/0/price/id").and_then(Value::as_str)
                .or_else(|| object.pointer("/items/data/0/price").and_then(Value::as_str)),
            "cancel_at_period_end": object.get("cancel_at_period_end").and_then(Value::as_bool),
            "cancel_at": object.get("cancel_at").and_then(Value::as_i64),
            "canceled_at": object.get("canceled_at").and_then(Value::as_i64),
            "cancellation_comment": object.pointer("/cancellation_details/comment").and_then(Value::as_str),
            "cancellation_feedback": object.pointer("/cancellation_details/feedback").and_then(Value::as_str),
            "cancellation_reason": object.pointer("/cancellation_details/reason").and_then(Value::as_str),
        }),
        "invoice.payment_succeeded" | "invoice.payment_failed" => json!({
            "invoice_id": object.get("id").and_then(Value::as_str),
            "customer_id": expandable_id(object.get("customer")),
            "subscription_id": expandable_id(object.get("subscription"))
                .or_else(|| object.pointer("/parent/subscription_details/subscription").and_then(|value| expandable_id(Some(value)))),
            "payment_intent_id": expandable_id(object.get("payment_intent")),
            "charge_id": expandable_id(object.get("charge")),
            "amount_due": object.get("amount_due").and_then(Value::as_i64),
            "amount_paid": object.get("amount_paid").and_then(Value::as_i64),
            "currency": object.get("currency").and_then(Value::as_str),
            "status": object.get("status").and_then(Value::as_str),
        }),
        _ => json!({
            "stripe_object_id": object.get("id").and_then(Value::as_str),
        }),
    }
}

fn summarize_event(event_type: &str, details: &Value) -> String {
    match event_type {
        "checkout.created" => format!(
            "Checkout created for {}. Purchase disclosure acknowledged: {}.",
            details
                .get("plan_display")
                .and_then(Value::as_str)
                .unwrap_or("selected plan"),
            details
                .get("disclosure_acknowledged")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
        "checkout.session.completed" => format!(
            "Stripe Checkout completed{}{}.",
            details
                .get("amount_total")
                .and_then(Value::as_i64)
                .map(|amount| format!(" for {amount} minor currency units"))
                .unwrap_or_default(),
            details
                .get("currency")
                .and_then(Value::as_str)
                .map(|currency| format!(" {currency}"))
                .unwrap_or_default()
        ),
        "customer.subscription.created" => "Stripe subscription created.".to_string(),
        "customer.subscription.updated" | "customer.subscription.deleted" => {
            let comment = details
                .get("cancellation_comment")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            if let Some(comment) = comment {
                format!("Stripe subscription update. Customer cancellation comment: “{comment}”.")
            } else {
                format!(
                    "Stripe subscription update. Status: {}.",
                    details
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                )
            }
        }
        "invoice.payment_succeeded" => "Stripe recorded a successful invoice payment.".to_string(),
        "invoice.payment_failed" => "Stripe recorded a failed invoice payment attempt.".to_string(),
        "checkout.reconciled" => "Successful Checkout was reconciled to the application account.".to_string(),
        _ => event_type.replace('.', " "),
    }
}

fn expandable_id(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| {
        value
            .as_str()
            .map(str::to_string)
            .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_string))
    })
}

fn evidence_db_key() -> Option<String> {
    env_nonempty("EVIDENCE_DB_KEY").or_else(|| env_nonempty("MEMORY_DB_KEY"))
}

fn retention_days() -> i64 {
    std::env::var("DISPUTE_EVIDENCE_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| (30..=3650).contains(value))
        .unwrap_or(DEFAULT_RETENTION_DAYS)
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn now_utc() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

fn format_unix(value: Option<i64>) -> Option<String> {
    value
        .and_then(|timestamp| OffsetDateTime::from_unix_timestamp(timestamp).ok())
        .and_then(|value| value.format(&Rfc3339).ok())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_comment_is_summarized_without_conversation_content() {
        let details = json!({
            "status": "active",
            "cancellation_comment": "It embellishes too much"
        });
        let summary = summarize_event("customer.subscription.updated", &details);
        assert!(summary.contains("It embellishes too much"));
        assert!(!summary.contains("conversation"));
    }

    #[test]
    fn commercial_copy_explicitly_discloses_interpretive_behavior() {
        assert!(PRODUCT_DESCRIPTION.contains("interpretations"));
        assert!(PURCHASE_DISCLOSURE.contains("patterns and insights"));
        assert!(PURCHASE_DISCLOSURE.contains("incomplete or mistaken"));
    }
}
