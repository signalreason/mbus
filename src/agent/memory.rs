use crate::types::{Action, Observation, StepResult};
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

#[derive(Clone, Debug, PartialEq)]
pub struct StepRecord {
    pub action: Action,
    pub result: StepResult,
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

    pub fn record_step(&mut self, action: Action, result: StepResult) {
        self.history.push(action.clone());
        self.steps.push(StepRecord { action, result });
        if self.history.len() > self.config.max_history {
            let drain = self.history.len() - self.config.max_history;
            self.history.drain(0..drain);
        }
        if self.steps.len() > self.config.max_history {
            let drain = self.steps.len() - self.config.max_history;
            self.steps.drain(0..drain);
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
