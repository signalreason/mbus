use crate::agent::memory::StepRecord;
use crate::types::Action;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractOutput {
    pub task_id: String,
    pub timestamp: String,
    pub extracts: Vec<ExtractRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractRecord {
    pub step_index: usize,
    pub query: String,
    pub id: Option<String>,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalState {
    Done,
    MaxSteps,
    NoProgress,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OutputArtifact {
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RunErrorSummary {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RunSummary {
    pub terminal_state: TerminalState,
    pub steps: usize,
    pub errors: Vec<RunErrorSummary>,
    pub output_artifacts: Vec<OutputArtifact>,
    pub validation_failures: usize,
    pub apply_failures: usize,
    pub apply_successes: usize,
    pub done_steps: usize,
    pub repair_attempts: usize,
    pub repair_successes: usize,
    pub repair_failures: usize,
}

pub fn task_id_for(task: &str) -> String {
    let mut hasher = DefaultHasher::new();
    task.hash(&mut hasher);
    format!("task_{:016x}", hasher.finish())
}

pub fn current_timestamp() -> Result<String, time::error::Format> {
    use time::format_description::well_known::Rfc3339;
    Ok(time::OffsetDateTime::now_utc().format(&Rfc3339)?)
}

pub fn build_extract_output(
    task_id: impl Into<String>,
    timestamp: impl Into<String>,
    steps: &[StepRecord],
) -> Option<ExtractOutput> {
    let extracts: Vec<ExtractRecord> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            if !step.result.ok {
                return None;
            }
            let extract = step.result.extract.as_ref()?;
            if !matches!(step.action, Action::Extract { .. }) {
                return None;
            }
            Some(ExtractRecord {
                step_index: index + 1,
                query: extract.query.clone(),
                id: extract.id.clone(),
                value: extract.value.clone(),
            })
        })
        .collect();

    if extracts.is_empty() {
        None
    } else {
        Some(ExtractOutput {
            task_id: task_id.into(),
            timestamp: timestamp.into(),
            extracts,
        })
    }
}

pub fn write_extract_output(path: &Path, output: &ExtractOutput) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let data = serde_json::to_vec_pretty(output)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    std::fs::write(path, data)?;
    Ok(())
}

#[derive(Default)]
pub struct RepairCounts {
    pub attempts: usize,
    pub successes: usize,
    pub failures: usize,
}

#[derive(Default)]
struct StepCounts {
    validation_failures: usize,
    apply_failures: usize,
    apply_successes: usize,
    done_steps: usize,
}

fn step_counts(steps: &[StepRecord]) -> StepCounts {
    let mut counts = StepCounts::default();
    for step in steps {
        if !step.validation.ok {
            counts.validation_failures += 1;
            continue;
        }
        if matches!(step.action, Action::Done { .. }) {
            counts.done_steps += 1;
            continue;
        }
        if step.result.ok {
            counts.apply_successes += 1;
        } else {
            counts.apply_failures += 1;
        }
    }
    counts
}

fn step_errors(steps: &[StepRecord]) -> Vec<RunErrorSummary> {
    let mut errors = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let step_index = index + 1;
        if !step.validation.ok {
            for err in &step.validation.errors {
                errors.push(RunErrorSummary {
                    code: err.code.clone(),
                    message: err.message.clone(),
                    step_index: Some(step_index),
                    field: err.field.clone(),
                    validation_code: None,
                    kind: Some("validation".to_string()),
                });
            }
            continue;
        }
        if let Some(err) = step.result.error.as_ref() {
            errors.push(RunErrorSummary {
                code: err.code.clone(),
                message: err.message.clone(),
                step_index: Some(step_index),
                field: None,
                validation_code: err.validation_code.clone(),
                kind: Some("apply".to_string()),
            });
        }
    }
    errors
}

pub fn build_run_summary(
    terminal_state: TerminalState,
    steps: &[StepRecord],
    mut extra_errors: Vec<RunErrorSummary>,
    output_artifacts: Vec<OutputArtifact>,
    repair_counts: RepairCounts,
) -> RunSummary {
    let counts = step_counts(steps);
    let mut errors = step_errors(steps);
    errors.append(&mut extra_errors);
    RunSummary {
        terminal_state,
        steps: steps.len(),
        errors,
        output_artifacts,
        validation_failures: counts.validation_failures,
        apply_failures: counts.apply_failures,
        apply_successes: counts.apply_successes,
        done_steps: counts.done_steps,
        repair_attempts: repair_counts.attempts,
        repair_successes: repair_counts.successes,
        repair_failures: repair_counts.failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::{StepOutcomeLog, StepTimings, ValidationOutcome};
    use crate::types::{Action, ExtractResult, StepResult};

    fn timings() -> StepTimings {
        StepTimings {
            step_duration_ms: 1,
            llm_duration_ms: 1,
            apply_duration_ms: 0,
            snapshot_duration_ms: 0,
        }
    }

    #[test]
    fn task_id_is_deterministic() {
        let first = task_id_for("find price");
        let second = task_id_for("find price");
        assert_eq!(first, second);
    }

    #[test]
    fn build_extract_output_skips_non_extract_steps() {
        let steps = vec![StepRecord {
            action: Action::Click {
                id: "el_1".to_string(),
            },
            validation: ValidationOutcome::success(),
            result: StepResult {
                ok: true,
                error: None,
                new_state_hash: None,
                scroll: None,
                extract: None,
            },
            outcome: StepOutcomeLog::Progress,
            timings: timings(),
        }];

        let output = build_extract_output("task_1", "now", &steps);
        assert!(output.is_none());
    }

    #[test]
    fn build_extract_output_collects_extracts() {
        let steps = vec![StepRecord {
            action: Action::Extract {
                query: "price".to_string(),
                id: Some("el_9".to_string()),
            },
            validation: ValidationOutcome::success(),
            result: StepResult {
                ok: true,
                error: None,
                new_state_hash: None,
                scroll: None,
                extract: Some(ExtractResult {
                    query: "price".to_string(),
                    id: Some("el_9".to_string()),
                    value: "$10".to_string(),
                }),
            },
            outcome: StepOutcomeLog::Progress,
            timings: timings(),
        }];

        let output = build_extract_output("task_1", "now", &steps).expect("output");
        assert_eq!(output.task_id, "task_1");
        assert_eq!(output.timestamp, "now");
        assert_eq!(output.extracts.len(), 1);
        assert_eq!(output.extracts[0].step_index, 1);
        assert_eq!(output.extracts[0].query, "price");
        assert_eq!(output.extracts[0].id, Some("el_9".to_string()));
        assert_eq!(output.extracts[0].value, "$10");
    }
}
