use async_trait::async_trait;
use mbus::agent::r#loop::{AgentLoop, LlmClients, RunStatus};
use mbus::agent::memory::StepRecord;
use mbus::agent::policy::AgentPolicy;
use mbus::bench::aggregate::aggregate_usage_from_steps;
use mbus::browser::{Browser, BrowserError};
use mbus::config::LlmMode;
use mbus::llm::client::{LlmClient, LlmError, LlmResponse};
use mbus::llm::scripted::ScriptedLlm;
use mbus::types::{Action, Observation, StepResult, TokenUsage};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
struct FakeBrowser {
    observation: Observation,
    applied: Arc<Mutex<Vec<Action>>>,
}

impl FakeBrowser {
    fn new(observation: Observation) -> Self {
        Self {
            observation,
            applied: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Browser for FakeBrowser {
    async fn snapshot(&self) -> Result<Observation, BrowserError> {
        Ok(self.observation.clone())
    }

    async fn apply(&self, action: &Action) -> Result<StepResult, BrowserError> {
        self.applied.lock().await.push(action.clone());
        Ok(StepResult {
            ok: true,
            error: None,
            new_state_hash: None,
            scroll: None,
            extract: None,
        })
    }

    async fn shutdown(&self) -> Result<(), BrowserError> {
        Ok(())
    }
}

#[derive(Clone)]
struct UsageLlm {
    usage: TokenUsage,
}

#[async_trait]
impl LlmClient for UsageLlm {
    async fn propose_action(
        &self,
        _task: &str,
        _plan: Option<&str>,
        _observation: &Observation,
        _observations: &VecDeque<Observation>,
        _history: &[Action],
        _steps: &[StepRecord],
    ) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            action: Action::Done {
                summary: "ok".to_string(),
            },
            usage: Some(self.usage.clone()),
        })
    }
}

fn sample_observation() -> Observation {
    Observation {
        url: "https://example.com".to_string(),
        title: "Example".to_string(),
        viewport: [1280, 800],
        focused: None,
        visible_text: "Hello".to_string(),
        screenshot: None,
        state_hash: "hash1".to_string(),
        elements: Vec::new(),
    }
}

#[tokio::test]
async fn scripted_mode_reads_actions_file_and_completes() {
    let temp_name = format!(
        "mbus_scripted_actions_{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );
    let temp_path = std::env::temp_dir().join(temp_name);
    std::fs::write(&temp_path, r#"[{"type":"done","summary":"ok"}]"#).expect("write actions file");

    let llm = ScriptedLlm::from_path(&temp_path).expect("scripted llm");
    let clients = LlmClients::new(Box::new(llm.clone()), Box::new(llm.clone()), Box::new(llm));
    let browser = FakeBrowser::new(sample_observation());
    let mut agent = AgentLoop::new(browser, clients, "task").with_policy(AgentPolicy {
        max_steps: 1,
        ..AgentPolicy::default()
    });

    let result = agent.run().await.expect("run");

    assert_eq!(result.status, RunStatus::Done);
    assert_eq!(result.steps.len(), 1);
    assert!(matches!(result.steps[0].action, Action::Done { .. }));

    let _ = std::fs::remove_file(&temp_path);
}

#[tokio::test]
async fn mocked_openai_usage_aggregates_tokens() {
    let usage = TokenUsage {
        prompt_tokens: Some(120),
        completion_tokens: Some(30),
        total_tokens: Some(150),
    };
    let llm = UsageLlm {
        usage: usage.clone(),
    };
    let clients = LlmClients::new(Box::new(llm.clone()), Box::new(llm.clone()), Box::new(llm));
    let browser = FakeBrowser::new(sample_observation());
    let mut agent = AgentLoop::new(browser, clients, "task").with_policy(AgentPolicy {
        max_steps: 1,
        ..AgentPolicy::default()
    });

    let result = agent.run().await.expect("run");

    let aggregated = aggregate_usage_from_steps(&result.steps, &LlmMode::OpenAi);
    assert!(aggregated.error.is_none());
    assert_eq!(aggregated.prompt_tokens, Some(120));
    assert_eq!(aggregated.completion_tokens, Some(30));
    assert_eq!(aggregated.total_tokens, Some(150));
}
