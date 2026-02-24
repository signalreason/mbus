use crate::types::{Observation, ReasoningEffort, StepResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Mid,
    Strong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Progress,
    Failure,
    NoProgress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouterCounters {
    pub failures: u32,
    pub no_progress: u32,
    pub low_actionability: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ProgressHeuristics {
    pub state_hash_unchanged: bool,
    pub actionables_unchanged: bool,
    pub low_actionability: bool,
    pub prev_actionables: usize,
    pub next_actionables: usize,
    pub actionability_score: f32,
    pub too_few_actionables: bool,
}

#[derive(Clone, Debug)]
pub struct RouterConfig {
    pub failures_to_mid: u32,
    pub failures_to_strong: u32,
    pub no_progress_to_mid: u32,
    pub no_progress_to_strong: u32,
    pub low_actionability_to_mid: u32,
    pub low_actionability_to_strong: u32,
    pub reasoning_effort: ReasoningEffort,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            failures_to_mid: 2,
            failures_to_strong: 4,
            no_progress_to_mid: 2,
            no_progress_to_strong: 4,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Router {
    config: RouterConfig,
    failures: u32,
    no_progress: u32,
    low_actionability: u32,
    reasoning_effort: ReasoningEffort,
}

impl Default for Router {
    fn default() -> Self {
        Self::new(RouterConfig::default())
    }
}

const LOW_ACTIONABILITY_COUNT_THRESHOLD: usize = 2;
const LOW_ACTIONABILITY_SCORE_THRESHOLD: f32 = 3.0;
const ACTIONABILITY_COUNT_WEIGHT: f32 = 0.35;
const ACTIONABILITY_COUNT_CAP: f32 = 8.0;

pub fn step_outcome(
    result: &StepResult,
    previous: &Observation,
    next: &Observation,
    state_hash_streak: u32,
) -> (StepOutcome, ProgressHeuristics) {
    let heuristics = evaluate_progress(previous, next);

    if !result.ok {
        return (StepOutcome::Failure, heuristics);
    }

    let outcome = if state_hash_streak > 0 {
        StepOutcome::NoProgress
    } else {
        StepOutcome::Progress
    };

    (outcome, heuristics)
}

impl Router {
    pub fn new(config: RouterConfig) -> Self {
        Self {
            reasoning_effort: config.reasoning_effort,
            config,
            failures: 0,
            no_progress: 0,
            low_actionability: 0,
        }
    }

    pub fn record(&mut self, outcome: StepOutcome) -> Tier {
        self.record_with_heuristics(outcome, None)
    }

    pub fn record_with_heuristics(
        &mut self,
        outcome: StepOutcome,
        heuristics: Option<&ProgressHeuristics>,
    ) -> Tier {
        match outcome {
            StepOutcome::Progress => {
                self.failures = 0;
                self.no_progress = 0;
            }
            StepOutcome::Failure => {
                self.failures = self.failures.saturating_add(1);
            }
            StepOutcome::NoProgress => {
                self.failures = 0;
                self.no_progress = self.no_progress.saturating_add(1);
            }
        }
        if let Some(heuristics) = heuristics {
            if heuristics.low_actionability {
                self.low_actionability = self.low_actionability.saturating_add(1);
            } else {
                self.low_actionability = 0;
            }
        } else {
            self.low_actionability = 0;
        }
        self.tier()
    }

    pub fn tier(&self) -> Tier {
        let failure_tier = tier_for_count(
            self.failures,
            self.config.failures_to_mid,
            self.config.failures_to_strong,
        );
        let no_progress_tier = tier_for_count(
            self.no_progress,
            self.config.no_progress_to_mid,
            self.config.no_progress_to_strong,
        );
        let low_actionability_tier = tier_for_count(
            self.low_actionability,
            self.config.low_actionability_to_mid,
            self.config.low_actionability_to_strong,
        );
        max_tier(
            max_tier(failure_tier, no_progress_tier),
            low_actionability_tier,
        )
    }

    pub fn effort(&self) -> ReasoningEffort {
        self.reasoning_effort
    }

    pub fn set_effort(&mut self, effort: ReasoningEffort) {
        self.reasoning_effort = effort;
    }

    pub fn counters(&self) -> RouterCounters {
        RouterCounters {
            failures: self.failures,
            no_progress: self.no_progress,
            low_actionability: self.low_actionability,
        }
    }

    pub fn reset(&mut self) {
        self.failures = 0;
        self.no_progress = 0;
        self.low_actionability = 0;
        self.reasoning_effort = self.config.reasoning_effort;
    }

    pub fn config(&self) -> &RouterConfig {
        &self.config
    }
}

fn tier_for_count(count: u32, mid: u32, strong: u32) -> Tier {
    if count >= strong {
        Tier::Strong
    } else if count >= mid {
        Tier::Mid
    } else {
        Tier::Fast
    }
}

fn max_tier(left: Tier, right: Tier) -> Tier {
    use Tier::{Fast, Mid, Strong};
    match (left, right) {
        (Strong, _) | (_, Strong) => Strong,
        (Mid, _) | (_, Mid) => Mid,
        _ => Fast,
    }
}

fn evaluate_progress(previous: &Observation, next: &Observation) -> ProgressHeuristics {
    let prev_actionables = previous.elements.len();
    let next_actionables = next.elements.len();
    let state_hash_unchanged = previous.state_hash == next.state_hash;
    let actionables_unchanged = actionable_signature(previous) == actionable_signature(next);
    let actionability_score = actionability_score(next);
    let too_few_actionables = next_actionables <= LOW_ACTIONABILITY_COUNT_THRESHOLD;
    let low_actionability =
        too_few_actionables || actionability_score < LOW_ACTIONABILITY_SCORE_THRESHOLD;

    ProgressHeuristics {
        state_hash_unchanged,
        actionables_unchanged,
        low_actionability,
        prev_actionables,
        next_actionables,
        actionability_score,
        too_few_actionables,
    }
}

fn actionable_signature(observation: &Observation) -> Vec<String> {
    let mut ids: Vec<String> = observation
        .elements
        .iter()
        .map(|element| element.id.clone())
        .collect();
    ids.sort();
    ids
}

fn actionability_score(observation: &Observation) -> f32 {
    if observation.elements.is_empty() {
        return 0.0;
    }
    let count = observation.elements.len() as f32;
    let count_score = count.min(ACTIONABILITY_COUNT_CAP) * ACTIONABILITY_COUNT_WEIGHT;
    let role_score: f32 = observation
        .elements
        .iter()
        .map(|element| role_weight(&element.role))
        .sum();
    role_score + count_score
}

fn role_weight(role: &str) -> f32 {
    match role {
        "textbox" | "searchbox" | "combobox" | "listbox" | "option" | "spinbutton" => 1.2,
        "button" | "link" | "menuitem" | "menuitemcheckbox" | "menuitemradio" | "tab" => 1.0,
        "checkbox" | "radio" | "switch" | "slider" => 0.8,
        _ => 0.6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ElementFlags, ElementRef, Observation, ReasoningEffort, StepResult};

    fn sample_observation(hash: &str) -> Observation {
        observation_with_elements(hash, Vec::new())
    }

    fn observation_with_elements(hash: &str, elements: Vec<ElementRef>) -> Observation {
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

    fn element(id: &str, role: &str) -> ElementRef {
        ElementRef {
            id: id.to_string(),
            role: role.to_string(),
            name: None,
            value: None,
            bbox: [0.0, 0.0, 0.0, 0.0],
            flags: ElementFlags::default(),
        }
    }

    fn sample_result(ok: bool) -> StepResult {
        StepResult {
            ok,
            error: None,
            diagnostics: Vec::new(),
            new_state_hash: None,
            scroll: None,
            extract: None,
        }
    }

    #[test]
    fn escalates_on_failure_thresholds() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 2,
            failures_to_strong: 4,
            no_progress_to_mid: 3,
            no_progress_to_strong: 5,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::Medium,
        });
        assert_eq!(router.tier(), Tier::Fast);
        assert_eq!(router.record(StepOutcome::Failure), Tier::Fast);
        assert_eq!(router.record(StepOutcome::Failure), Tier::Mid);
        assert_eq!(router.record(StepOutcome::Failure), Tier::Mid);
        assert_eq!(router.record(StepOutcome::Failure), Tier::Strong);
    }

    #[test]
    fn escalates_on_no_progress_thresholds() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 4,
            failures_to_strong: 6,
            no_progress_to_mid: 1,
            no_progress_to_strong: 2,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::Medium,
        });
        assert_eq!(router.record(StepOutcome::NoProgress), Tier::Mid);
        assert_eq!(router.record(StepOutcome::NoProgress), Tier::Strong);
    }

    #[test]
    fn uses_highest_tier_across_counters() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 2,
            failures_to_strong: 5,
            no_progress_to_mid: 2,
            no_progress_to_strong: 2,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::Medium,
        });
        assert_eq!(router.record(StepOutcome::Failure), Tier::Fast);
        assert_eq!(router.record(StepOutcome::Failure), Tier::Mid);
        assert_eq!(router.record(StepOutcome::NoProgress), Tier::Fast);
        assert_eq!(router.record(StepOutcome::NoProgress), Tier::Strong);
    }

    #[test]
    fn resets_counters_on_progress() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 1,
            failures_to_strong: 2,
            no_progress_to_mid: 1,
            no_progress_to_strong: 2,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::Medium,
        });
        router.record(StepOutcome::Failure);
        router.record(StepOutcome::NoProgress);
        router.record(StepOutcome::NoProgress);
        assert_eq!(router.tier(), Tier::Strong);
        assert_eq!(
            router.counters(),
            RouterCounters {
                failures: 0,
                no_progress: 2,
                low_actionability: 0
            }
        );
        assert_eq!(router.record(StepOutcome::Progress), Tier::Fast);
        assert_eq!(
            router.counters(),
            RouterCounters {
                failures: 0,
                no_progress: 0,
                low_actionability: 0
            }
        );
    }

    #[test]
    fn resets_counters_on_progress_after_mixed_outcomes() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 1,
            failures_to_strong: 2,
            no_progress_to_mid: 1,
            no_progress_to_strong: 2,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::Medium,
        });

        let prev = sample_observation("hash1");
        let same = sample_observation("hash1");
        let changed = sample_observation("hash2");

        let (outcome, _) = step_outcome(&sample_result(true), &prev, &same, 1);
        assert_eq!(outcome, StepOutcome::NoProgress);
        router.record(outcome);

        let (outcome, _) = step_outcome(&sample_result(false), &same, &changed, 0);
        assert_eq!(outcome, StepOutcome::Failure);
        router.record(outcome);

        assert_eq!(
            router.counters(),
            RouterCounters {
                failures: 1,
                no_progress: 1,
                low_actionability: 0
            }
        );

        let (outcome, _) = step_outcome(&sample_result(true), &same, &changed, 0);
        assert_eq!(outcome, StepOutcome::Progress);
        router.record(outcome);

        assert_eq!(
            router.counters(),
            RouterCounters {
                failures: 0,
                no_progress: 0,
                low_actionability: 0
            }
        );
    }

    #[test]
    fn escalates_on_low_actionability_thresholds() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 4,
            failures_to_strong: 6,
            no_progress_to_mid: 3,
            no_progress_to_strong: 5,
            low_actionability_to_mid: 1,
            low_actionability_to_strong: 2,
            reasoning_effort: ReasoningEffort::Medium,
        });

        let prev = observation_with_elements("hash1", vec![element("el_1", "button")]);
        let next = observation_with_elements("hash2", vec![element("el_2", "link")]);
        let (outcome, heuristics) = step_outcome(&sample_result(true), &prev, &next, 0);

        assert!(heuristics.low_actionability);
        assert_eq!(
            router.record_with_heuristics(outcome, Some(&heuristics)),
            Tier::Mid
        );

        let (outcome, heuristics) = step_outcome(&sample_result(true), &prev, &next, 0);
        assert_eq!(
            router.record_with_heuristics(outcome, Some(&heuristics)),
            Tier::Strong
        );
    }
}
