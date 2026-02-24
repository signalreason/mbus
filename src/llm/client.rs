use crate::agent::memory::StepRecord;
use crate::types::{Action, Observation, TokenUsage};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::fmt;

#[derive(Debug)]
pub struct LlmContext<'a> {
    pub task: &'a str,
    pub plan: Option<&'a str>,
    pub observation: &'a Observation,
    pub observations: &'a VecDeque<Observation>,
    pub observation_screenshot: Option<&'a [u8]>,
    pub history: &'a [Action],
    pub steps: &'a [StepRecord],
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct LlmResponse {
    pub action: Action,
    pub usage: Option<TokenUsage>,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn propose_action(&self, context: &LlmContext<'_>) -> LlmResult<LlmResponse>;
}
