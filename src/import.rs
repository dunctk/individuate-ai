use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::io::{Cursor, Read};

use crate::agent::ChatLog;

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedConversation {
    pub title: String,
    pub messages: Vec<ChatLog>,
}

pub fn parse_gemini_takeout(filename: &str, bytes: &[u8]) -> Result<Vec<ImportedConversation>> {
    if bytes.len() > 25 * 1024 * 1024 {
        return Err(anyhow!(
            "The import is too large. Please upload a file under 25 MB."
        ));
    }

    if bytes.starts_with(b"PK\x03\x04") || filename.to_ascii_lowercase().ends_with(".zip") {
        return parse_zip(bytes);
    }
    let value: Value = serde_json::from_slice(bytes)
        .context("Gemini Takeout imports must be JSON or a ZIP containing JSON files")?;
    let mut conversations = Vec::new();
    collect_conversations(&value, filename, &mut conversations);
    if conversations.is_empty() {
        return Err(anyhow!(
            "No Gemini conversations with user messages were found."
        ));
    }
    Ok(dedupe_conversations(conversations))
}

fn parse_zip(bytes: &[u8]) -> Result<Vec<ImportedConversation>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .context("Could not read the Takeout ZIP archive")?;
    let mut conversations = Vec::new();
    let mut extracted_bytes = 0usize;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .context("Could not read a Takeout file")?;
        if file.is_dir() || !file.name().to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let name = file.name().to_string();
        let remaining = 25 * 1024 * 1024usize - extracted_bytes;
        let mut content = Vec::new();
        file.take((remaining + 1) as u64)
            .read_to_end(&mut content)
            .with_context(|| format!("Could not read {name}"))?;
        extracted_bytes += content.len();
        if extracted_bytes > 25 * 1024 * 1024 {
            return Err(anyhow!("The uncompressed import is too large."));
        }
        let value: Value = match serde_json::from_slice(&content) {
            Ok(value) => value,
            Err(_) => continue,
        };
        collect_conversations(&value, &name, &mut conversations);
    }
    if conversations.is_empty() {
        return Err(anyhow!(
            "No Gemini conversations with user messages were found in the ZIP."
        ));
    }
    Ok(dedupe_conversations(conversations))
}

fn collect_conversations(
    value: &Value,
    fallback_title: &str,
    output: &mut Vec<ImportedConversation>,
) {
    match value {
        Value::Object(object) => {
            for key in ["conversation", "messages", "turns", "contents", "chat"] {
                if let Some(Value::Array(items)) = object.get(key) {
                    let messages = items.iter().filter_map(parse_message).collect::<Vec<_>>();
                    if messages.iter().any(|message| message.role == "user") {
                        let title = object
                            .get("title")
                            .or_else(|| object.get("name"))
                            .and_then(Value::as_str)
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or(fallback_title);
                        output.push(ImportedConversation {
                            title: clean_title(title),
                            messages,
                        });
                    }
                }
            }
            for child in object.values() {
                collect_conversations(child, fallback_title, output);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_conversations(child, fallback_title, output);
            }
        }
        _ => {}
    }
}

fn parse_message(value: &Value) -> Option<ChatLog> {
    let object = value.as_object()?;
    let role = object
        .get("role")
        .or_else(|| object.get("author"))
        .or_else(|| object.get("speaker"))
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("role").and_then(Value::as_str))
        })?
        .to_ascii_lowercase();
    let role = if matches!(role.as_str(), "user" | "human" | "prompt") {
        "user"
    } else if matches!(role.as_str(), "assistant" | "model" | "gemini" | "bot") {
        "assistant"
    } else {
        return None;
    };
    let content = ["content", "text", "message", "parts"]
        .iter()
        .find_map(|key| object.get(*key).and_then(value_text))?
        .trim()
        .to_string();
    (!content.is_empty()).then(|| ChatLog {
        role: role.to_string(),
        content,
    })
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(value_text)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(object) => ["text", "content", "value", "parts"]
            .iter()
            .find_map(|key| object.get(*key).and_then(value_text)),
        _ => None,
    }
}

fn clean_title(title: &str) -> String {
    let title = title
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(title)
        .trim()
        .trim_end_matches(".json")
        .trim();
    if title.is_empty() {
        "Imported Gemini conversation".to_string()
    } else {
        title.chars().take(160).collect()
    }
}

fn dedupe_conversations(conversations: Vec<ImportedConversation>) -> Vec<ImportedConversation> {
    let mut unique = Vec::new();
    for conversation in conversations {
        if conversation.messages.is_empty() {
            continue;
        }
        let duplicate = unique.iter().any(|existing: &ImportedConversation| {
            existing.title == conversation.title && existing.messages == conversation.messages
        });
        if !duplicate {
            unique.push(conversation);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_and_assistant_turns_but_keeps_roles_distinct() {
        let json = br#"{"title":"Moving cities","conversation":[{"role":"user","content":"I moved to Madrid."},{"role":"model","content":"That sounds significant."}]}"#;
        let conversations = parse_gemini_takeout("conversations.json", json).unwrap();
        assert_eq!(conversations[0].title, "Moving cities");
        assert_eq!(conversations[0].messages[0].role, "user");
        assert_eq!(conversations[0].messages[1].role, "assistant");
    }

    #[test]
    fn ignores_ai_only_conversations() {
        let json = br#"[{"role":"model","content":"An invented fact."}]"#;
        assert!(parse_gemini_takeout("conversations.json", json).is_err());
    }

    #[test]
    fn reads_nested_parts_text() {
        let json = br#"{"conversation":[{"role":"human","parts":[{"text":"I felt overwhelmed yesterday."}]}]}"#;
        let conversations = parse_gemini_takeout("takeout.json", json).unwrap();
        assert_eq!(
            conversations[0].messages[0].content,
            "I felt overwhelmed yesterday."
        );
    }
}
