use crate::llm::client::{LlmClient, LlmError, LlmResponse, LlmResult};
use crate::llm::prompts::SYSTEM_PROMPT;
use crate::llm::repair::repair_action;
use crate::llm::schema::ActionSchema;
use crate::telemetry;
use crate::types::{Action, Observation, TokenUsage};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
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
        format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        )
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
        let schema_json = serde_json::to_string(self.schema.json())
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let plan_text = plan.unwrap_or("(none)");

        Ok(format!(
            "Task: {task}\nPlan: {plan_text}\nObservation: {observation_json}\nRecentObservations: {observations_json}\nHistory: {history_json}\nSchema: {schema_json}\nReturn exactly one JSON action object matching the schema and nothing else.",
        ))
    }

    fn parse_content(&self, content: &str) -> LlmResult<Action> {
        let value = match self.parse_json_value(content) {
            Ok(value) => value,
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
        self.schema
            .validate_json(&value)
            .map_err(|errors| {
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
                ],
                "temperature": self.config.temperature
            });
            if let Some(max_tokens) = self.config.max_tokens {
                // OpenAI chat completions now prefer max_completion_tokens.
                body["max_completion_tokens"] = json!(max_tokens);
            }

            let response = self
                .http
                .post(self.config.endpoint())
                .bearer_auth(&self.config.api_key)
                .json(&body)
                .send()
                .await
                .map_err(map_reqwest_error)?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<no body>".to_string());
                return Err(LlmError::new(
                    "http_error",
                    format!("status {status}: {text}"),
                ));
            }

            let payload: ChatResponse = response
                .json()
                .await
                .map_err(map_reqwest_error)?;
            let content = payload
                .choices
                .get(0)
                .and_then(|choice| choice.message.content.as_ref())
                .ok_or_else(|| LlmError::new("empty_response", "missing content"))?;
            let content_text = extract_content(content)?;
            let action = self.parse_content(&content_text)?;
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

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<Value>,
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
}

fn extract_content(value: &Value) -> LlmResult<String> {
    match value {
        Value::String(text) => Ok(text.to_string()),
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
        _ => Err(LlmError::new(
            "empty_response",
            "unexpected content format",
        )),
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
        let err = client.parse_strict("not json").expect_err("expected invalid json");
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
    fn parse_content_rejects_action_arrays() {
        let client = test_client();
        let payload =
            r#"[{"type":"done","summary":"ok"},{"type":"done","summary":"two"}]"#;
        let err = client
            .parse_content(payload)
            .expect_err("expected multi action error");
        assert_eq!(err.code, "multi_action");
    }

    #[test]
    fn parse_content_rejects_action_wrapper_arrays() {
        let client = test_client();
        let payload = r#"{"action":[{"type":"done","summary":"ok"},{"type":"done","summary":"two"}]}"#;
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
        assert_eq!(action, Action::Click { id: "el_1".to_string() });
    }

    #[test]
    fn parse_content_repairs_code_fence() {
        let client = test_client();
        let payload = "```json\n{\"type\":\"wait\",\"ms\":200}\n```";
        let action = client
            .parse_content(payload)
            .expect("repair fenced action");
        assert_eq!(action, Action::Wait { ms: 200 });
    }

    #[test]
    fn parse_content_repairs_json_string() {
        let client = test_client();
        let payload = r#""{\"type\":\"click\",\"id\":\"el_9\"}""#;
        let action = client
            .parse_content(payload)
            .expect("repair json string");
        assert_eq!(action, Action::Click { id: "el_9".to_string() });
    }
}
