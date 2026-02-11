use crate::agent::memory::{Memory, StepRecord, StepTimings, ValidationOutcome};
use crate::agent::policy::AgentPolicy;
use crate::browser::{Browser, BrowserError};
use crate::llm::client::{LlmClient, LlmError};
use crate::llm::router::{step_outcome, Router, StepOutcome, Tier};
use crate::telemetry;
use crate::types::{Action, Observation, StepError, StepResult};
use crate::verify::rules::{ValidationError, Validator};
use std::fmt;
use std::time::{Duration, Instant};
use tracing::Instrument;

pub struct LlmClients {
    fast: Box<dyn LlmClient>,
    mid: Box<dyn LlmClient>,
    strong: Box<dyn LlmClient>,
}

impl LlmClients {
    pub fn new(
        fast: Box<dyn LlmClient>,
        mid: Box<dyn LlmClient>,
        strong: Box<dyn LlmClient>,
    ) -> Self {
        Self { fast, mid, strong }
    }

    fn client(&self, tier: Tier) -> &dyn LlmClient {
        match tier {
            Tier::Fast => self.fast.as_ref(),
            Tier::Mid => self.mid.as_ref(),
            Tier::Strong => self.strong.as_ref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Done,
    MaxSteps,
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub status: RunStatus,
    pub final_action: Action,
    pub steps: Vec<StepRecord>,
    pub final_observation: Observation,
}

#[derive(Debug)]
pub enum AgentError {
    Browser(BrowserError),
    Llm(LlmError),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::Browser(err) => write!(f, "browser error: {err}"),
            AgentError::Llm(err) => write!(f, "llm error: {err}"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<BrowserError> for AgentError {
    fn from(err: BrowserError) -> Self {
        AgentError::Browser(err)
    }
}

impl From<LlmError> for AgentError {
    fn from(err: LlmError) -> Self {
        AgentError::Llm(err)
    }
}

pub struct AgentLoop<B: Browser> {
    browser: B,
    clients: LlmClients,
    router: Router,
    validator: Validator,
    policy: AgentPolicy,
    memory: Memory,
    task: String,
}

impl<B: Browser> AgentLoop<B> {
    pub fn new(browser: B, clients: LlmClients, task: impl Into<String>) -> Self {
        let policy = AgentPolicy::default();
        let memory = Memory::new(policy.memory.clone());
        Self {
            browser,
            clients,
            router: Router::default(),
            validator: Validator::default(),
            policy,
            memory,
            task: task.into(),
        }
    }

    pub fn with_plan(mut self, plan: impl Into<String>) -> Self {
        self.memory.set_plan(Some(plan.into()));
        self
    }

    pub fn with_policy(mut self, policy: AgentPolicy) -> Self {
        let plan = self.memory.plan().map(|value| value.to_string());
        self.policy = policy;
        self.memory = Memory::new(self.policy.memory.clone());
        self.memory.set_plan(plan);
        self
    }

    pub fn with_router(mut self, router: Router) -> Self {
        self.router = router;
        self
    }

    pub fn with_validator(mut self, validator: Validator) -> Self {
        self.validator = validator;
        self
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    pub async fn shutdown(&self) -> Result<(), BrowserError> {
        self.browser.shutdown().await
    }

    pub async fn run(&mut self) -> Result<RunResult, AgentError> {
        enum StepControl {
            Continue,
            Done(Action),
        }

        let snapshot_start = Instant::now();
        let mut observation = match self.browser.snapshot().await {
            Ok(observation) => {
                telemetry::record_snapshot_duration(snapshot_start.elapsed());
                observation
            }
            Err(err) => {
                telemetry::record_snapshot_duration(snapshot_start.elapsed());
                tracing::error!(
                    event = "snapshot_error",
                    error_code = err.code,
                    error_message = %err.message
                );
                return Err(AgentError::Browser(err));
            }
        };
        self.memory.record_observation(observation.clone());

        for step_index in 0..self.policy.max_steps {
            let tier = self.router.tier();
            telemetry::inc_step();

            let step_span = tracing::info_span!(
                "step",
                step_index = step_index + 1,
                tier = ?tier,
                url = %observation.url,
                state_hash = ?observation.state_hash
            );

            let step_result = async {
                let step_start = Instant::now();
                let client = self.clients.client(tier);

                let llm_start = Instant::now();
                let action = match client
                    .propose_action(
                        &self.task,
                        self.memory.plan(),
                        &observation,
                        self.memory.observations(),
                        self.memory.history(),
                    )
                    .await
                {
                    Ok(action) => action,
                    Err(err) => {
                        let duration = step_start.elapsed();
                        telemetry::record_step_duration(duration);
                        tracing::error!(
                            event = "llm_error",
                            error_code = err.code,
                            error_message_len = err.message.chars().count(),
                            step_duration_ms = duration.as_millis() as u64
                        );
                        return Err(AgentError::Llm(err));
                    }
                };
                let llm_duration = llm_start.elapsed();

                telemetry::inc_action(&action);
                tracing::info!(
                    event = "action_proposed",
                    action_type = telemetry::action_type(&action),
                    action = ?telemetry::ActionSummary::from(&action)
                );

                if let Err(errors) = self.validator.validate(&action, &observation) {
                    let error_count = errors.len();
                    telemetry::inc_validation_failure();
                    let mut result = validation_result(errors.clone());
                    result.new_state_hash = Some(observation.state_hash.clone());
                    let duration = step_start.elapsed();
                    self.memory.record_step(StepRecord {
                        action,
                        validation: ValidationOutcome::failure(errors),
                        result,
                        timings: StepTimings {
                            step_duration_ms: duration_ms(duration),
                            llm_duration_ms: duration_ms(llm_duration),
                            apply_duration_ms: None,
                            snapshot_duration_ms: None,
                        },
                    });
                    let tier_after = self.router.record(StepOutcome::Failure);
                    telemetry::set_no_progress_streak(self.router.counters().no_progress);
                    telemetry::record_step_duration(duration);
                    tracing::warn!(
                        event = "validation_failed",
                        error_count = error_count,
                        tier = ?tier_after,
                        step_duration_ms = duration.as_millis() as u64
                    );
                    return Ok(StepControl::Continue);
                }

                if matches!(action, Action::Done { .. }) {
                    let result = done_result(&observation);
                    let duration = step_start.elapsed();
                    self.memory.record_step(StepRecord {
                        action: action.clone(),
                        validation: ValidationOutcome::success(),
                        result,
                        timings: StepTimings {
                            step_duration_ms: duration_ms(duration),
                            llm_duration_ms: duration_ms(llm_duration),
                            apply_duration_ms: None,
                            snapshot_duration_ms: None,
                        },
                    });
                    telemetry::record_step_duration(duration);
                    tracing::info!(
                        event = "done",
                        step_duration_ms = duration.as_millis() as u64
                    );
                    return Ok(StepControl::Done(action));
                }

                let apply_start = Instant::now();
                let mut result = match self.browser.apply(&action).await {
                    Ok(result) => result,
                    Err(err) => {
                        let apply_duration = apply_start.elapsed();
                        telemetry::record_apply_duration(apply_duration);
                        let duration = step_start.elapsed();
                        telemetry::record_step_duration(duration);
                        tracing::error!(
                            event = "apply_error",
                            error_code = err.code,
                            error_message = %err.message,
                            step_duration_ms = duration.as_millis() as u64
                        );
                        return Err(AgentError::Browser(err));
                    }
                };
                let apply_duration = apply_start.elapsed();
                telemetry::record_apply_duration(apply_duration);
                if !result.ok {
                    telemetry::inc_apply_failure();
                }

                let snapshot_start = Instant::now();
                let next_observation = match self.browser.snapshot().await {
                    Ok(observation) => observation,
                    Err(err) => {
                        let snapshot_duration = snapshot_start.elapsed();
                        telemetry::record_snapshot_duration(snapshot_duration);
                        let duration = step_start.elapsed();
                        telemetry::record_step_duration(duration);
                        tracing::error!(
                            event = "snapshot_error",
                            error_code = err.code,
                            error_message = %err.message,
                            step_duration_ms = duration.as_millis() as u64
                        );
                        return Err(AgentError::Browser(err));
                    }
                };
                let snapshot_duration = snapshot_start.elapsed();
                telemetry::record_snapshot_duration(snapshot_duration);
                self.memory.record_observation(next_observation.clone());
                let new_hash = next_observation.state_hash.clone();
                result.new_state_hash = Some(new_hash.clone());

                let duration = step_start.elapsed();
                self.memory.record_step(StepRecord {
                    action: action.clone(),
                    validation: ValidationOutcome::success(),
                    result: result.clone(),
                    timings: StepTimings {
                        step_duration_ms: duration_ms(duration),
                        llm_duration_ms: duration_ms(llm_duration),
                        apply_duration_ms: Some(duration_ms(apply_duration)),
                        snapshot_duration_ms: Some(duration_ms(snapshot_duration)),
                    },
                });
                self.memory.update_last_step_state_hash(new_hash);

                let (outcome, heuristics) = step_outcome(&result, &observation, &next_observation);
                let tier_after = self.router.record(outcome);
                telemetry::set_no_progress_streak(self.router.counters().no_progress);

                let error_code = result
                    .error
                    .as_ref()
                    .map(|err| err.code.as_str())
                    .unwrap_or("none");
                telemetry::record_step_duration(duration);
                tracing::info!(
                    event = "step_result",
                    outcome = ?outcome,
                    ok = result.ok,
                    error_code = error_code,
                    tier = ?tier_after,
                    apply_duration_ms = apply_duration.as_millis() as u64,
                    snapshot_duration_ms = snapshot_duration.as_millis() as u64,
                    step_duration_ms = duration.as_millis() as u64,
                    new_state_hash = ?result.new_state_hash,
                    state_hash_unchanged = heuristics.state_hash_unchanged,
                    actionables_unchanged = heuristics.actionables_unchanged,
                    low_actionability = heuristics.low_actionability,
                    prev_actionables = heuristics.prev_actionables,
                    next_actionables = heuristics.next_actionables
                );

                observation = next_observation;
                Ok(StepControl::Continue)
            }
            .instrument(step_span)
            .await?;

            if let StepControl::Done(action) = step_result {
                return Ok(RunResult {
                    status: RunStatus::Done,
                    final_action: action,
                    steps: self.memory.steps().to_vec(),
                    final_observation: observation,
                });
            }
        }

        let final_action = Action::Done {
            summary: "max_steps reached".to_string(),
        };

        Ok(RunResult {
            status: RunStatus::MaxSteps,
            final_action,
            steps: self.memory.steps().to_vec(),
            final_observation: observation,
        })
    }
}

fn validation_result(errors: Vec<ValidationError>) -> StepResult {
    let validation_code = errors.first().map(|err| err.code.clone());
    let message = format_validation_errors(&errors);
    StepResult {
        ok: false,
        error: Some(StepError {
            code: "invalid_action".to_string(),
            message,
            validation_code,
        }),
        new_state_hash: None,
        scroll: None,
        extract: None,
    }
}

fn format_validation_errors(errors: &[ValidationError]) -> String {
    let mut parts = Vec::new();
    for err in errors {
        if let Some(field) = err.field.as_ref() {
            parts.push(format!("{field}: {}", err.message));
        } else {
            parts.push(err.message.clone());
        }
    }
    parts.join("; ")
}

fn done_result(observation: &Observation) -> StepResult {
    StepResult {
        ok: true,
        error: None,
        new_state_hash: Some(observation.state_hash.clone()),
        scroll: None,
        extract: None,
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserResult;
    use crate::types::{ElementFlags, ElementRef};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use tokio::sync::Mutex;

    #[derive(Debug)]
    struct FakeBrowser {
        snapshots: Mutex<VecDeque<Observation>>,
        apply_results: Mutex<VecDeque<StepResult>>,
        applied_actions: Mutex<Vec<Action>>,
    }

    impl FakeBrowser {
        fn new(
            snapshots: Vec<Observation>,
            apply_results: Vec<StepResult>,
        ) -> Self {
            Self {
                snapshots: Mutex::new(VecDeque::from(snapshots)),
                apply_results: Mutex::new(VecDeque::from(apply_results)),
                applied_actions: Mutex::new(Vec::new()),
            }
        }

        async fn applied(&self) -> Vec<Action> {
            self.applied_actions.lock().await.clone()
        }
    }

    #[async_trait]
    impl Browser for FakeBrowser {
        async fn snapshot(&self) -> BrowserResult<Observation> {
            let mut guard = self.snapshots.lock().await;
            guard
                .pop_front()
                .ok_or_else(|| BrowserError::new("missing_snapshot", "no snapshot queued"))
        }

        async fn apply(&self, action: &Action) -> BrowserResult<StepResult> {
            self.applied_actions.lock().await.push(action.clone());
            let mut guard = self.apply_results.lock().await;
            guard
                .pop_front()
                .ok_or_else(|| BrowserError::new("missing_result", "no apply result queued"))
        }

        async fn shutdown(&self) -> BrowserResult<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ScriptedLlm {
        actions: Mutex<VecDeque<Action>>,
    }

    impl ScriptedLlm {
        fn new(actions: Vec<Action>) -> Self {
            Self {
                actions: Mutex::new(VecDeque::from(actions)),
            }
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlm {
        async fn propose_action(
            &self,
            _task: &str,
            _plan: Option<&str>,
            _observation: &Observation,
            _observations: &VecDeque<Observation>,
            _history: &[Action],
        ) -> Result<Action, LlmError> {
            let mut guard = self.actions.lock().await;
            guard
                .pop_front()
                .ok_or_else(|| LlmError::new("no_action", "no action queued"))
        }
    }

    fn sample_observation(_id: &str, hash: &str, elements: Vec<ElementRef>) -> Observation {
        Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            state_hash: hash.to_string(),
            elements,
        }
    }

    fn element(id: &str) -> ElementRef {
        ElementRef {
            id: id.to_string(),
            role: "button".to_string(),
            name: Some("Submit".to_string()),
            value: None,
            bbox: [0.0, 0.0, 10.0, 10.0],
            flags: ElementFlags::default(),
        }
    }

    #[tokio::test]
    async fn stops_on_done_without_applying_action() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(vec![obs.clone()], vec![]);
        let llm = ScriptedLlm::new(vec![Action::Done {
            summary: "ok".to_string(),
        }]);
        let clients = LlmClients::new(Box::new(llm), Box::new(ScriptedLlm::new(vec![])), Box::new(ScriptedLlm::new(vec![])));
        let mut agent = AgentLoop::new(browser, clients, "task");

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(
            result.final_action,
            Action::Done {
                summary: "ok".to_string()
            }
        );
        assert_eq!(result.steps.len(), 1);
        assert_eq!(agent.memory().history().len(), 1);
        assert!(agent.memory().history()[0].matches_done());
        assert!(agent.memory().steps()[0].result.ok);
        assert!(agent.memory().observations().len() == 1);
        let applied = agent.browser.applied().await;
        assert!(applied.is_empty());
    }

    #[tokio::test]
    async fn rejects_invalid_action_without_applying() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(vec![obs.clone()], vec![]);
        let llm = ScriptedLlm::new(vec![
            Action::Click {
                id: "missing".to_string(),
            },
            Action::Done {
                summary: "done".to_string(),
            },
        ]);
        let clients = LlmClients::new(Box::new(llm), Box::new(ScriptedLlm::new(vec![])), Box::new(ScriptedLlm::new(vec![])));
        let mut agent = AgentLoop::new(browser, clients, "task");

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(agent.memory().history().len(), 2);
        assert_eq!(agent.memory().steps().len(), 2);
        assert_eq!(agent.memory().steps()[0].result.ok, false);
        assert_eq!(
            agent.memory().steps()[0]
                .result
                .error
                .as_ref()
                .unwrap()
                .code,
            "invalid_action"
        );
        assert_eq!(
            agent.memory().steps()[0]
                .result
                .error
                .as_ref()
                .unwrap()
                .validation_code
                .as_deref(),
            Some("unknown_id")
        );
        let applied = agent.browser.applied().await;
        assert!(applied.is_empty());
    }

    #[tokio::test]
    async fn stops_after_max_steps() {
        let obs_a = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let obs_b = sample_observation("obs2", "hash2", vec![element("el_1")]);
        let obs_c = sample_observation("obs3", "hash3", vec![element("el_1")]);
        let browser = FakeBrowser::new(
            vec![obs_a.clone(), obs_b.clone(), obs_c.clone()],
            vec![
                StepResult {
                    ok: true,
                    error: None,
                    new_state_hash: None,
                    scroll: None,
                    extract: None,
                },
                StepResult {
                    ok: true,
                    error: None,
                    new_state_hash: None,
                    scroll: None,
                    extract: None,
                },
            ],
        );
        let llm = ScriptedLlm::new(vec![
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_1".to_string(),
            },
        ]);
        let clients = LlmClients::new(Box::new(llm), Box::new(ScriptedLlm::new(vec![])), Box::new(ScriptedLlm::new(vec![])));
        let mut agent = AgentLoop::new(browser, clients, "task")
            .with_policy(AgentPolicy {
                max_steps: 2,
                ..AgentPolicy::default()
            });

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::MaxSteps);
        assert_eq!(
            result.final_action,
            Action::Done {
                summary: "max_steps reached".to_string()
            }
        );
        assert_eq!(agent.memory().history().len(), 2);
        let applied = agent.browser.applied().await;
        assert_eq!(applied.len(), 2);
    }

    #[test]
    fn step_outcome_marks_no_progress_when_state_hash_unchanged() {
        let prev = sample_observation("obs1", "hash1", vec![element("el_1"), element("el_2")]);
        let next = sample_observation("obs2", "hash1", vec![element("el_3")]);
        let result = StepResult {
            ok: true,
            error: None,
            new_state_hash: None,
            scroll: None,
            extract: None,
        };

        let (outcome, heuristics) = step_outcome(&result, &prev, &next);
        assert_eq!(outcome, StepOutcome::NoProgress);
        assert!(heuristics.state_hash_unchanged);
    }

    #[test]
    fn step_outcome_reports_progress_when_state_hash_changes() {
        let prev = sample_observation(
            "obs1",
            "hash1",
            vec![element("el_1"), element("el_2"), element("el_3")],
        );
        let next = sample_observation(
            "obs2",
            "hash2",
            vec![element("el_4"), element("el_5"), element("el_6")],
        );
        let result = StepResult {
            ok: true,
            error: None,
            new_state_hash: None,
            scroll: None,
            extract: None,
        };

        let (outcome, heuristics) = step_outcome(&result, &prev, &next);
        assert_eq!(outcome, StepOutcome::Progress);
        assert!(!heuristics.state_hash_unchanged);
    }

    trait ActionMatch {
        fn matches_done(&self) -> bool;
    }

    impl ActionMatch for Action {
        fn matches_done(&self) -> bool {
            matches!(self, Action::Done { .. })
        }
    }
}
