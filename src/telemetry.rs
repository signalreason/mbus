use crate::types::Action;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub steps_total: u64,
    pub llm_calls_total: u64,
    pub llm_failures_total: u64,
    pub repair_attempts_total: u64,
    pub repair_success_total: u64,
    pub repair_failures_total: u64,
    pub validation_failures_total: u64,
    pub apply_failures_total: u64,
    pub actions_click_total: u64,
    pub actions_type_total: u64,
    pub actions_select_total: u64,
    pub actions_scroll_total: u64,
    pub actions_wait_total: u64,
    pub actions_navigate_total: u64,
    pub actions_back_total: u64,
    pub actions_extract_total: u64,
    pub actions_done_total: u64,
    pub last_step_duration_ms: u64,
    pub last_snapshot_duration_ms: u64,
    pub last_apply_duration_ms: u64,
    pub last_llm_duration_ms: u64,
    pub no_progress_streak: u64,
}

#[derive(Debug, Default)]
struct Metrics {
    steps_total: AtomicU64,
    llm_calls_total: AtomicU64,
    llm_failures_total: AtomicU64,
    llm_failures_by_code: Mutex<HashMap<&'static str, u64>>,
    repair_attempts_total: AtomicU64,
    repair_success_total: AtomicU64,
    repair_failures_total: AtomicU64,
    validation_failures_total: AtomicU64,
    apply_failures_total: AtomicU64,
    actions_click_total: AtomicU64,
    actions_type_total: AtomicU64,
    actions_select_total: AtomicU64,
    actions_scroll_total: AtomicU64,
    actions_wait_total: AtomicU64,
    actions_navigate_total: AtomicU64,
    actions_back_total: AtomicU64,
    actions_extract_total: AtomicU64,
    actions_done_total: AtomicU64,
    last_step_duration_ms: AtomicU64,
    last_snapshot_duration_ms: AtomicU64,
    last_apply_duration_ms: AtomicU64,
    last_llm_duration_ms: AtomicU64,
    no_progress_streak: AtomicU64,
}

impl Metrics {
    fn inc_step(&self) {
        self.steps_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_llm_call(&self) {
        self.llm_calls_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_llm_failure(&self, code: &'static str) {
        self.llm_failures_total.fetch_add(1, Ordering::Relaxed);
        match self.llm_failures_by_code.lock() {
            Ok(mut guard) => {
                let entry = guard.entry(code).or_insert(0);
                *entry = entry.saturating_add(1);
            }
            Err(err) => {
                tracing::error!(
                    event = "telemetry_error",
                    error = %err,
                    error_code = code,
                    message = "failed to record llm failure code metric"
                );
            }
        }
    }

    fn inc_repair_attempt(&self) {
        self.repair_attempts_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_repair_success(&self) {
        self.repair_success_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_repair_failure(&self) {
        self.repair_failures_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_validation_failure(&self) {
        self.validation_failures_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_apply_failure(&self) {
        self.apply_failures_total.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_action(&self, action: &Action) {
        match action {
            Action::Click { .. } => self.actions_click_total.fetch_add(1, Ordering::Relaxed),
            Action::Type { .. } => self.actions_type_total.fetch_add(1, Ordering::Relaxed),
            Action::Select { .. } => self.actions_select_total.fetch_add(1, Ordering::Relaxed),
            Action::Scroll { .. } => self.actions_scroll_total.fetch_add(1, Ordering::Relaxed),
            Action::Wait { .. } => self.actions_wait_total.fetch_add(1, Ordering::Relaxed),
            Action::Navigate { .. } => {
                self.actions_navigate_total.fetch_add(1, Ordering::Relaxed)
            }
            Action::Back => self.actions_back_total.fetch_add(1, Ordering::Relaxed),
            Action::Extract { .. } => self.actions_extract_total.fetch_add(1, Ordering::Relaxed),
            Action::Done { .. } => self.actions_done_total.fetch_add(1, Ordering::Relaxed),
        };
    }

    fn record_step_duration(&self, duration: Duration) {
        self.last_step_duration_ms
            .store(duration.as_millis() as u64, Ordering::Relaxed);
    }

    fn record_snapshot_duration(&self, duration: Duration) {
        self.last_snapshot_duration_ms
            .store(duration.as_millis() as u64, Ordering::Relaxed);
    }

    fn record_apply_duration(&self, duration: Duration) {
        self.last_apply_duration_ms
            .store(duration.as_millis() as u64, Ordering::Relaxed);
    }

    fn record_llm_duration(&self, duration: Duration) {
        self.last_llm_duration_ms
            .store(duration.as_millis() as u64, Ordering::Relaxed);
    }

    fn set_no_progress_streak(&self, value: u32) {
        self.no_progress_streak
            .store(value as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            steps_total: self.steps_total.load(Ordering::Relaxed),
            llm_calls_total: self.llm_calls_total.load(Ordering::Relaxed),
            llm_failures_total: self.llm_failures_total.load(Ordering::Relaxed),
            repair_attempts_total: self.repair_attempts_total.load(Ordering::Relaxed),
            repair_success_total: self.repair_success_total.load(Ordering::Relaxed),
            repair_failures_total: self.repair_failures_total.load(Ordering::Relaxed),
            validation_failures_total: self.validation_failures_total.load(Ordering::Relaxed),
            apply_failures_total: self.apply_failures_total.load(Ordering::Relaxed),
            actions_click_total: self.actions_click_total.load(Ordering::Relaxed),
            actions_type_total: self.actions_type_total.load(Ordering::Relaxed),
            actions_select_total: self.actions_select_total.load(Ordering::Relaxed),
            actions_scroll_total: self.actions_scroll_total.load(Ordering::Relaxed),
            actions_wait_total: self.actions_wait_total.load(Ordering::Relaxed),
            actions_navigate_total: self.actions_navigate_total.load(Ordering::Relaxed),
            actions_back_total: self.actions_back_total.load(Ordering::Relaxed),
            actions_extract_total: self.actions_extract_total.load(Ordering::Relaxed),
            actions_done_total: self.actions_done_total.load(Ordering::Relaxed),
            last_step_duration_ms: self.last_step_duration_ms.load(Ordering::Relaxed),
            last_snapshot_duration_ms: self.last_snapshot_duration_ms.load(Ordering::Relaxed),
            last_apply_duration_ms: self.last_apply_duration_ms.load(Ordering::Relaxed),
            last_llm_duration_ms: self.last_llm_duration_ms.load(Ordering::Relaxed),
            no_progress_streak: self.no_progress_streak.load(Ordering::Relaxed),
        }
    }
}

static METRICS: OnceLock<Metrics> = OnceLock::new();

fn metrics() -> &'static Metrics {
    METRICS.get_or_init(Metrics::default)
}

pub fn inc_step() {
    metrics().inc_step();
}

pub fn inc_action(action: &Action) {
    metrics().inc_action(action);
}

pub fn inc_llm_call() {
    metrics().inc_llm_call();
}

pub fn inc_llm_failure(code: &'static str) {
    metrics().inc_llm_failure(code);
    tracing::info!(
        event = "metric",
        metric_name = "llm_failures_total",
        value = 1u64,
        error_code = code
    );
}

pub fn inc_repair_attempt() {
    metrics().inc_repair_attempt();
    tracing::info!(
        event = "metric",
        metric_name = "repair_attempts_total",
        value = 1u64
    );
}

pub fn inc_repair_success() {
    metrics().inc_repair_success();
    tracing::info!(
        event = "metric",
        metric_name = "repair_success_total",
        value = 1u64
    );
}

pub fn inc_repair_failure() {
    metrics().inc_repair_failure();
    tracing::info!(
        event = "metric",
        metric_name = "repair_failures_total",
        value = 1u64
    );
}

pub fn inc_validation_failure() {
    metrics().inc_validation_failure();
}

pub fn inc_apply_failure() {
    metrics().inc_apply_failure();
}

pub fn record_step_duration(duration: Duration) {
    metrics().record_step_duration(duration);
}

pub fn record_snapshot_duration(duration: Duration) {
    metrics().record_snapshot_duration(duration);
}

pub fn record_apply_duration(duration: Duration) {
    metrics().record_apply_duration(duration);
}

pub fn record_llm_duration(duration: Duration) {
    metrics().record_llm_duration(duration);
    tracing::info!(
        event = "metric",
        metric_name = "llm_duration_ms",
        value = duration.as_millis() as u64
    );
}

pub fn set_no_progress_streak(value: u32) {
    metrics().set_no_progress_streak(value);
}

pub fn snapshot() -> MetricsSnapshot {
    metrics().snapshot()
}

pub fn action_type(action: &Action) -> &'static str {
    match action {
        Action::Click { .. } => "click",
        Action::Type { .. } => "type",
        Action::Select { .. } => "select",
        Action::Scroll { .. } => "scroll",
        Action::Wait { .. } => "wait",
        Action::Navigate { .. } => "navigate",
        Action::Back => "back",
        Action::Extract { .. } => "extract",
        Action::Done { .. } => "done",
    }
}

#[derive(Debug)]
pub struct ActionSummary<'a> {
    pub action_type: &'static str,
    pub id: Option<&'a str>,
    pub text_len: Option<usize>,
    pub submit: Option<bool>,
    pub value_len: Option<usize>,
    pub url: Option<&'a str>,
    pub scroll: Option<(i64, i64)>,
    pub wait_ms: Option<u64>,
    pub query_len: Option<usize>,
    pub summary_len: Option<usize>,
}

impl<'a> From<&'a Action> for ActionSummary<'a> {
    fn from(action: &'a Action) -> Self {
        match action {
            Action::Click { id } => ActionSummary {
                action_type: "click",
                id: Some(id.as_str()),
                text_len: None,
                submit: None,
                value_len: None,
                url: None,
                scroll: None,
                wait_ms: None,
                query_len: None,
                summary_len: None,
            },
            Action::Type { id, text, submit } => ActionSummary {
                action_type: "type",
                id: Some(id.as_str()),
                text_len: Some(text.chars().count()),
                submit: *submit,
                value_len: None,
                url: None,
                scroll: None,
                wait_ms: None,
                query_len: None,
                summary_len: None,
            },
            Action::Select { id, value } => ActionSummary {
                action_type: "select",
                id: Some(id.as_str()),
                text_len: None,
                submit: None,
                value_len: Some(value.chars().count()),
                url: None,
                scroll: None,
                wait_ms: None,
                query_len: None,
                summary_len: None,
            },
            Action::Scroll { dx, dy } => ActionSummary {
                action_type: "scroll",
                id: None,
                text_len: None,
                submit: None,
                value_len: None,
                url: None,
                scroll: Some((*dx, *dy)),
                wait_ms: None,
                query_len: None,
                summary_len: None,
            },
            Action::Wait { ms } => ActionSummary {
                action_type: "wait",
                id: None,
                text_len: None,
                submit: None,
                value_len: None,
                url: None,
                scroll: None,
                wait_ms: Some(*ms),
                query_len: None,
                summary_len: None,
            },
            Action::Navigate { url } => ActionSummary {
                action_type: "navigate",
                id: None,
                text_len: None,
                submit: None,
                value_len: None,
                url: Some(url.as_str()),
                scroll: None,
                wait_ms: None,
                query_len: None,
                summary_len: None,
            },
            Action::Back => ActionSummary {
                action_type: "back",
                id: None,
                text_len: None,
                submit: None,
                value_len: None,
                url: None,
                scroll: None,
                wait_ms: None,
                query_len: None,
                summary_len: None,
            },
            Action::Extract { query, id } => ActionSummary {
                action_type: "extract",
                id: id.as_deref(),
                text_len: None,
                submit: None,
                value_len: None,
                url: None,
                scroll: None,
                wait_ms: None,
                query_len: Some(query.chars().count()),
                summary_len: None,
            },
            Action::Done { summary } => ActionSummary {
                action_type: "done",
                id: None,
                text_len: None,
                submit: None,
                value_len: None,
                url: None,
                scroll: None,
                wait_ms: None,
                query_len: None,
                summary_len: Some(summary.chars().count()),
            },
        }
    }
}

pub fn init_tracing() {
    use tracing_subscriber::filter::EnvFilter;
    use tracing_subscriber::fmt;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .json()
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_increment_counters() {
        let metrics = Metrics::default();
        metrics.inc_step();
        metrics.inc_llm_call();
        metrics.inc_llm_failure("timeout");
        metrics.inc_repair_attempt();
        metrics.inc_repair_success();
        metrics.inc_repair_failure();
        metrics.inc_validation_failure();
        metrics.inc_apply_failure();
        metrics.inc_action(&Action::Click { id: "el_1".to_string() });
        metrics.record_llm_duration(Duration::from_millis(12));
        metrics.record_step_duration(Duration::from_millis(7));
        metrics.set_no_progress_streak(3);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.steps_total, 1);
        assert_eq!(snapshot.llm_calls_total, 1);
        assert_eq!(snapshot.llm_failures_total, 1);
        assert_eq!(snapshot.repair_attempts_total, 1);
        assert_eq!(snapshot.repair_success_total, 1);
        assert_eq!(snapshot.repair_failures_total, 1);
        assert_eq!(snapshot.validation_failures_total, 1);
        assert_eq!(snapshot.apply_failures_total, 1);
        assert_eq!(snapshot.actions_click_total, 1);
        assert_eq!(snapshot.last_llm_duration_ms, 12);
        assert_eq!(snapshot.last_step_duration_ms, 7);
        assert_eq!(snapshot.no_progress_streak, 3);
    }
}
