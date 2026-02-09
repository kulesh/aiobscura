//! LLM assessment infrastructure.

use crate::config::{LlmConfig, LlmProvider};
use crate::types::{AuthorRole, Message, Session};
use crate::{Database, Error, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;

const DEFAULT_ASSESSOR: &str = "llm.assessment";
const MAX_TRANSCRIPT_CHARS: usize = 16_000;
const SYSTEM_PROMPT: &str = "You are an assessment engine for AI coding sessions. Return strict JSON with numeric scores 0.0-1.0 for keys: sycophancy, goal_clarity, autonomy_level, code_quality_signals, frustration_indicators. Also include a short string field summary.";

/// Fully-evaluated assessment payload ready for DB insert.
#[derive(Debug, Clone)]
pub struct AssessmentDraft {
    pub session_id: String,
    pub assessor: String,
    pub model: Option<String>,
    pub assessed_at: chrono::DateTime<Utc>,
    pub scores: serde_json::Value,
    pub raw_response: Option<String>,
    pub prompt_hash: Option<String>,
}

/// LLM completion interface for assessments.
pub trait LlmAssessmentClient: Send + Sync {
    fn complete(&self, prompt: &str) -> Result<String>;
}

/// Create the default HTTP-backed assessment client.
pub fn create_assessment_client(llm: &LlmConfig) -> Result<Box<dyn LlmAssessmentClient>> {
    Ok(Box::new(HttpLlmAssessmentClient::new(llm)?))
}

/// Assess a session and persist it to `assessments` if prompt hash changed.
///
/// Returns:
/// - `Ok(Some(assessment_id))` when a new assessment is stored
/// - `Ok(None)` when skipped due to unchanged prompt hash
pub fn assess_and_store_session(
    db: &Database,
    session: &Session,
    messages: &[Message],
    llm: &LlmConfig,
) -> Result<Option<i64>> {
    if messages.is_empty() {
        return Ok(None);
    }

    let client = create_assessment_client(llm)?;
    assess_and_store_session_with_client(db, session, messages, llm, client.as_ref())
}

/// Assess a session and persist it using a supplied client.
///
/// This allows callers to reuse a single initialized client across many sessions.
pub fn assess_and_store_session_with_client(
    db: &Database,
    session: &Session,
    messages: &[Message],
    llm: &LlmConfig,
    client: &dyn LlmAssessmentClient,
) -> Result<Option<i64>> {
    if messages.is_empty() {
        return Ok(None);
    }

    let draft = assess_with_client(session, messages, llm, client)?;

    if let Some(prompt_hash) = draft.prompt_hash.as_deref() {
        let latest_hash = db
            .get_latest_assessment(&session.id, &draft.assessor)?
            .and_then(|a| a.prompt_hash);
        if latest_hash.as_deref() == Some(prompt_hash) {
            return Ok(None);
        }
    }

    let id = db.insert_assessment(&crate::db::NewAssessment {
        session_id: &draft.session_id,
        assessor: &draft.assessor,
        model: draft.model.as_deref(),
        assessed_at: &draft.assessed_at,
        scores: &draft.scores,
        raw_response: draft.raw_response.as_deref(),
        prompt_hash: draft.prompt_hash.as_deref(),
    })?;

    Ok(Some(id))
}

/// Run assessment with a supplied client (used for tests and custom clients).
pub fn assess_with_client(
    session: &Session,
    messages: &[Message],
    llm: &LlmConfig,
    client: &dyn LlmAssessmentClient,
) -> Result<AssessmentDraft> {
    let prompt = build_prompt(session, messages);
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    let prompt_hash = Some(hex::encode(hasher.finalize()));

    let raw_response = client.complete(&prompt)?;
    let scores = parse_scores(&raw_response)?;

    Ok(AssessmentDraft {
        session_id: session.id.clone(),
        assessor: DEFAULT_ASSESSOR.to_string(),
        model: Some(llm.model.clone()),
        assessed_at: Utc::now(),
        scores,
        raw_response: Some(raw_response),
        prompt_hash,
    })
}

fn build_prompt(session: &Session, messages: &[Message]) -> String {
    let mut transcript = String::new();
    for msg in messages {
        let role = match msg.author_role {
            AuthorRole::Human => "human",
            AuthorRole::Caller => "caller",
            AuthorRole::Assistant => "assistant",
            AuthorRole::Agent => "agent",
            AuthorRole::Tool => "tool",
            AuthorRole::System => "system",
        };
        let content = msg.content.as_deref().unwrap_or("");
        let line = format!(
            "[{}] {} {}: {}\n",
            msg.emitted_at.to_rfc3339(),
            role,
            msg.message_type.as_str(),
            content.replace('\n', " ")
        );
        transcript.push_str(&line);
        if transcript.len() >= MAX_TRANSCRIPT_CHARS {
            transcript.truncate(MAX_TRANSCRIPT_CHARS);
            transcript.push_str("\n...[truncated]");
            break;
        }
    }

    format!(
        "{SYSTEM_PROMPT}\n\nSession ID: {}\nAssistant: {}\n\nTranscript:\n{}\n\nReturn only JSON.",
        session.id,
        session.assistant.as_str(),
        transcript
    )
}

fn parse_scores(raw: &str) -> Result<serde_json::Value> {
    let parsed = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => value,
        Err(_) => {
            let extracted = extract_json_object(raw)?;
            serde_json::from_str::<serde_json::Value>(&extracted)?
        }
    };

    if !parsed.is_object() {
        return Err(Error::Llm(
            "assessment response must be a JSON object".to_string(),
        ));
    }

    Ok(parsed)
}

fn extract_json_object(raw: &str) -> Result<String> {
    let start = raw
        .find('{')
        .ok_or_else(|| Error::Llm("assessment response did not contain JSON object".to_string()))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| Error::Llm("assessment response did not contain JSON object".to_string()))?;
    if end <= start {
        return Err(Error::Llm(
            "assessment response JSON bounds are invalid".to_string(),
        ));
    }
    Ok(raw[start..=end].to_string())
}

struct HttpLlmAssessmentClient {
    model: String,
    provider: LlmProvider,
    endpoint: String,
    api_key: Option<String>,
    runtime: tokio::runtime::Runtime,
    http: reqwest::Client,
}

impl HttpLlmAssessmentClient {
    fn new(config: &LlmConfig) -> Result<Self> {
        let endpoint = config
            .endpoint
            .clone()
            .unwrap_or_else(|| config.provider.default_endpoint().to_string());
        let api_key = match config.provider {
            LlmProvider::Ollama => None,
            LlmProvider::Claude => config
                .api_key
                .clone()
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok()),
            LlmProvider::OpenAI => config
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok()),
        };

        if matches!(config.provider, LlmProvider::Claude | LlmProvider::OpenAI) && api_key.is_none()
        {
            return Err(Error::Config(
                "llm.api_key (or provider env var) is required".to_string(),
            ));
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::Llm(format!("failed to build tokio runtime: {e}")))?;
        let timeout_secs = config.timeout_secs.max(1);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| Error::Llm(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            model: config.model.clone(),
            provider: config.provider,
            endpoint,
            api_key,
            runtime,
            http,
        })
    }
}

impl LlmAssessmentClient for HttpLlmAssessmentClient {
    fn complete(&self, prompt: &str) -> Result<String> {
        self.runtime.block_on(async {
            match self.provider {
                LlmProvider::Ollama => {
                    let url = format!("{}/api/generate", self.endpoint.trim_end_matches('/'));
                    let resp = self
                        .http
                        .post(url)
                        .json(&json!({
                            "model": self.model,
                            "prompt": prompt,
                            "stream": false,
                        }))
                        .send()
                        .await
                        .map_err(|e| Error::Llm(format!("ollama request failed: {e}")))?;
                    let status = resp.status();
                    let body = resp
                        .text()
                        .await
                        .map_err(|e| Error::Llm(format!("ollama read body failed: {e}")))?;
                    if !status.is_success() {
                        return Err(Error::Llm(format!(
                            "ollama returned {}: {}",
                            status.as_u16(),
                            body
                        )));
                    }
                    let json: serde_json::Value = serde_json::from_str(&body)?;
                    json.get("response")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .ok_or_else(|| {
                            Error::Llm(
                                "ollama response missing string field `response`".to_string(),
                            )
                        })
                }
                LlmProvider::Claude => {
                    let url = format!("{}/v1/messages", self.endpoint.trim_end_matches('/'));
                    let mut headers = HeaderMap::new();
                    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                    headers.insert(
                        "x-api-key",
                        HeaderValue::from_str(self.api_key.as_deref().unwrap_or_default())
                            .map_err(|e| {
                                Error::Llm(format!("invalid claude api key header: {e}"))
                            })?,
                    );
                    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

                    let resp = self
                        .http
                        .post(url)
                        .headers(headers)
                        .json(&json!({
                            "model": self.model,
                            "max_tokens": 600,
                            "temperature": 0,
                            "system": SYSTEM_PROMPT,
                            "messages": [{ "role": "user", "content": prompt }],
                        }))
                        .send()
                        .await
                        .map_err(|e| Error::Llm(format!("claude request failed: {e}")))?;
                    let status = resp.status();
                    let body = resp
                        .text()
                        .await
                        .map_err(|e| Error::Llm(format!("claude read body failed: {e}")))?;
                    if !status.is_success() {
                        return Err(Error::Llm(format!(
                            "claude returned {}: {}",
                            status.as_u16(),
                            body
                        )));
                    }
                    let json: serde_json::Value = serde_json::from_str(&body)?;
                    json.get("content")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|v| v.get("text"))
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .ok_or_else(|| {
                            Error::Llm("claude response missing content[0].text".to_string())
                        })
                }
                LlmProvider::OpenAI => {
                    let url = format!(
                        "{}/v1/chat/completions",
                        self.endpoint.trim_end_matches('/')
                    );
                    let mut headers = HeaderMap::new();
                    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                    headers.insert(
                        AUTHORIZATION,
                        HeaderValue::from_str(&format!(
                            "Bearer {}",
                            self.api_key.as_deref().unwrap_or_default()
                        ))
                        .map_err(|e| Error::Llm(format!("invalid auth header: {e}")))?,
                    );

                    let resp = self
                        .http
                        .post(url)
                        .headers(headers)
                        .json(&json!({
                            "model": self.model,
                            "temperature": 0,
                            "messages": [
                                { "role": "system", "content": SYSTEM_PROMPT },
                                { "role": "user", "content": prompt }
                            ]
                        }))
                        .send()
                        .await
                        .map_err(|e| Error::Llm(format!("openai request failed: {e}")))?;
                    let status = resp.status();
                    let body = resp
                        .text()
                        .await
                        .map_err(|e| Error::Llm(format!("openai read body failed: {e}")))?;
                    if !status.is_success() {
                        return Err(Error::Llm(format!(
                            "openai returned {}: {}",
                            status.as_u16(),
                            body
                        )));
                    }
                    let json: serde_json::Value = serde_json::from_str(&body)?;
                    json.get("choices")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|v| v.get("message"))
                        .and_then(|v| v.get("content"))
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .ok_or_else(|| {
                            Error::Llm(
                                "openai response missing choices[0].message.content".to_string(),
                            )
                        })
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LlmConfig, LlmProvider};
    use crate::types::{Assistant, MessageType, SessionStatus};
    use chrono::Utc;

    struct MockClient {
        response: String,
    }

    impl LlmAssessmentClient for MockClient {
        fn complete(&self, _prompt: &str) -> Result<String> {
            Ok(self.response.clone())
        }
    }

    fn test_session() -> Session {
        let now = Utc::now();
        Session {
            id: "session-assess".to_string(),
            assistant: Assistant::Codex,
            backing_model_id: None,
            project_id: None,
            started_at: now,
            last_activity_at: Some(now),
            status: SessionStatus::Active,
            source_file_path: "src.jsonl".to_string(),
            metadata: json!({}),
        }
    }

    fn test_message(content: &str) -> Message {
        let now = Utc::now();
        Message {
            id: 1,
            session_id: "session-assess".to_string(),
            thread_id: "session-assess-main".to_string(),
            seq: 1,
            emitted_at: now,
            observed_at: now,
            author_role: AuthorRole::Human,
            author_name: None,
            message_type: MessageType::Prompt,
            content: Some(content.to_string()),
            content_type: None,
            tool_name: None,
            tool_input: None,
            tool_result: None,
            tokens_in: None,
            tokens_out: None,
            duration_ms: None,
            source_file_path: "src.jsonl".to_string(),
            source_offset: 0,
            source_line: None,
            raw_data: json!({}),
            metadata: json!({}),
        }
    }

    fn llm_config() -> LlmConfig {
        LlmConfig {
            provider: LlmProvider::Ollama,
            model: "test-model".to_string(),
            endpoint: Some("http://localhost:11434".to_string()),
            api_key: None,
            timeout_secs: 30,
        }
    }

    #[test]
    fn assess_with_client_parses_json_and_hashes_prompt() {
        let session = test_session();
        let messages = vec![test_message("please fix the bug")];
        let client = MockClient {
            response: r#"{"sycophancy":0.2,"goal_clarity":0.8,"autonomy_level":0.7,"code_quality_signals":0.6,"frustration_indicators":0.1,"summary":"solid session"}"#.to_string(),
        };

        let draft = assess_with_client(&session, &messages, &llm_config(), &client)
            .expect("assessment should parse");
        assert_eq!(draft.assessor, DEFAULT_ASSESSOR);
        assert!(draft.prompt_hash.is_some());
        assert!(draft.scores.get("goal_clarity").is_some());
    }

    #[test]
    fn parse_scores_accepts_embedded_json() {
        let raw = "```json\n{\"sycophancy\":0.5,\"summary\":\"ok\"}\n```";
        let scores = parse_scores(raw).expect("embedded JSON should parse");
        assert_eq!(scores.get("sycophancy").and_then(|v| v.as_f64()), Some(0.5));
    }
}
