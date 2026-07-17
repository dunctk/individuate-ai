//! Assertions for the standalone privacy E2E test.
//!
//! This deliberately lives in a binary instead of using a third-party
//! SQLCipher CLI so the test checks the same bundled SQLCipher build as the
//! application.

use rusqlite::Connection;
use serde::Serialize;
use std::{env, fs, path::Path};

#[derive(Debug, Serialize)]
struct Report {
    unkeyed_readable: bool,
    encrypted_messages: i64,
    encrypted_memory_rows: i64,
    encrypted_episodes: i64,
    encrypted_patient_graphs: i64,
    encrypted_social_graphs: i64,
    encrypted_cycle_profiles: i64,
    encrypted_cycle_events: i64,
    key_wraps: i64,
    plaintext_messages: i64,
    plaintext_sessions: i64,
    raw_files_contain_canary: bool,
}

fn count(conn: &Connection, sql: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [], |row| row.get(0))
}

fn open_keyed(path: &str, key: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "key", key)?;
    // Force SQLCipher to authenticate the page before querying tables.
    count(&conn, "SELECT count(*) FROM sqlite_master")?;
    Ok(conn)
}

fn contains_bytes(path: &Path, needle: &[u8]) -> bool {
    fs::read(path)
        .map(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
        .unwrap_or(false)
}

fn main() -> anyhow::Result<()> {
    let path = env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: e2e_inspect <database-path> <canary>"))?;
    let canary = env::args()
        .nth(2)
        .ok_or_else(|| anyhow::anyhow!("usage: e2e_inspect <database-path> <canary>"))?;
    let key = env::var("MEMORY_DB_KEY")
        .map_err(|_| anyhow::anyhow!("MEMORY_DB_KEY must be set for inspection"))?;

    let unkeyed_readable = Connection::open(&path)
        .and_then(|conn| count(&conn, "SELECT count(*) FROM sqlite_master"))
        .is_ok();
    let conn = open_keyed(&path, &key)?;

    let report = Report {
        unkeyed_readable,
        encrypted_messages: count(
            &conn,
            "SELECT count(*) FROM messages WHERE length(content_ciphertext) > 0 AND content = ''",
        )?,
        encrypted_memory_rows: count(
            &conn,
            "SELECT count(*) FROM encrypted_memory WHERE length(embedding_ciphertext) > 0",
        )?,
        encrypted_episodes: count(
            &conn,
            "SELECT count(*) FROM episodes WHERE length(title_ciphertext) > 0 AND length(narrative_ciphertext) > 0",
        )?,
        encrypted_patient_graphs: count(
            &conn,
            "SELECT count(*) FROM patient_graphs WHERE length(graph_ciphertext) > 0",
        )?,
        encrypted_social_graphs: count(
            &conn,
            "SELECT count(*) FROM social_graphs WHERE length(graph_ciphertext) > 0",
        )?,
        encrypted_cycle_profiles: count(
            &conn,
            "SELECT count(*) FROM cycle_profiles WHERE length(payload_ciphertext) > 0",
        )?,
        encrypted_cycle_events: count(
            &conn,
            "SELECT count(*) FROM cycle_events WHERE length(payload_ciphertext) > 0",
        )?,
        key_wraps: count(&conn, "SELECT count(*) FROM key_wraps")?,
        plaintext_messages: count(&conn, "SELECT count(*) FROM messages WHERE length(content) > 0")?,
        plaintext_sessions: count(
            &conn,
            "SELECT count(*) FROM sessions WHERE length(title) > 0 OR length(preview) > 0",
        )?,
        raw_files_contain_canary: [
            path.clone(),
            format!("{path}-wal"),
            format!("{path}-shm"),
            format!("{path}.plaintext.bak"),
        ]
        .iter()
        .map(Path::new)
        .any(|file| contains_bytes(file, canary.as_bytes())),
    };

    println!("{}", serde_json::to_string(&report)?);

    if report.unkeyed_readable
        || report.encrypted_messages == 0
        || report.encrypted_memory_rows == 0
        || report.encrypted_episodes == 0
        || report.encrypted_cycle_profiles == 0
        || report.encrypted_cycle_events == 0
        || report.key_wraps == 0
        || report.plaintext_messages != 0
        || report.plaintext_sessions != 0
        || report.raw_files_contain_canary
    {
        anyhow::bail!("privacy E2E database assertions failed");
    }

    Ok(())
}
