use crate::agent::memory::{Memory, StepOutcomeLog, StepRecord, StepTimings, ValidationOutcome};
use crate::agent::policy::AgentPolicy;
use crate::browser::{Browser, BrowserError, ScreenshotCapture};
use crate::llm::client::{LlmClient, LlmContext, LlmError};
use crate::llm::router::{
    LadderPolicyInput, Router, RouterTransition, StepOutcome, Tier, ladder_transition_policy,
    step_outcome,
};
use crate::output::sha256_hex;
use crate::telemetry;
use crate::types::{
    Action, Observation, SCREENSHOT_MIME_TYPE, ScreenshotMetadata, StepDiagnostic, StepError,
    StepResult,
};
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
    NoProgress,
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub status: RunStatus,
    pub final_action: Action,
    pub steps: Vec<StepRecord>,
    pub final_observation: Observation,
    pub step_screenshots: Vec<Option<Vec<u8>>>,
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

#[derive(Debug)]
struct LoopState {
    last_state_hash: String,
    state_hash_streak: u32,
    last_validation_code: Option<String>,
    last_validation_state_hash: Option<String>,
    validation_code_streak: u32,
}

const REPEAT_NO_PROGRESS_VALIDATION_LIMIT: u32 = 3;

impl LoopState {
    fn new(initial_hash: String) -> Self {
        Self {
            last_state_hash: initial_hash,
            state_hash_streak: 0,
            last_validation_code: None,
            last_validation_state_hash: None,
            validation_code_streak: 0,
        }
    }

    fn update_hash(&mut self, new_hash: &str) -> u32 {
        if new_hash == self.last_state_hash {
            self.state_hash_streak = self.state_hash_streak.saturating_add(1);
        } else {
            self.state_hash_streak = 0;
        }
        self.last_state_hash = new_hash.to_string();
        self.state_hash_streak
    }

    fn record_validation_failure(&mut self, state_hash: &str, code: Option<&str>) -> u32 {
        let code = match code {
            Some(code) => code,
            None => {
                self.reset_validation_streak();
                return 0;
            }
        };
        if self.last_validation_state_hash.as_deref() == Some(state_hash)
            && self.last_validation_code.as_deref() == Some(code)
        {
            self.validation_code_streak = self.validation_code_streak.saturating_add(1);
        } else {
            self.validation_code_streak = 1;
        }
        self.last_validation_state_hash = Some(state_hash.to_string());
        self.last_validation_code = Some(code.to_string());
        self.validation_code_streak
    }

    fn reset_validation_streak(&mut self) {
        self.validation_code_streak = 0;
        self.last_validation_code = None;
        self.last_validation_state_hash = None;
    }
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
            Done(Action, RunStatus),
        }

        let mut step_screenshots: Vec<Option<Vec<u8>>> = Vec::new();
        let snapshot_start = Instant::now();
        let initial_tier = self.router.active_tier();
        let snapshot_span = tracing::info_span!(
            "step.snapshot",
            step_index = 0,
            tier = ?initial_tier
        );
        let mut observation = match self.browser.snapshot().instrument(snapshot_span).await {
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
        let (mut observation_screenshot, mut observation_diagnostics) =
            take_screenshot_capture(&self.browser).await;
        observation.screenshot = screenshot_metadata_from_bytes(observation_screenshot.as_deref());
        self.memory.record_observation(observation.clone());
        let mut loop_state = LoopState::new(observation.state_hash.clone());

        for step_index in 0..self.policy.max_steps {
            let tier = self.router.active_tier();
            let step_number = step_index + 1;
            telemetry::inc_step();

            let step_span = tracing::info_span!(
                "step",
                step_index = step_number,
                tier = ?tier,
                url = %observation.url,
                state_hash = ?observation.state_hash
            );

            let step_result = async {
                let step_start = Instant::now();
                let client = self.clients.client(tier);

                let llm_start = Instant::now();
                let llm_span = tracing::info_span!(
                    "step.llm",
                    step_index = step_number,
                    tier = ?tier
                );
                let context = LlmContext {
                    task: &self.task,
                    plan: self.memory.plan(),
                    observation: &observation,
                    observations: self.memory.observations(),
                    observation_screenshot: observation_screenshot.as_deref(),
                    history: self.memory.history(),
                    steps: self.memory.steps(),
                    reasoning_effort: self.router.effort(),
                };
                let llm_response = match client.propose_action(&context).instrument(llm_span).await
                {
                    Ok(response) => response,
                    Err(err) => {
                        let duration = step_start.elapsed();
                        telemetry::record_step_duration(duration);
                        telemetry::record_apply_duration(Duration::from_millis(0));
                        telemetry::record_snapshot_duration(Duration::from_millis(0));
                        if is_recoverable_llm_error(&err) {
                            let tier_after = self.router.record(StepOutcome::Failure);
                            telemetry::set_no_progress_streak(self.router.counters().no_progress);
                            tracing::warn!(
                                event = "llm_error_recoverable",
                                error_code = err.code,
                                error_message_len = err.message.chars().count(),
                                tier = ?tier_after,
                                step_duration_ms = duration.as_millis() as u64
                            );
                            return Ok(StepControl::Continue);
                        }
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
                let action = llm_response.action;
                let llm_usage = llm_response.usage;
                let llm_payload_mode = llm_response.payload_mode;

                telemetry::inc_action(&action);
                tracing::info!(
                    event = "action_proposed",
                    action_type = telemetry::action_type(&action),
                    action = ?telemetry::ActionSummary::from(&action)
                );

                let validation_span = tracing::info_span!(
                    "step.validation",
                    step_index = step_number,
                    tier = ?tier
                );
                let validation_check = validation_span.in_scope(|| {
                    if let Some(error) = repeated_no_progress_validation_error(
                        self.memory.steps(),
                        &observation,
                        &action,
                    ) {
                        Err(vec![error])
                    } else {
                        self.validator.validate(&action, &observation)
                    }
                });
                if let Err(errors) = validation_check {
                    let has_repeat_no_progress_error = errors
                        .iter()
                        .any(|error| error.code == "repeat_no_progress_action");
                    let error_count = errors.len();
                    let validation_code = errors.first().map(|error| error.code.clone());
                    let validation_streak = loop_state.record_validation_failure(
                        observation.state_hash.as_str(),
                        errors.first().map(|error| error.code.as_str()),
                    );
                    telemetry::inc_validation_failure();
                    let mut result = validation_result(errors.clone());
                    result.new_state_hash = Some(observation.state_hash.clone());
                    result.diagnostics = observation_diagnostics.clone();
                    let duration = step_start.elapsed();
                    telemetry::record_apply_duration(Duration::from_millis(0));
                    telemetry::record_snapshot_duration(Duration::from_millis(0));
                    self.memory.record_step(StepRecord {
                        action,
                        validation: ValidationOutcome::failure(errors),
                        result,
                        outcome: StepOutcomeLog::ValidationFailed,
                        timings: StepTimings {
                            step_duration_ms: duration_ms(duration),
                            llm_duration_ms: duration_ms(llm_duration),
                            apply_duration_ms: 0,
                            snapshot_duration_ms: 0,
                        },
                        llm_payload_mode,
                        llm_usage,
                    });
                    step_screenshots.push(observation_screenshot.clone());
                    let tier_after = self.router.record(StepOutcome::Failure);
                    if let Some(transition) = self.apply_ladder_policy(
                        loop_state.state_hash_streak,
                        validation_streak,
                        validation_code.as_deref(),
                    ) {
                        log_router_transition(&transition);
                    }
                    telemetry::set_no_progress_streak(self.router.counters().no_progress);
                    telemetry::record_step_duration(duration);
                    tracing::warn!(
                        event = "validation_failed",
                        error_count = error_count,
                        tier = ?tier_after,
                        step_duration_ms = duration.as_millis() as u64
                    );
                    if has_repeat_no_progress_error
                        && validation_streak >= REPEAT_NO_PROGRESS_VALIDATION_LIMIT
                    {
                        let final_action = Action::Done {
                            summary: format!(
                                "no progress after {} repeated blocked actions",
                                validation_streak
                            ),
                        };
                        tracing::warn!(
                            event = "repeat_no_progress_termination",
                            repeat_no_progress_validation_streak = validation_streak,
                            state_hash = %observation.state_hash,
                            limit = REPEAT_NO_PROGRESS_VALIDATION_LIMIT
                        );
                        return Ok(StepControl::Done(final_action, RunStatus::NoProgress));
                    }
                    return Ok(StepControl::Continue);
                }

                if matches!(action, Action::Done { .. }) {
                    loop_state.reset_validation_streak();
                    let mut result = done_result(&observation);
                    result.diagnostics = observation_diagnostics.clone();
                    let duration = step_start.elapsed();
                    telemetry::record_apply_duration(Duration::from_millis(0));
                    telemetry::record_snapshot_duration(Duration::from_millis(0));
                    self.memory.record_step(StepRecord {
                        action: action.clone(),
                        validation: ValidationOutcome::success(),
                        result,
                        outcome: StepOutcomeLog::Done,
                        timings: StepTimings {
                            step_duration_ms: duration_ms(duration),
                            llm_duration_ms: duration_ms(llm_duration),
                            apply_duration_ms: 0,
                            snapshot_duration_ms: 0,
                        },
                        llm_payload_mode,
                        llm_usage,
                    });
                    step_screenshots.push(observation_screenshot.clone());
                    telemetry::record_step_duration(duration);
                    tracing::info!(
                        event = "done",
                        step_duration_ms = duration.as_millis() as u64
                    );
                    return Ok(StepControl::Done(action, RunStatus::Done));
                }

                let apply_start = Instant::now();
                loop_state.reset_validation_streak();
                let apply_span = tracing::info_span!(
                    "step.apply",
                    step_index = step_number,
                    tier = ?tier
                );
                let mut result = match self.browser.apply(&action).instrument(apply_span).await {
                    Ok(result) => result,
                    Err(err) => {
                        let apply_duration = apply_start.elapsed();
                        telemetry::record_apply_duration(apply_duration);
                        let duration = step_start.elapsed();
                        telemetry::record_step_duration(duration);
                        telemetry::record_snapshot_duration(Duration::from_millis(0));
                        tracing::error!(
                            event = "apply_error",
                            error_code = err.code,
                            error_message = %err.message,
                            step_duration_ms = duration.as_millis() as u64
                        );
                        return Err(AgentError::Browser(err));
                    }
                };
                result.diagnostics = observation_diagnostics.clone();
                let apply_duration = apply_start.elapsed();
                telemetry::record_apply_duration(apply_duration);
                if !result.ok {
                    telemetry::inc_apply_failure();
                }

                let snapshot_start = Instant::now();
                let snapshot_span = tracing::info_span!(
                    "step.snapshot",
                    step_index = step_number,
                    tier = ?tier
                );
                let mut next_observation =
                    match self.browser.snapshot().instrument(snapshot_span).await {
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
                let (next_screenshot, next_diagnostics) =
                    take_screenshot_capture(&self.browser).await;
                next_observation.screenshot =
                    screenshot_metadata_from_bytes(next_screenshot.as_deref());
                self.memory.record_observation(next_observation.clone());
                let new_hash = next_observation.state_hash.clone();
                result.new_state_hash = Some(new_hash.clone());
                let state_hash_streak = loop_state.update_hash(&new_hash);

                let (outcome, heuristics) =
                    step_outcome(&result, &observation, &next_observation, state_hash_streak);
                let log_outcome = match outcome {
                    StepOutcome::Failure => StepOutcomeLog::ApplyFailed,
                    StepOutcome::NoProgress => StepOutcomeLog::NoProgress,
                    StepOutcome::Progress => StepOutcomeLog::Progress,
                };

                let duration = step_start.elapsed();
                self.memory.record_step(StepRecord {
                    action: action.clone(),
                    validation: ValidationOutcome::success(),
                    result: result.clone(),
                    outcome: log_outcome,
                    timings: StepTimings {
                        step_duration_ms: duration_ms(duration),
                        llm_duration_ms: duration_ms(llm_duration),
                        apply_duration_ms: duration_ms(apply_duration),
                        snapshot_duration_ms: duration_ms(snapshot_duration),
                    },
                    llm_payload_mode,
                    llm_usage,
                });
                step_screenshots.push(observation_screenshot.clone());
                self.memory.update_last_step_state_hash(new_hash);

                let tier_after = self
                    .router
                    .record_with_heuristics(outcome, Some(&heuristics));
                if let Some(transition) = self.apply_ladder_policy(state_hash_streak, 0, None) {
                    log_router_transition(&transition);
                }
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
                    state_hash_streak = state_hash_streak,
                    actionables_unchanged = heuristics.actionables_unchanged,
                    low_actionability = heuristics.low_actionability,
                    actionability_score = heuristics.actionability_score,
                    too_few_actionables = heuristics.too_few_actionables,
                    prev_actionables = heuristics.prev_actionables,
                    next_actionables = heuristics.next_actionables
                );

                observation = next_observation;
                observation_screenshot = next_screenshot;
                observation_diagnostics = next_diagnostics;

                if self.policy.max_no_progress_steps > 0
                    && state_hash_streak as usize >= self.policy.max_no_progress_steps
                {
                    let final_action = Action::Done {
                        summary: format!("no progress after {} steps", state_hash_streak),
                    };
                    tracing::warn!(
                        event = "no_progress_termination",
                        state_hash_streak = state_hash_streak,
                        max_no_progress_steps = self.policy.max_no_progress_steps
                    );
                    return Ok(StepControl::Done(final_action, RunStatus::NoProgress));
                }

                Ok(StepControl::Continue)
            }
            .instrument(step_span)
            .await?;

            if let StepControl::Done(action, status) = step_result {
                return Ok(RunResult {
                    status,
                    final_action: action,
                    steps: self.memory.steps().to_vec(),
                    final_observation: observation,
                    step_screenshots,
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
            step_screenshots,
        })
    }
}

fn log_router_transition(transition: &RouterTransition) {
    tracing::info!(
        event = "router_transition",
        reason_code = transition.reason.code(),
        model = %transition.step.model,
        effort = ?transition.step.effort,
        tier = ?transition.step.tier,
        ladder_index = transition.index
    );
}

impl<B: Browser> AgentLoop<B> {
    fn apply_ladder_policy(
        &mut self,
        state_hash_streak: u32,
        validation_code_streak: u32,
        validation_code: Option<&str>,
    ) -> Option<RouterTransition> {
        let ladder_len = self.router.ladder().len();
        let decision = ladder_transition_policy(LadderPolicyInput {
            current_index: self.router.ladder_index(),
            ladder_len,
            state_hash_streak,
            validation_code_streak,
            validation_code,
        })?;
        self.router.apply_ladder_transition(decision)
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
        diagnostics: Vec::new(),
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
        diagnostics: Vec::new(),
        new_state_hash: Some(observation.state_hash.clone()),
        scroll: None,
        extract: None,
    }
}

fn screenshot_metadata_from_bytes(bytes: Option<&[u8]>) -> Option<ScreenshotMetadata> {
    let bytes = bytes?;
    Some(ScreenshotMetadata {
        mime_type: SCREENSHOT_MIME_TYPE.to_string(),
        artifact_ref: None,
        sha256: sha256_hex(bytes),
        bytes: bytes.len(),
    })
}

async fn take_screenshot_capture<B: Browser>(
    browser: &B,
) -> (Option<Vec<u8>>, Vec<StepDiagnostic>) {
    match browser.take_last_screenshot().await {
        Ok(ScreenshotCapture { bytes, error }) => {
            let mut diagnostics = Vec::new();
            if let Some(err) = error {
                diagnostics.push(StepDiagnostic {
                    code: err.code.to_string(),
                    message: err.message,
                });
            }
            (bytes, diagnostics)
        }
        Err(err) => (
            None,
            vec![StepDiagnostic {
                code: err.code.to_string(),
                message: err.message,
            }],
        ),
    }
}

fn repeated_no_progress_validation_error(
    steps: &[StepRecord],
    observation: &Observation,
    action: &Action,
) -> Option<ValidationError> {
    let hash = observation.state_hash.as_str();
    for step in steps.iter().rev() {
        if step.result.new_state_hash.as_deref() != Some(hash) {
            break;
        }
        if !matches!(
            step.outcome,
            StepOutcomeLog::NoProgress | StepOutcomeLog::ValidationFailed
        ) {
            continue;
        }
        if step.action == *action {
            return Some(ValidationError {
                code: "repeat_no_progress_action".to_string(),
                field: None,
                message: "action already attempted in unchanged state; choose a different action"
                    .to_string(),
            });
        }
    }
    None
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

fn is_recoverable_llm_error(err: &LlmError) -> bool {
    err.code == "empty_response"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserResult;
    use crate::llm::client::LlmResponse;
    use crate::types::{ElementFlags, ElementRef, LlmPayloadMode, ReasoningEffort};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::sync::Mutex;

    #[derive(Debug)]
    struct FakeBrowser {
        snapshots: Mutex<VecDeque<Observation>>,
        screenshots: Mutex<VecDeque<ScreenshotCapture>>,
        apply_results: Mutex<VecDeque<StepResult>>,
        applied_actions: Mutex<Vec<Action>>,
    }

    impl FakeBrowser {
        fn new(snapshots: Vec<Observation>, apply_results: Vec<StepResult>) -> Self {
            let screenshot_count = snapshots.len();
            Self::new_with_screenshots(snapshots, apply_results, vec![None; screenshot_count])
        }

        fn new_with_screenshots(
            snapshots: Vec<Observation>,
            apply_results: Vec<StepResult>,
            screenshots: Vec<Option<Vec<u8>>>,
        ) -> Self {
            let captures = screenshots
                .into_iter()
                .map(|bytes| ScreenshotCapture { bytes, error: None })
                .collect();
            Self::new_with_screenshot_captures(snapshots, apply_results, captures)
        }

        fn new_with_screenshot_captures(
            snapshots: Vec<Observation>,
            apply_results: Vec<StepResult>,
            screenshots: Vec<ScreenshotCapture>,
        ) -> Self {
            Self {
                snapshots: Mutex::new(VecDeque::from(snapshots)),
                screenshots: Mutex::new(VecDeque::from(screenshots)),
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

        async fn take_last_screenshot(&self) -> BrowserResult<ScreenshotCapture> {
            Ok(self
                .screenshots
                .lock()
                .await
                .pop_front()
                .unwrap_or_default())
        }
    }

    #[derive(Debug)]
    struct CaptureEffortLlm {
        captured: Arc<StdMutex<Option<ReasoningEffort>>>,
    }

    impl CaptureEffortLlm {
        fn new(captured: Arc<StdMutex<Option<ReasoningEffort>>>) -> Self {
            Self { captured }
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

    #[derive(Debug)]
    struct ScriptedLlmResponses {
        responses: Mutex<VecDeque<Result<Action, LlmError>>>,
    }

    impl ScriptedLlmResponses {
        fn new(responses: Vec<Result<Action, LlmError>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmClient for CaptureEffortLlm {
        async fn propose_action(&self, context: &LlmContext<'_>) -> Result<LlmResponse, LlmError> {
            let mut guard = self.captured.lock().expect("capture lock");
            *guard = Some(context.reasoning_effort);
            Ok(LlmResponse {
                action: Action::Done {
                    summary: "ok".to_string(),
                },
                usage: None,
                payload_mode: LlmPayloadMode::TextOnly,
            })
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlm {
        async fn propose_action(&self, _context: &LlmContext<'_>) -> Result<LlmResponse, LlmError> {
            let mut guard = self.actions.lock().await;
            let action = guard
                .pop_front()
                .ok_or_else(|| LlmError::new("no_action", "no action queued"))?;
            Ok(LlmResponse {
                action,
                usage: None,
                payload_mode: LlmPayloadMode::TextOnly,
            })
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlmResponses {
        async fn propose_action(&self, _context: &LlmContext<'_>) -> Result<LlmResponse, LlmError> {
            let mut guard = self.responses.lock().await;
            let response = guard
                .pop_front()
                .ok_or_else(|| LlmError::new("no_action", "no response queued"))?;
            response.map(|action| LlmResponse {
                action,
                usage: None,
                payload_mode: LlmPayloadMode::TextOnly,
            })
        }
    }

    fn sample_observation(_id: &str, hash: &str, elements: Vec<ElementRef>) -> Observation {
        Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            screenshot: None,
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
        let browser =
            FakeBrowser::new_with_screenshots(vec![obs.clone()], vec![], vec![Some(vec![1, 2, 3])]);
        let llm = ScriptedLlm::new(vec![Action::Done {
            summary: "ok".to_string(),
        }]);
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 10,
            failures_to_strong: 20,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: crate::types::ReasoningEffort::Medium,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task").with_router(router);

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
        assert_eq!(result.step_screenshots, vec![Some(vec![1, 2, 3])]);
        let applied = agent.browser.applied().await;
        assert!(applied.is_empty());
    }

    #[tokio::test]
    async fn passes_router_reasoning_effort_into_llm_context() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(vec![obs.clone()], vec![]);
        let captured = Arc::new(StdMutex::new(None));
        let client = CaptureEffortLlm::new(captured.clone());
        let clients = LlmClients::new(
            Box::new(CaptureEffortLlm::new(captured.clone())),
            Box::new(CaptureEffortLlm::new(captured.clone())),
            Box::new(client),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 10,
            failures_to_strong: 20,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: ReasoningEffort::High,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task").with_router(router);

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        let effort = captured.lock().expect("capture lock");
        assert_eq!(*effort, Some(ReasoningEffort::High));
    }

    #[tokio::test]
    async fn tracks_observation_screenshots_for_each_step() {
        let obs_a = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let obs_b = sample_observation("obs2", "hash2", vec![element("el_1")]);
        let browser = FakeBrowser::new_with_screenshots(
            vec![obs_a.clone(), obs_b.clone()],
            vec![StepResult {
                ok: true,
                error: None,
                diagnostics: Vec::new(),
                new_state_hash: None,
                scroll: None,
                extract: None,
            }],
            vec![Some(vec![7]), Some(vec![8])],
        );
        let llm = ScriptedLlm::new(vec![
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Done {
                summary: "done".to_string(),
            },
        ]);
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 10,
            failures_to_strong: 20,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: crate::types::ReasoningEffort::Medium,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task").with_router(router);

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.step_screenshots, vec![Some(vec![7]), Some(vec![8])]);
    }

    #[tokio::test]
    async fn reuses_same_observation_screenshot_after_validation_failure() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let browser =
            FakeBrowser::new_with_screenshots(vec![obs.clone()], vec![], vec![Some(vec![9])]);
        let llm = ScriptedLlm::new(vec![
            Action::Click {
                id: "missing".to_string(),
            },
            Action::Done {
                summary: "done".to_string(),
            },
        ]);
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let mut agent = AgentLoop::new(browser, clients, "task");

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.step_screenshots, vec![Some(vec![9]), Some(vec![9])]);
    }

    #[tokio::test]
    async fn records_screenshot_failure_diagnostics() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let capture = ScreenshotCapture {
            bytes: None,
            error: Some(BrowserError::new("screenshot_failed", "capture failed")),
        };
        let browser =
            FakeBrowser::new_with_screenshot_captures(vec![obs.clone()], vec![], vec![capture]);
        let llm = ScriptedLlm::new(vec![Action::Done {
            summary: "done".to_string(),
        }]);
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let mut agent = AgentLoop::new(browser, clients, "task");

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        let diagnostics = &agent.memory().steps()[0].result.diagnostics;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "screenshot_failed");
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
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let mut agent = AgentLoop::new(browser, clients, "task");

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(agent.memory().history().len(), 2);
        assert_eq!(agent.memory().steps().len(), 2);
        assert!(!agent.memory().steps()[0].result.ok);
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
    async fn empty_response_error_escalates_and_recovers() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(vec![obs.clone()], vec![]);
        let clients = LlmClients::new(
            Box::new(ScriptedLlmResponses::new(vec![Err(LlmError::new(
                "empty_response",
                "missing content text",
            ))])),
            Box::new(ScriptedLlmResponses::new(vec![Ok(Action::Done {
                summary: "recovered".to_string(),
            })])),
            Box::new(ScriptedLlmResponses::new(vec![])),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 1,
            failures_to_strong: 3,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: crate::types::ReasoningEffort::Medium,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task")
            .with_router(router)
            .with_policy(AgentPolicy {
                max_steps: 3,
                ..AgentPolicy::default()
            });

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(
            result.final_action,
            Action::Done {
                summary: "recovered".to_string()
            }
        );
        assert_eq!(agent.memory().steps().len(), 1);
        assert_eq!(agent.memory().history().len(), 1);
        let applied = agent.browser.applied().await;
        assert!(applied.is_empty());
    }

    #[tokio::test]
    async fn non_recoverable_llm_error_still_terminates() {
        let obs = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(vec![obs.clone()], vec![]);
        let clients = LlmClients::new(
            Box::new(ScriptedLlmResponses::new(vec![Err(LlmError::new(
                "timeout",
                "request timed out",
            ))])),
            Box::new(ScriptedLlmResponses::new(vec![Ok(Action::Done {
                summary: "unused".to_string(),
            })])),
            Box::new(ScriptedLlmResponses::new(vec![])),
        );
        let mut agent = AgentLoop::new(browser, clients, "task");

        let err = agent.run().await.expect_err("expected timeout error");
        match err {
            AgentError::Llm(inner) => assert_eq!(inner.code, "timeout"),
            other => panic!("unexpected error type: {other}"),
        }
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
                    diagnostics: Vec::new(),
                    new_state_hash: None,
                    scroll: None,
                    extract: None,
                },
                StepResult {
                    ok: true,
                    error: None,
                    diagnostics: Vec::new(),
                    new_state_hash: None,
                    scroll: None,
                    extract: None,
                },
            ],
        );
        let actions = vec![
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_1".to_string(),
            },
        ];
        let clients = LlmClients::new(
            Box::new(ScriptedLlm::new(actions.clone())),
            Box::new(ScriptedLlm::new(actions.clone())),
            Box::new(ScriptedLlm::new(actions)),
        );
        let mut agent = AgentLoop::new(browser, clients, "task").with_policy(AgentPolicy {
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

    #[tokio::test]
    async fn stops_on_no_progress_streak() {
        let obs_a = sample_observation("obs1", "hash1", vec![element("el_1"), element("el_2")]);
        let obs_b = sample_observation("obs2", "hash1", vec![element("el_1"), element("el_2")]);
        let obs_c = sample_observation("obs3", "hash1", vec![element("el_1"), element("el_2")]);
        let browser = FakeBrowser::new(
            vec![obs_a.clone(), obs_b.clone(), obs_c.clone()],
            vec![
                StepResult {
                    ok: true,
                    error: None,
                    diagnostics: Vec::new(),
                    new_state_hash: None,
                    scroll: None,
                    extract: None,
                },
                StepResult {
                    ok: true,
                    error: None,
                    diagnostics: Vec::new(),
                    new_state_hash: None,
                    scroll: None,
                    extract: None,
                },
            ],
        );
        let actions = vec![
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_2".to_string(),
            },
        ];
        let clients = LlmClients::new(
            Box::new(ScriptedLlm::new(actions.clone())),
            Box::new(ScriptedLlm::new(actions.clone())),
            Box::new(ScriptedLlm::new(actions)),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 10,
            failures_to_strong: 20,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: crate::types::ReasoningEffort::Medium,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task")
            .with_router(router)
            .with_policy(AgentPolicy {
                max_steps: 5,
                max_no_progress_steps: 2,
                ..AgentPolicy::default()
            });

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::NoProgress);
        assert_eq!(
            result.final_action,
            Action::Done {
                summary: "no progress after 2 steps".to_string()
            }
        );
        assert_eq!(agent.memory().history().len(), 2);
        assert_eq!(agent.memory().steps().len(), 2);
        let applied = agent.browser.applied().await;
        assert_eq!(applied.len(), 2);
    }

    #[test]
    fn loop_state_validation_streak_tracks_same_code_and_hash() {
        let mut state = LoopState::new("hash1".to_string());
        assert_eq!(
            state.record_validation_failure("hash1", Some("repeat_no_progress_action")),
            1
        );
        assert_eq!(
            state.record_validation_failure("hash1", Some("repeat_no_progress_action")),
            2
        );
    }

    #[test]
    fn loop_state_validation_streak_resets_on_code_or_hash_change() {
        let mut state = LoopState::new("hash1".to_string());
        assert_eq!(
            state.record_validation_failure("hash1", Some("unknown_id")),
            1
        );
        assert_eq!(
            state.record_validation_failure("hash1", Some("repeat_no_progress_action")),
            1
        );
        assert_eq!(
            state.record_validation_failure("hash2", Some("repeat_no_progress_action")),
            1
        );
    }

    #[test]
    fn loop_state_validation_streak_resets_after_success() {
        let mut state = LoopState::new("hash1".to_string());
        assert_eq!(
            state.record_validation_failure("hash1", Some("repeat_no_progress_action")),
            1
        );
        state.reset_validation_streak();
        assert_eq!(
            state.record_validation_failure("hash1", Some("repeat_no_progress_action")),
            1
        );
    }

    #[test]
    fn step_outcome_marks_no_progress_when_state_hash_unchanged() {
        let prev = sample_observation("obs1", "hash1", vec![element("el_1"), element("el_2")]);
        let next = sample_observation("obs2", "hash1", vec![element("el_3")]);
        let result = StepResult {
            ok: true,
            error: None,
            diagnostics: Vec::new(),
            new_state_hash: None,
            scroll: None,
            extract: None,
        };

        let (outcome, heuristics) = step_outcome(&result, &prev, &next, 1);
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
            diagnostics: Vec::new(),
            new_state_hash: None,
            scroll: None,
            extract: None,
        };

        let (outcome, heuristics) = step_outcome(&result, &prev, &next, 0);
        assert_eq!(outcome, StepOutcome::Progress);
        assert!(!heuristics.state_hash_unchanged);
    }

    #[tokio::test]
    async fn rejects_repeated_action_after_no_progress_in_same_state() {
        let obs_a = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let obs_b = sample_observation("obs2", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(
            vec![obs_a.clone(), obs_b.clone()],
            vec![StepResult {
                ok: true,
                error: None,
                diagnostics: Vec::new(),
                new_state_hash: None,
                scroll: None,
                extract: None,
            }],
        );
        let llm = ScriptedLlm::new(vec![
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Done {
                summary: "done".to_string(),
            },
        ]);
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 10,
            failures_to_strong: 20,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: crate::types::ReasoningEffort::Medium,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task")
            .with_router(router)
            .with_policy(AgentPolicy {
                max_steps: 3,
                max_no_progress_steps: 8,
                ..AgentPolicy::default()
            });

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(agent.memory().steps().len(), 3);
        assert!(!agent.memory().steps()[1].validation.ok);
        assert_eq!(
            agent.memory().steps()[1]
                .result
                .error
                .as_ref()
                .expect("validation error")
                .validation_code
                .as_deref(),
            Some("repeat_no_progress_action")
        );
        let applied = agent.browser.applied().await;
        assert_eq!(applied.len(), 1);
    }

    #[tokio::test]
    async fn terminates_when_repeat_no_progress_validation_loops() {
        let obs_a = sample_observation("obs1", "hash1", vec![element("el_1")]);
        let obs_b = sample_observation("obs2", "hash1", vec![element("el_1")]);
        let browser = FakeBrowser::new(
            vec![obs_a.clone(), obs_b.clone()],
            vec![StepResult {
                ok: true,
                error: None,
                diagnostics: Vec::new(),
                new_state_hash: None,
                scroll: None,
                extract: None,
            }],
        );
        let llm = ScriptedLlm::new(vec![
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_1".to_string(),
            },
            Action::Click {
                id: "el_1".to_string(),
            },
        ]);
        let clients = LlmClients::new(
            Box::new(llm),
            Box::new(ScriptedLlm::new(vec![])),
            Box::new(ScriptedLlm::new(vec![])),
        );
        let router = Router::new(crate::llm::router::RouterConfig {
            failures_to_mid: 10,
            failures_to_strong: 20,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: crate::types::ReasoningEffort::Medium,
            ..crate::llm::router::RouterConfig::default()
        });
        let mut agent = AgentLoop::new(browser, clients, "task")
            .with_router(router)
            .with_policy(AgentPolicy {
                max_steps: 10,
                max_no_progress_steps: 8,
                ..AgentPolicy::default()
            });

        let result = agent.run().await.expect("run");
        assert_eq!(result.status, RunStatus::NoProgress);
        assert_eq!(
            result.final_action,
            Action::Done {
                summary: "no progress after 3 repeated blocked actions".to_string()
            }
        );
        assert_eq!(agent.memory().steps().len(), 4);
        let applied = agent.browser.applied().await;
        assert_eq!(applied.len(), 1);
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
