use crate::llm::client::{LlmClient, LlmError, LlmResult};
use crate::llm::prompts::SYSTEM_PROMPT;
use crate::llm::schema::ActionSchema;
use crate::telemetry;
use crate::types::{Action, Observation};
use crate::verify::repair::repair_action;
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
        history: &[Action],
    ) -> LlmResult<String> {
        let observation_json = serde_json::to_string(observation)
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let history_json = serde_json::to_string(history)
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let schema_json = serde_json::to_string(self.schema.json())
            .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
        let plan_text = plan.unwrap_or("(none)");

        Ok(format!(
            "Task: {task}\nPlan: {plan_text}\nObservation: {observation_json}\nHistory: {history_json}\nSchema: {schema_json}\nReturn exactly one JSON action object matching the schema and nothing else.",
        ))
    }

    fn parse_content(&self, content: &str) -> LlmResult<Action> {
        match self.parse_strict(content) {
            Ok(action) => Ok(action),
            Err(err) => {
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
                        let message = format!("{}; repair_failed: {}", err.message, repair_err);
                        tracing::warn!(
                            event = "repair_failed",
                            error_code = err.code,
                            repair_error = %repair_err
                        );
                        Err(LlmError::new("repair_failed", message))
                    }
                }
            }
        }
    }

    fn parse_strict(&self, content: &str) -> LlmResult<Action> {
        let value: Value = serde_json::from_str(content)
            .map_err(|err| LlmError::new("invalid_json", err.to_string()))?;
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
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn propose_action(
        &self,
        task: &str,
        plan: Option<&str>,
        observation: &Observation,
        history: &[Action],
    ) -> LlmResult<Action> {
        telemetry::inc_llm_call();
        let start = Instant::now();
        let span = tracing::info_span!("llm_call", model = %self.config.model);
        let result = async {
            let prompt = self.build_prompt(task, plan, observation, history)?;
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
                .map_err(|err| LlmError::new("http_error", err.to_string()))?;

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
                .map_err(|err| LlmError::new("http_error", err.to_string()))?;
            let content = payload
                .choices
                .get(0)
                .and_then(|choice| choice.message.content.as_ref())
                .ok_or_else(|| LlmError::new("empty_response", "missing content"))?;
            let content_text = extract_content(content)?;
            self.parse_content(&content_text)
        }
        .instrument(span)
        .await;

        telemetry::record_llm_duration(start.elapsed());
        if let Err(err) = &result {
            telemetry::inc_llm_failure();
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
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<Value>,
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
mod tests {
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
}
