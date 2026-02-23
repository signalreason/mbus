pub mod act;
pub mod cdp;
pub mod observe;

use crate::types::{Action, Observation, StepResult};
use async_trait::async_trait;
use std::fmt;

pub use cdp::{CdpBrowser, CdpConfig};
pub use observe::{Observer, ObserverConfig};

pub type BrowserResult<T> = Result<T, BrowserError>;

#[derive(Debug, Clone)]
pub struct BrowserError {
    pub code: &'static str,
    pub message: String,
}

impl BrowserError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BrowserError {}

#[async_trait]
pub trait Browser: Send + Sync {
    async fn snapshot(&self) -> BrowserResult<Observation>;
    async fn apply(&self, action: &Action) -> BrowserResult<StepResult>;
    async fn shutdown(&self) -> BrowserResult<()>;
    async fn take_last_screenshot(&self) -> BrowserResult<Option<Vec<u8>>> {
        Ok(None)
    }
}
