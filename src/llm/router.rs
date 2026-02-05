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
}

#[derive(Clone, Debug)]
pub struct RouterConfig {
    pub failures_to_mid: u32,
    pub failures_to_strong: u32,
    pub no_progress_to_mid: u32,
    pub no_progress_to_strong: u32,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            failures_to_mid: 2,
            failures_to_strong: 4,
            no_progress_to_mid: 2,
            no_progress_to_strong: 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Router {
    config: RouterConfig,
    failures: u32,
    no_progress: u32,
}

impl Default for Router {
    fn default() -> Self {
        Self::new(RouterConfig::default())
    }
}

impl Router {
    pub fn new(config: RouterConfig) -> Self {
        Self {
            config,
            failures: 0,
            no_progress: 0,
        }
    }

    pub fn record(&mut self, outcome: StepOutcome) -> Tier {
        match outcome {
            StepOutcome::Progress => {
                self.failures = 0;
                self.no_progress = 0;
            }
            StepOutcome::Failure => {
                self.failures = self.failures.saturating_add(1);
            }
            StepOutcome::NoProgress => {
                self.no_progress = self.no_progress.saturating_add(1);
            }
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
        max_tier(failure_tier, no_progress_tier)
    }

    pub fn counters(&self) -> RouterCounters {
        RouterCounters {
            failures: self.failures,
            no_progress: self.no_progress,
        }
    }

    pub fn reset(&mut self) {
        self.failures = 0;
        self.no_progress = 0;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalates_on_failure_thresholds() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 2,
            failures_to_strong: 4,
            no_progress_to_mid: 3,
            no_progress_to_strong: 5,
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
        });
        assert_eq!(router.record(StepOutcome::Failure), Tier::Fast);
        assert_eq!(router.record(StepOutcome::Failure), Tier::Mid);
        assert_eq!(router.record(StepOutcome::NoProgress), Tier::Mid);
        assert_eq!(router.record(StepOutcome::NoProgress), Tier::Strong);
    }

    #[test]
    fn resets_counters_on_progress() {
        let mut router = Router::new(RouterConfig {
            failures_to_mid: 1,
            failures_to_strong: 2,
            no_progress_to_mid: 1,
            no_progress_to_strong: 2,
        });
        router.record(StepOutcome::Failure);
        router.record(StepOutcome::NoProgress);
        router.record(StepOutcome::NoProgress);
        assert_eq!(router.tier(), Tier::Strong);
        assert_eq!(router.counters(), RouterCounters { failures: 1, no_progress: 2 });
        assert_eq!(router.record(StepOutcome::Progress), Tier::Fast);
        assert_eq!(router.counters(), RouterCounters { failures: 0, no_progress: 0 });
    }
}
