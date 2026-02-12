use crate::types::{Action, Observation, StepResult, TokenUsage};
use crate::verify::rules::ValidationError;
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct MemoryConfig {
    pub max_observations: usize,
    pub max_history: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_observations: 8,
            max_history: 100,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValidationOutcome {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ValidationError>,
}

impl ValidationOutcome {
    pub fn success() -> Self {
        Self {
            ok: true,
            errors: Vec::new(),
        }
    }

    pub fn failure(errors: Vec<ValidationError>) -> Self {
        Self { ok: false, errors }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StepTimings {
    pub step_duration_ms: u64,
    pub llm_duration_ms: u64,
    pub apply_duration_ms: u64,
    pub snapshot_duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcomeLog {
    Done,
    ValidationFailed,
    ApplyFailed,
    NoProgress,
    Progress,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StepRecord {
    pub action: Action,
    pub validation: ValidationOutcome,
    pub result: StepResult,
    pub outcome: StepOutcomeLog,
    pub timings: StepTimings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_usage: Option<TokenUsage>,
}

#[derive(Clone, Debug)]
pub struct Memory {
    config: MemoryConfig,
    plan: Option<String>,
    observations: VecDeque<Observation>,
    history: Vec<Action>,
    steps: Vec<StepRecord>,
}

impl Memory {
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            config,
            plan: None,
            observations: VecDeque::new(),
            history: Vec::new(),
            steps: Vec::new(),
        }
    }

    pub fn set_plan(&mut self, plan: Option<String>) {
        self.plan = plan;
    }

    pub fn plan(&self) -> Option<&str> {
        self.plan.as_deref()
    }

    pub fn record_observation(&mut self, observation: Observation) {
        self.observations.push_back(observation);
        while self.observations.len() > self.config.max_observations {
            self.observations.pop_front();
        }
    }

    pub fn record_step(&mut self, record: StepRecord) {
        self.history.push(record.action.clone());
        self.steps.push(record);
        if self.history.len() > self.config.max_history {
            let drain = self.history.len() - self.config.max_history;
            self.history.drain(0..drain);
        }
        if self.steps.len() > self.config.max_history {
            let drain = self.steps.len() - self.config.max_history;
            self.steps.drain(0..drain);
        }
    }

    pub fn update_last_step_state_hash(&mut self, new_state_hash: String) {
        if let Some(step) = self.steps.last_mut() {
            step.result.new_state_hash = Some(new_state_hash);
        }
    }

    pub fn history(&self) -> &[Action] {
        &self.history
    }

    pub fn steps(&self) -> &[StepRecord] {
        &self.steps
    }

    pub fn observations(&self) -> &VecDeque<Observation> {
        &self.observations
    }
}
