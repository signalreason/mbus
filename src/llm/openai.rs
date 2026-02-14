use crate::llm::client::{LlmClient, LlmError, LlmResponse, LlmResult};
use crate::llm::prompts::SYSTEM_PROMPT;
use crate::llm::repair::repair_action;
use crate::llm::schema::ActionSchema;
use crate::telemetry;
use crate::types::{Action, Observation, TokenUsage};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tracing::Instrument;

#[derive(Clone, Debug)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

impl OpenAiConfig {
    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

pub struct OpenAiClient {
    http: Client,
    config: OpenAiConfig,
    schema: ActionSchema,
}

impl OpenAiClient {
    pub fn new(config: OpenAiConfig) -> LlmResult<Self> {
        if config.api_key.trim().is_empty() {
            return Err(LlmError::new("missing_api_key", "api key is required"));
        }
        let http = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|err| LlmError::new("client_error", err.to_string()))?;
        Ok(Self {
            http,
            config,
            schema: ActionSchema::default(),
        })
    }

    fn build_prompt(
        &self,
        task: &str,
        plan: Option<&str>,
        observation: &Observation,
        observations: &std::collections::VecDeque<Observation>,
        history: &[Action],
    ) -> LlmResult<String> {
        let observation_json = serde_json::to_string(observation)
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let observations_json = serde_json::to_string(observations)
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let history_json = serde_json::to_string(history)
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let history_tail_json = serde_json::to_string(&history_tail(history, 8))
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let schema_json = serde_json::to_string(self.schema.json())
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let plan_text = plan.unwrap_or("(none)");
        let state_hash_streak = trailing_state_hash_streak(observations);

        Ok(format!(
            "Task: {task}\nPlan: {plan_text}\nObservation: {observation_json}\nRecentObservations: {observations_json}\nStateHashStreak: {state_hash_streak}\nHistory: {history_json}\nRecentHistoryTail: {history_tail_json}\nExecutionRules: [\"If StateHashStreak > 0, do not repeat the same exact action from RecentHistoryTail[-1]\", \"Use different element ids or action types when the state hash is unchanged\", \"Keep scroll deltas within |dx|<=2000 and |dy|<=2000\", \"Keep wait.ms <= 30000\"]\nSchema: {schema_json}\nReturn exactly one JSON action object matching the schema and nothing else.",
        ))
    }

    fn parse_content(&self, content: &str) -> LlmResult<Action> {
        let value = match self.parse_json_value(content) {
            Ok(value) => value,
            Err(err) if is_retryable_empty_output_error(&err) => return Err(err),
            Err(err) => return self.attempt_repair(err, content),
        };

        if let Err(err) = self.reject_multi_action(&value) {
            return self.attempt_repair(err, content);
        }

        match self.parse_value_strict(value.clone()) {
            Ok(action) => Ok(action),
            Err(err) => self.attempt_repair(err, content),
        }
    }

    #[allow(dead_code)]
    fn parse_strict(&self, content: &str) -> LlmResult<Action> {
        let value = self.parse_json_value(content)?;
        self.parse_value_strict(value)
    }

    fn parse_json_value(&self, content: &str) -> LlmResult<Value> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(LlmError::new("invalid_json", "empty response"));
        }
        serde_json::from_str(trimmed).map_err(|err| LlmError::new("invalid_json", err.to_string()))
    }

    fn parse_value_strict(&self, value: Value) -> LlmResult<Action> {
        self.schema.validate_json(&value).map_err(|errors| {
            let message = errors
                .into_iter()
                .map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            LlmError::new("schema_violation", message)
        })?;
        serde_json::from_value(value)
            .map_err(|err| LlmError::new("deserialize_error", err.to_string()))
    }

    fn reject_multi_action(&self, value: &Value) -> LlmResult<()> {
        match value {
            Value::Array(_) => Err(LlmError::new(
                "multi_action",
                "expected single JSON action object, got array",
            )),
            Value::Object(map) => {
                if matches!(map.get("action"), Some(Value::Array(_)))
                    || matches!(map.get("actions"), Some(Value::Array(_)))
                {
                    Err(LlmError::new(
                        "multi_action",
                        "expected single JSON action object, got action array",
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    fn attempt_repair(&self, err: LlmError, content: &str) -> LlmResult<Action> {
        telemetry::inc_repair_attempt();
        match repair_action(content, &self.schema) {
            Ok(action) => {
                telemetry::inc_repair_success();
                tracing::info!(
                    event = "repair_success",
                    error_code = err.code,
                    repaired = true
                );
                Ok(action)
            }
            Err(repair_err) => {
                telemetry::inc_repair_failure();
                let message = format!("{}; repair_failed: {}", err.message, repair_err);
                tracing::warn!(
                    event = "repair_failed",
                    error_code = err.code,
                    repair_error = %repair_err
                );
                Err(LlmError::new(err.code, message))
            }
        }
    }

    async fn send_chat_request(&self, body: &Value) -> LlmResult<ChatResponse> {
        let response = self
            .http
            .post(self.config.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "<no body>".to_string());
            if is_unsupported_temperature_error(status, &text) {
                return Err(LlmError::new(
                    "unsupported_temperature",
                    format!("status {status}: {text}"),
                ));
            }
            return Err(LlmError::new(
                "http_error",
                format!("status {status}: {text}"),
            ));
        }

        response.json().await.map_err(map_reqwest_error)
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn propose_action(
        &self,
        task: &str,
        plan: Option<&str>,
        observation: &Observation,
        observations: &std::collections::VecDeque<Observation>,
        history: &[Action],
    ) -> LlmResult<LlmResponse> {
        telemetry::inc_llm_call();
        let start = Instant::now();
        let span = tracing::info_span!("llm_call", model = %self.config.model);
        let result = async {
            let prompt = self.build_prompt(task, plan, observation, observations, history)?;
            let mut body = json!({
                "model": self.config.model,
                "messages": [
                    {"role": "system", "content": SYSTEM_PROMPT},
                    {"role": "user", "content": prompt}
                ]
            });
            body["temperature"] = json!(self.config.temperature);
            if let Some(max_tokens) = self.config.max_tokens {
                // OpenAI chat completions now prefer max_completion_tokens.
                body["max_completion_tokens"] = json!(max_tokens);
            }

            let payload = match self.send_chat_request(&body).await {
                Ok(payload) => payload,
                Err(err) if err.code == "unsupported_temperature" => {
                    tracing::info!(
                        event = "llm_retry_without_temperature",
                        model = %self.config.model
                    );
                    if let Value::Object(map) = &mut body {
                        map.remove("temperature");
                    }
                    self.send_chat_request(&body).await?
                }
                Err(err) => return Err(err),
            };
            let mut payload = payload;
            let mut action = self.parse_action(&payload);
            if let Err(err) = action.as_ref() {
                if is_retryable_empty_output_error(err) {
                    log_empty_output_diagnostics(&self.config.model, &payload, err, 1);
                    tracing::info!(
                        event = "llm_retry_empty_output",
                        model = %self.config.model,
                        error_code = err.code
                    );
                    payload = self.send_chat_request(&body).await?;
                    action = self.parse_action(&payload);
                    if let Err(retry_err) = action.as_ref() {
                        if is_retryable_empty_output_error(retry_err) {
                            log_empty_output_diagnostics(
                                &self.config.model,
                                &payload,
                                retry_err,
                                2,
                            );
                        }
                    }
                }
            }
            let action = action?;
            let usage = payload.usage.map(|usage| TokenUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            });
            Ok(LlmResponse { action, usage })
        }
        .instrument(span)
        .await;

        telemetry::record_llm_duration(start.elapsed());
        if let Err(err) = &result {
            telemetry::inc_llm_failure(err.code);
            tracing::warn!(
                event = "llm_failure",
                error_code = err.code,
                error_message_len = err.message.chars().count()
            );
        }
        result
    }
}

impl OpenAiClient {
    fn parse_action(&self, payload: &ChatResponse) -> LlmResult<Action> {
        let content = payload
            .choices
            .get(0)
            .and_then(|choice| choice.message.content.as_ref())
            .ok_or_else(|| LlmError::new("empty_response", "missing content"))?;
        let content_text = extract_content(content)?;
        self.parse_content(&content_text)
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<Value>,
    refusal: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: Option<ApiErrorBody>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
    param: Option<String>,
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

fn map_reqwest_error(err: reqwest::Error) -> LlmError {
    if err.is_timeout() {
        LlmError::new("timeout", err.to_string())
    } else if err.is_connect() {
        LlmError::new("transport_error", err.to_string())
    } else {
        LlmError::new("http_error", err.to_string())
    }
}

fn trailing_state_hash_streak(observations: &std::collections::VecDeque<Observation>) -> u32 {
    let Some(latest) = observations.back() else {
        return 0;
    };
    let mut streak = 0_u32;
    for observation in observations.iter().rev().skip(1) {
        if observation.state_hash == latest.state_hash {
            streak = streak.saturating_add(1);
        } else {
            break;
        }
    }
    streak
}

fn history_tail(history: &[Action], max_items: usize) -> &[Action] {
    let start = history.len().saturating_sub(max_items);
    &history[start..]
}

fn is_unsupported_temperature_error(status: reqwest::StatusCode, body: &str) -> bool {
    if status != reqwest::StatusCode::BAD_REQUEST {
        return false;
    }

    let parsed: ApiErrorEnvelope = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let Some(error) = parsed.error else {
        return false;
    };
    if error.param.as_deref() != Some("temperature") {
        return false;
    }

    if error.code.as_deref() == Some("unsupported_value") {
        return true;
    }

    error
        .message
        .as_deref()
        .map(|message| {
            let lowered = message.to_ascii_lowercase();
            lowered.contains("temperature") && lowered.contains("default")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod error_tests {
    use super::is_unsupported_temperature_error;

    #[test]
    fn detects_unsupported_temperature_error() {
        let body = r#"{
            "error": {
                "message": "Unsupported value: 'temperature' does not support 0.2 with this model. Only the default (1) value is supported.",
                "type": "invalid_request_error",
                "param": "temperature",
                "code": "unsupported_value"
            }
        }"#;

        assert!(is_unsupported_temperature_error(
            reqwest::StatusCode::BAD_REQUEST,
            body
        ));
    }

    #[test]
    fn ignores_non_temperature_errors() {
        let body = r#"{
            "error": {
                "message": "Unsupported value: 'max_tokens'.",
                "type": "invalid_request_error",
                "param": "max_tokens",
                "code": "unsupported_value"
            }
        }"#;

        assert!(!is_unsupported_temperature_error(
            reqwest::StatusCode::BAD_REQUEST,
            body
        ));
    }
}

#[cfg(test)]
mod prompt_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;

    fn sample_observation(hash: &str) -> Observation {
        Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            state_hash: hash.to_string(),
            elements: Vec::new(),
        }
    }

    #[test]
    fn prompt_includes_recent_observations_in_order() {
        let client = OpenAiClient::new(OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost".to_string(),
            model: "test".to_string(),
            timeout: Duration::from_secs(1),
            temperature: 0.0,
            max_tokens: None,
        })
        .expect("client");

        let mut observations = VecDeque::new();
        observations.push_back(sample_observation("hash-1"));
        observations.push_back(sample_observation("hash-2"));
        let current = sample_observation("hash-2");

        let prompt = client
            .build_prompt("task", None, &current, &observations, &[])
            .expect("prompt");

        let line = prompt
            .lines()
            .find(|line| line.starts_with("RecentObservations: "))
            .expect("recent observations line");
        let payload = line.trim_start_matches("RecentObservations: ");
        let parsed: Vec<Observation> = serde_json::from_str(payload).expect("parse observations");

        let hashes: Vec<String> = parsed.into_iter().map(|obs| obs.state_hash).collect();
        assert_eq!(hashes, vec!["hash-1".to_string(), "hash-2".to_string()]);
    }

    #[test]
    fn prompt_includes_state_hash_streak() {
        let client = OpenAiClient::new(OpenAiConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost".to_string(),
            model: "test".to_string(),
            timeout: Duration::from_secs(1),
            temperature: 0.0,
            max_tokens: None,
        })
        .expect("client");

        let mut observations = VecDeque::new();
        observations.push_back(sample_observation("hash-1"));
        observations.push_back(sample_observation("hash-2"));
        observations.push_back(sample_observation("hash-2"));
        observations.push_back(sample_observation("hash-2"));
        let current = sample_observation("hash-2");

        let prompt = client
            .build_prompt("task", None, &current, &observations, &[])
            .expect("prompt");

        assert!(
            prompt.contains("StateHashStreak: 2"),
            "expected prompt to include trailing streak count"
        );
    }
}

fn extract_content(value: &Value) -> LlmResult<String> {
    match value {
        Value::String(text) => {
            if text.trim().is_empty() {
                Err(LlmError::new("empty_response", "missing content text"))
            } else {
                Ok(text.to_string())
            }
        }
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                    out.push_str(text);
                }
            }
            if out.trim().is_empty() {
                Err(LlmError::new("empty_response", "missing content text"))
            } else {
                Ok(out)
            }
        }
        _ => Err(LlmError::new("empty_response", "unexpected content format")),
    }
}

fn is_retryable_empty_output_error(err: &LlmError) -> bool {
    err.code == "empty_response"
        || (err.code == "invalid_json" && err.message.contains("empty response"))
}

fn log_empty_output_diagnostics(model: &str, payload: &ChatResponse, err: &LlmError, attempt: u8) {
    let diagnostics = collect_empty_output_diagnostics(payload);
    tracing::warn!(
        event = "llm_empty_output",
        model = model,
        attempt = attempt as u64,
        error_code = err.code,
        error_message_len = err.message.chars().count(),
        finish_reason = ?diagnostics.finish_reason,
        content_kind = diagnostics.content_kind,
        content_text_chars = diagnostics.content_text_chars as u64,
        content_part_types = ?diagnostics.content_part_types,
        refusal_kind = diagnostics.refusal_kind,
        refusal_chars = diagnostics.refusal_chars as u64,
        prompt_tokens = ?diagnostics.prompt_tokens,
        completion_tokens = ?diagnostics.completion_tokens,
        total_tokens = ?diagnostics.total_tokens
    );
}

#[derive(Debug, PartialEq, Eq)]
struct EmptyOutputDiagnostics {
    finish_reason: Option<String>,
    content_kind: &'static str,
    content_text_chars: usize,
    content_part_types: Vec<String>,
    refusal_kind: &'static str,
    refusal_chars: usize,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

fn collect_empty_output_diagnostics(payload: &ChatResponse) -> EmptyOutputDiagnostics {
    let usage = payload.usage.as_ref();
    let mut diagnostics = EmptyOutputDiagnostics {
        finish_reason: None,
        content_kind: "missing_choice",
        content_text_chars: 0,
        content_part_types: Vec::new(),
        refusal_kind: "none",
        refusal_chars: 0,
        prompt_tokens: usage.and_then(|value| value.prompt_tokens),
        completion_tokens: usage.and_then(|value| value.completion_tokens),
        total_tokens: usage.and_then(|value| value.total_tokens),
    };

    let Some(choice) = payload.choices.first() else {
        return diagnostics;
    };
    diagnostics.finish_reason = choice.finish_reason.clone();

    let (content_kind, content_text_chars, content_part_types) =
        collect_content_diagnostics(choice.message.content.as_ref());
    diagnostics.content_kind = content_kind;
    diagnostics.content_text_chars = content_text_chars;
    diagnostics.content_part_types = content_part_types;

    let (refusal_kind, refusal_chars) =
        collect_refusal_diagnostics(choice.message.refusal.as_ref());
    diagnostics.refusal_kind = refusal_kind;
    diagnostics.refusal_chars = refusal_chars;

    diagnostics
}

fn collect_content_diagnostics(content: Option<&Value>) -> (&'static str, usize, Vec<String>) {
    match content {
        None => ("none", 0, Vec::new()),
        Some(Value::String(text)) => ("string", text.chars().count(), Vec::new()),
        Some(Value::Array(parts)) => {
            let mut text_chars = 0;
            let mut part_types = Vec::new();
            for part in parts {
                if let Some(part_type) = part.get("type").and_then(|value| value.as_str()) {
                    part_types.push(part_type.to_string());
                }
                if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                    text_chars += text.chars().count();
                }
            }
            ("array", text_chars, part_types)
        }
        Some(_) => ("other", 0, Vec::new()),
    }
}

fn collect_refusal_diagnostics(refusal: Option<&Value>) -> (&'static str, usize) {
    match refusal {
        None => ("none", 0),
        Some(Value::String(text)) => ("string", text.chars().count()),
        Some(Value::Array(values)) => {
            let chars = values
                .iter()
                .filter_map(|value| value.as_str())
                .map(|text| text.chars().count())
                .sum();
            ("array", chars)
        }
        Some(_) => ("other", 0),
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;
    use std::time::Duration;

    fn test_client() -> OpenAiClient {
        OpenAiClient::new(OpenAiConfig {
            api_key: "test".to_string(),
            base_url: "http://localhost".to_string(),
            model: "unit-test".to_string(),
            timeout: Duration::from_secs(1),
            temperature: 0.0,
            max_tokens: None,
        })
        .expect("client")
    }

    #[test]
    fn parse_strict_reports_invalid_json_code() {
        let client = test_client();
        let err = client
            .parse_strict("not json")
            .expect_err("expected invalid json");
        assert_eq!(err.code, "invalid_json");
    }

    #[test]
    fn parse_strict_reports_schema_violation_codes() {
        let client = test_client();
        let samples = [
            r#"{"type":"click"}"#,
            r#"{"type":"wait","ms":"fast"}"#,
            r#"{"type":"teleport","id":"el_1"}"#,
        ];
        for payload in samples {
            let err = client
                .parse_strict(payload)
                .expect_err("expected schema violation");
            assert_eq!(err.code, "schema_violation");
        }
    }

    #[test]
    fn parse_content_rejects_non_json() {
        let client = test_client();
        let err = client
            .parse_content("not json")
            .expect_err("expected invalid json");
        assert_eq!(err.code, "invalid_json");
    }

    #[test]
    fn parse_content_empty_keeps_empty_response_message() {
        let client = test_client();
        let err = client.parse_content("").expect_err("expected invalid json");
        assert_eq!(err.code, "invalid_json");
        assert_eq!(err.message, "empty response");
    }

    #[test]
    fn parse_content_rejects_action_arrays() {
        let client = test_client();
        let payload = r#"[{"type":"done","summary":"ok"},{"type":"done","summary":"two"}]"#;
        let err = client
            .parse_content(payload)
            .expect_err("expected multi action error");
        assert_eq!(err.code, "multi_action");
    }

    #[test]
    fn parse_content_rejects_action_wrapper_arrays() {
        let client = test_client();
        let payload =
            r#"{"action":[{"type":"done","summary":"ok"},{"type":"done","summary":"two"}]}"#;
        let err = client
            .parse_content(payload)
            .expect_err("expected multi action error");
        assert_eq!(err.code, "multi_action");
    }

    #[test]
    fn parse_content_repairs_single_item_array() {
        let client = test_client();
        let action = client
            .parse_content(r#"[{"type":"done","summary":"ok"}]"#)
            .expect("repair single action array");
        assert_eq!(
            action,
            Action::Done {
                summary: "ok".to_string()
            }
        );
    }

    #[test]
    fn parse_content_repairs_action_wrapper() {
        let client = test_client();
        let payload = r#"{"action":{"type":"click","id":"el_1"}}"#;
        let action = client
            .parse_content(payload)
            .expect("repair action wrapper");
        assert_eq!(
            action,
            Action::Click {
                id: "el_1".to_string()
            }
        );
    }

    #[test]
    fn parse_content_repairs_code_fence() {
        let client = test_client();
        let payload = "```json\n{\"type\":\"wait\",\"ms\":200}\n```";
        let action = client.parse_content(payload).expect("repair fenced action");
        assert_eq!(action, Action::Wait { ms: 200 });
    }

    #[test]
    fn parse_content_repairs_json_string() {
        let client = test_client();
        let payload = r#""{\"type\":\"click\",\"id\":\"el_9\"}""#;
        let action = client.parse_content(payload).expect("repair json string");
        assert_eq!(
            action,
            Action::Click {
                id: "el_9".to_string()
            }
        );
    }

    #[test]
    fn extract_content_rejects_empty_string_content() {
        let err =
            extract_content(&Value::String(" ".to_string())).expect_err("expected empty response");
        assert_eq!(err.code, "empty_response");
    }

    #[test]
    fn retryable_empty_output_error_covers_known_empty_signals() {
        let empty_response = LlmError::new("empty_response", "missing content");
        assert!(is_retryable_empty_output_error(&empty_response));

        let empty_json = LlmError::new(
            "invalid_json",
            "empty response; repair_failed: invalid json: empty content",
        );
        assert!(is_retryable_empty_output_error(&empty_json));

        let invalid = LlmError::new("invalid_json", "expected value at line 1 column 1");
        assert!(!is_retryable_empty_output_error(&invalid));
    }

    #[test]
    fn collect_empty_output_diagnostics_reads_finish_reason_and_usage() {
        let payload = ChatResponse {
            choices: vec![Choice {
                message: Message {
                    content: Some(Value::Array(vec![
                        serde_json::json!({"type":"text","text":" "}),
                        serde_json::json!({"type":"output_text","text":""}),
                    ])),
                    refusal: Some(Value::String("".to_string())),
                },
                finish_reason: Some("length".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: Some(123),
                completion_tokens: Some(7),
                total_tokens: Some(130),
            }),
        };

        let diagnostics = collect_empty_output_diagnostics(&payload);
        assert_eq!(diagnostics.finish_reason.as_deref(), Some("length"));
        assert_eq!(diagnostics.content_kind, "array");
        assert_eq!(diagnostics.content_text_chars, 1);
        assert_eq!(
            diagnostics.content_part_types,
            vec!["text".to_string(), "output_text".to_string()]
        );
        assert_eq!(diagnostics.refusal_kind, "string");
        assert_eq!(diagnostics.prompt_tokens, Some(123));
        assert_eq!(diagnostics.completion_tokens, Some(7));
        assert_eq!(diagnostics.total_tokens, Some(130));
    }
}
