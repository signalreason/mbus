use crate::types::{Action, Observation};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::fmt;

pub type LlmResult<T> = Result<T, LlmError>;

#[derive(Debug, Clone)]
pub struct LlmError {
    pub code: &'static str,
    pub message: String,
}

impl LlmError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LlmError {}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn propose_action(
        &self,
        task: &str,
        plan: Option<&str>,
        observation: &Observation,
        observations: &VecDeque<Observation>,
        history: &[Action],
    ) -> LlmResult<Action>;
}
