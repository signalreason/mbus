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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouterLadderStep {
    pub model: String,
    pub tier: Tier,
    pub effort: ReasoningEffort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouterCounters {
    pub failures: u32,
    pub no_progress: u32,
    pub low_actionability: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouterTransitionReason {
    UnchangedState { streak: u32 },
    RepeatValidationCode { code: String, streak: u32 },
    CounterTier { tier: Tier },
}

impl RouterTransitionReason {
    pub fn code(&self) -> &'static str {
        match self {
            RouterTransitionReason::UnchangedState { .. } => "unchanged_state",
            RouterTransitionReason::RepeatValidationCode { .. } => "repeat_validation_code",
            RouterTransitionReason::CounterTier { .. } => "counter_tier",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouterTransition {
    pub reason: RouterTransitionReason,
    pub step: RouterLadderStep,
    pub index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LadderPolicyInput<'a> {
    pub current_index: usize,
    pub ladder_len: usize,
    pub state_hash_streak: u32,
    pub validation_code_streak: u32,
    pub validation_code: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LadderTransitionDecision {
    pub next_index: usize,
    pub reason: RouterTransitionReason,
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
    pub ladder: Vec<RouterLadderStep>,
    pub(crate) ladder_spec: Option<Vec<String>>,
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
            ladder: Vec::new(),
            ladder_spec: None,
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
    ladder_index: usize,
    transitions: Vec<RouterTransition>,
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

pub fn ladder_transition_policy(input: LadderPolicyInput<'_>) -> Option<LadderTransitionDecision> {
    if input.ladder_len == 0 {
        return None;
    }
    if input.current_index + 1 >= input.ladder_len {
        return None;
    }
    if input.validation_code_streak >= 2
        && let Some(code) = input.validation_code
    {
        return Some(LadderTransitionDecision {
            next_index: input.current_index + 1,
            reason: RouterTransitionReason::RepeatValidationCode {
                code: code.to_string(),
                streak: input.validation_code_streak,
            },
        });
    }
    if input.state_hash_streak > 0 {
        return Some(LadderTransitionDecision {
            next_index: input.current_index + 1,
            reason: RouterTransitionReason::UnchangedState {
                streak: input.state_hash_streak,
            },
        });
    }
    None
}

impl Router {
    pub fn new(config: RouterConfig) -> Self {
        let reasoning_effort = baseline_effort(&config);
        Self {
            reasoning_effort,
            config,
            failures: 0,
            no_progress: 0,
            low_actionability: 0,
            ladder_index: 0,
            transitions: Vec::new(),
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
                self.reset_state(false);
                return self.active_tier();
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
        let tier_floor = self.tier();
        self.apply_tier_floor(tier_floor);
        self.active_tier()
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

    pub fn active_tier(&self) -> Tier {
        self.config
            .ladder
            .get(self.ladder_index)
            .map(|step| step.tier)
            .unwrap_or_else(|| self.tier())
    }

    pub fn effort(&self) -> ReasoningEffort {
        self.reasoning_effort
    }

    pub fn set_effort(&mut self, effort: ReasoningEffort) {
        self.reasoning_effort = effort;
    }

    pub fn ladder_index(&self) -> usize {
        self.ladder_index
    }

    pub fn counters(&self) -> RouterCounters {
        RouterCounters {
            failures: self.failures,
            no_progress: self.no_progress,
            low_actionability: self.low_actionability,
        }
    }

    pub fn reset(&mut self) {
        self.reset_state(true);
    }

    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    pub fn ladder(&self) -> &[RouterLadderStep] {
        &self.config.ladder
    }

    pub fn transitions(&self) -> &[RouterTransition] {
        &self.transitions
    }

    pub fn apply_ladder_transition(
        &mut self,
        decision: LadderTransitionDecision,
    ) -> Option<RouterTransition> {
        if self.config.ladder.is_empty() {
            return None;
        }
        if decision.next_index <= self.ladder_index {
            return None;
        }
        let step = self.config.ladder.get(decision.next_index)?.clone();
        self.ladder_index = decision.next_index;
        self.reasoning_effort = step.effort;
        let transition = RouterTransition {
            reason: decision.reason,
            step,
            index: decision.next_index,
        };
        self.transitions.push(transition.clone());
        Some(transition)
    }

    fn apply_tier_floor(&mut self, tier: Tier) -> Option<RouterTransition> {
        if self.config.ladder.is_empty() {
            return None;
        }
        let required_index = ladder_index_for_tier(&self.config.ladder, tier);
        if required_index <= self.ladder_index {
            return None;
        }
        let step = self.config.ladder.get(required_index)?.clone();
        self.ladder_index = required_index;
        self.reasoning_effort = step.effort;
        let transition = RouterTransition {
            reason: RouterTransitionReason::CounterTier { tier },
            step,
            index: required_index,
        };
        self.transitions.push(transition.clone());
        Some(transition)
    }

    fn reset_state(&mut self, clear_transitions: bool) {
        self.failures = 0;
        self.no_progress = 0;
        self.low_actionability = 0;
        self.ladder_index = 0;
        if clear_transitions {
            self.transitions.clear();
        }
        self.reasoning_effort = baseline_effort(&self.config);
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

fn ladder_index_for_tier(steps: &[RouterLadderStep], tier: Tier) -> usize {
    let target_rank = tier_rank(tier);
    steps
        .iter()
        .position(|step| tier_rank(step.tier) >= target_rank)
        .unwrap_or_else(|| steps.len().saturating_sub(1))
}

fn tier_rank(tier: Tier) -> u8 {
    match tier {
        Tier::Fast => 0,
        Tier::Mid => 1,
        Tier::Strong => 2,
    }
}

fn baseline_effort(config: &RouterConfig) -> ReasoningEffort {
    if config.ladder.is_empty() {
        config.reasoning_effort
    } else {
        config
            .ladder
            .first()
            .map(|step| step.effort)
            .unwrap_or(config.reasoning_effort)
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
            ..RouterConfig::default()
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
            ..RouterConfig::default()
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
            ..RouterConfig::default()
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
            ..RouterConfig::default()
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
            ..RouterConfig::default()
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
            ..RouterConfig::default()
        });

        let prev = observation_with_elements("hash1", vec![element("el_1", "button")]);
        let next = observation_with_elements("hash1", vec![element("el_2", "link")]);
        let (outcome, heuristics) = step_outcome(&sample_result(true), &prev, &next, 1);

        assert!(heuristics.low_actionability);
        assert_eq!(
            router.record_with_heuristics(outcome, Some(&heuristics)),
            Tier::Mid
        );

        let (outcome, heuristics) = step_outcome(&sample_result(true), &prev, &next, 1);
        assert_eq!(
            router.record_with_heuristics(outcome, Some(&heuristics)),
            Tier::Strong
        );
    }

    #[test]
    fn ladder_policy_advances_on_unchanged_state() {
        let decision = ladder_transition_policy(LadderPolicyInput {
            current_index: 0,
            ladder_len: 3,
            state_hash_streak: 1,
            validation_code_streak: 0,
            validation_code: None,
        })
        .expect("expected decision");
        assert_eq!(decision.next_index, 1);
        assert!(matches!(
            decision.reason,
            RouterTransitionReason::UnchangedState { .. }
        ));
    }

    #[test]
    fn ladder_policy_advances_on_repeat_validation() {
        let decision = ladder_transition_policy(LadderPolicyInput {
            current_index: 0,
            ladder_len: 3,
            state_hash_streak: 0,
            validation_code_streak: 2,
            validation_code: Some("repeat_no_progress_action"),
        })
        .expect("expected decision");
        assert_eq!(decision.next_index, 1);
        assert!(matches!(
            decision.reason,
            RouterTransitionReason::RepeatValidationCode { .. }
        ));
    }

    #[test]
    fn apply_ladder_transition_records_reason_and_step() {
        let ladder = vec![
            RouterLadderStep {
                model: "fast".to_string(),
                tier: Tier::Fast,
                effort: ReasoningEffort::Low,
            },
            RouterLadderStep {
                model: "mid".to_string(),
                tier: Tier::Mid,
                effort: ReasoningEffort::Medium,
            },
        ];
        let mut router = Router::new(RouterConfig {
            ladder,
            reasoning_effort: ReasoningEffort::Low,
            ..RouterConfig::default()
        });
        let decision = LadderTransitionDecision {
            next_index: 1,
            reason: RouterTransitionReason::UnchangedState { streak: 1 },
        };
        let transition = router
            .apply_ladder_transition(decision)
            .expect("transition recorded");
        assert_eq!(transition.index, 1);
        assert_eq!(transition.step.model, "mid");
        assert_eq!(transition.step.effort, ReasoningEffort::Medium);
        assert!(matches!(
            transition.reason,
            RouterTransitionReason::UnchangedState { .. }
        ));
    }

    #[test]
    fn resets_ladder_on_progress_after_tier_escalation() {
        let ladder = vec![
            RouterLadderStep {
                model: "fast".to_string(),
                tier: Tier::Fast,
                effort: ReasoningEffort::Low,
            },
            RouterLadderStep {
                model: "mid".to_string(),
                tier: Tier::Mid,
                effort: ReasoningEffort::Low,
            },
            RouterLadderStep {
                model: "strong".to_string(),
                tier: Tier::Strong,
                effort: ReasoningEffort::Low,
            },
        ];
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 1,
            failures_to_strong: 3,
            no_progress_to_mid: 10,
            no_progress_to_strong: 20,
            low_actionability_to_mid: 10,
            low_actionability_to_strong: 20,
            reasoning_effort: ReasoningEffort::Low,
            ladder,
            ..RouterConfig::default()
        });

        router.record(StepOutcome::Failure);
        assert_eq!(router.ladder_index(), 1);
        assert_eq!(router.active_tier(), Tier::Mid);

        assert_eq!(router.record(StepOutcome::Progress), Tier::Fast);
        assert_eq!(router.ladder_index(), 0);
        assert_eq!(router.effort(), ReasoningEffort::Low);
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
    fn resets_ladder_on_progress_after_effort_escalation() {
        let ladder = vec![
            RouterLadderStep {
                model: "fast".to_string(),
                tier: Tier::Fast,
                effort: ReasoningEffort::Low,
            },
            RouterLadderStep {
                model: "fast".to_string(),
                tier: Tier::Fast,
                effort: ReasoningEffort::Medium,
            },
            RouterLadderStep {
                model: "fast".to_string(),
                tier: Tier::Fast,
                effort: ReasoningEffort::High,
            },
        ];
        let mut router = Router::new(RouterConfig {
            ladder,
            reasoning_effort: ReasoningEffort::Low,
            ..RouterConfig::default()
        });
        let decision = LadderTransitionDecision {
            next_index: 1,
            reason: RouterTransitionReason::UnchangedState { streak: 1 },
        };
        router
            .apply_ladder_transition(decision)
            .expect("transition recorded");
        assert_eq!(router.ladder_index(), 1);
        assert_eq!(router.effort(), ReasoningEffort::Medium);
        assert_eq!(router.active_tier(), Tier::Fast);

        assert_eq!(router.record(StepOutcome::Progress), Tier::Fast);
        assert_eq!(router.ladder_index(), 0);
        assert_eq!(router.effort(), ReasoningEffort::Low);
        assert_eq!(
            router.counters(),
            RouterCounters {
                failures: 0,
                no_progress: 0,
                low_actionability: 0
            }
        );
    }
}
