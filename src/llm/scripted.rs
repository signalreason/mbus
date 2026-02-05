use crate::llm::client::{LlmClient, LlmError, LlmResult};
use crate::telemetry;
use crate::types::Action;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct StubLlm {
    summary: String,
}

impl StubLlm {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
        }
    }
}

#[async_trait]
impl LlmClient for StubLlm {
    async fn propose_action(
        &self,
        _task: &str,
        _plan: Option<&str>,
        _observation: &crate::types::Observation,
        _history: &[Action],
    ) -> LlmResult<Action> {
        telemetry::inc_llm_call();
        let start = Instant::now();
        let action = Action::Done {
            summary: self.summary.clone(),
        };
        telemetry::record_llm_duration(start.elapsed());
        Ok(action)
    }
}

#[derive(Clone, Debug)]
pub struct ScriptedLlm {
    actions: Arc<Mutex<VecDeque<Action>>>,
    fallback_summary: String,
}

impl ScriptedLlm {
    pub fn new(actions: Vec<Action>) -> Self {
        Self {
            actions: Arc::new(Mutex::new(VecDeque::from(actions))),
            fallback_summary: "scripted actions exhausted".to_string(),
        }
    }

    pub fn from_path(path: &Path) -> LlmResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|err| LlmError::new("read_error", err.to_string()))?;
        let actions = parse_actions(&content)?;
        Ok(Self::new(actions))
    }
}

#[async_trait]
impl LlmClient for ScriptedLlm {
    async fn propose_action(
        &self,
        _task: &str,
        _plan: Option<&str>,
        _observation: &crate::types::Observation,
        _history: &[Action],
    ) -> LlmResult<Action> {
        telemetry::inc_llm_call();
        let start = Instant::now();
        let mut guard = self.actions.lock().await;
        let action = if let Some(action) = guard.pop_front() {
            action
        } else {
            Action::Done {
                summary: self.fallback_summary.clone(),
            }
        };
        telemetry::record_llm_duration(start.elapsed());
        Ok(action)
    }
}

fn parse_actions(content: &str) -> LlmResult<Vec<Action>> {
    if let Ok(actions) = serde_json::from_str::<Vec<Action>>(content) {
        return Ok(actions);
    }

    if let Ok(action) = serde_json::from_str::<Action>(content) {
        return Ok(vec![action]);
    }

    let mut actions = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let action: Action = serde_json::from_str(trimmed)
            .map_err(|err| LlmError::new("invalid_actions", err.to_string()))?;
        actions.push(action);
    }

    if actions.is_empty() {
        Err(LlmError::new(
            "invalid_actions",
            "no actions parsed",
        ))
    } else {
        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_array() {
        let payload = r#"[{"type":"done","summary":"ok"}]"#;
        let actions = parse_actions(payload).expect("actions");
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn parses_jsonl() {
        let payload = r#"{"type":"done","summary":"one"}
{"type":"done","summary":"two"}"#;
        let actions = parse_actions(payload).expect("actions");
        assert_eq!(actions.len(), 2);
    }
}
