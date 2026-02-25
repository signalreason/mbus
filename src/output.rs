use crate::agent::memory::StepRecord;
use crate::types::{Action, ReasoningEffort, SCREENSHOT_MIME_TYPE as OBS_SCREENSHOT_MIME_TYPE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const EXTRACT_OUTPUT_SCHEMA_VERSION: u32 = 1;
pub const TRANSITION_TRACE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractOutput {
    pub schema_version: u32,
    pub run_id: String,
    pub task_id: String,
    pub task: String,
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
pub struct TransitionTraceSnippet {
    pub schema_version: u32,
    pub run_id: String,
    pub task_id: String,
    pub task: String,
    pub timestamp: String,
    pub entries: Vec<TransitionTraceEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransitionTraceEntry {
    pub step_index: usize,
    pub reason_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streak: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_tier: Option<String>,
    pub model: String,
    pub effort: ReasoningEffort,
    pub tier: String,
    pub ladder_index: usize,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<usize>,
}

pub const SCREENSHOT_ARTIFACT_KIND: &str = "screenshot";
pub const SCREENSHOT_FILENAME: &str = "screenshot.png";
pub const SCREENSHOT_MIME_TYPE: &str = OBS_SCREENSHOT_MIME_TYPE;
pub const TRANSITION_TRACE_ARTIFACT_KIND: &str = "transition_trace";
pub const TRANSITION_TRACE_FILENAME: &str = "transition-trace.json";
pub const TRANSITION_TRACE_MIME_TYPE: &str = "application/json";
const SCREENSHOT_ARTIFACT_ROOT: &str = ".ralph/runs";

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ScreenshotSummary {
    pub captures: usize,
    pub failures: usize,
    pub bytes: usize,
    pub duration_ms: usize,
    pub persist_failures: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RouterTransitionSummary {
    pub reason_code: String,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RouterFinalState {
    pub model: String,
    pub effort: ReasoningEffort,
    pub tier: String,
    pub ladder_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RouterSummary {
    pub transitions: Vec<RouterTransitionSummary>,
    pub final_state: RouterFinalState,
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
    pub screenshots: ScreenshotSummary,
    pub router: RouterSummary,
}

pub fn task_id_for(task: &str) -> String {
    let mut hasher = DefaultHasher::new();
    task.hash(&mut hasher);
    format!("task_{:016x}", hasher.finish())
}

pub fn current_timestamp() -> Result<String, time::error::Format> {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc().format(&Rfc3339)
}

pub fn run_id_for(task_id: &str, timestamp: &str) -> String {
    format!("{task_id}_{timestamp}")
}

pub fn screenshot_artifact_ref(run_id: &str, step_index: usize) -> String {
    format!("step://{run_id}/step-{step_index}/{SCREENSHOT_FILENAME}")
}

pub fn screenshot_artifact_path(run_id: &str, step_index: usize) -> PathBuf {
    PathBuf::from(SCREENSHOT_ARTIFACT_ROOT)
        .join(run_id)
        .join("steps")
        .join(format!("step-{step_index}"))
        .join(SCREENSHOT_FILENAME)
}

pub fn transition_trace_artifact_path(run_id: &str) -> PathBuf {
    PathBuf::from(SCREENSHOT_ARTIFACT_ROOT)
        .join(run_id)
        .join(TRANSITION_TRACE_FILENAME)
}

pub fn write_screenshot_artifact(
    run_id: &str,
    step_index: usize,
    bytes: &[u8],
) -> io::Result<OutputArtifact> {
    let path = screenshot_artifact_path(run_id, step_index);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    let digest = sha256_hex(bytes);
    Ok(OutputArtifact {
        kind: SCREENSHOT_ARTIFACT_KIND.to_string(),
        path: path.display().to_string(),
        record_count: None,
        step_index: Some(step_index),
        artifact_ref: Some(screenshot_artifact_ref(run_id, step_index)),
        mime_type: Some(SCREENSHOT_MIME_TYPE.to_string()),
        sha256: Some(digest),
        bytes: Some(bytes.len()),
    })
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn build_extract_output(
    task: impl Into<String>,
    task_id: impl Into<String>,
    timestamp: impl Into<String>,
    steps: &[StepRecord],
) -> Option<ExtractOutput> {
    let task = task.into();
    let task_id = task_id.into();
    let timestamp = timestamp.into();
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
            schema_version: EXTRACT_OUTPUT_SCHEMA_VERSION,
            run_id: run_id_for(&task_id, &timestamp),
            task_id,
            task,
            timestamp,
            extracts,
        })
    }
}

pub fn build_transition_trace(
    task: impl Into<String>,
    task_id: impl Into<String>,
    timestamp: impl Into<String>,
    steps: &[StepRecord],
) -> Option<TransitionTraceSnippet> {
    let task = task.into();
    let task_id = task_id.into();
    let timestamp = timestamp.into();
    let mut entries = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let Some(router) = step.router.as_ref() else {
            continue;
        };
        if router.transitions.is_empty() {
            continue;
        }
        for transition in &router.transitions {
            entries.push(TransitionTraceEntry {
                step_index: index + 1,
                reason_code: transition.reason_code.clone(),
                validation_code: transition.validation_code.clone(),
                streak: transition.streak,
                counter_tier: transition.counter_tier.clone(),
                model: transition.model.clone(),
                effort: transition.effort,
                tier: transition.tier.clone(),
                ladder_index: transition.ladder_index,
            });
        }
    }

    if entries.is_empty() {
        None
    } else {
        Some(TransitionTraceSnippet {
            schema_version: TRANSITION_TRACE_SCHEMA_VERSION,
            run_id: run_id_for(&task_id, &timestamp),
            task_id,
            task,
            timestamp,
            entries,
        })
    }
}

pub fn write_extract_output(path: &Path, output: &ExtractOutput) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec(output).map_err(io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    let len = file.metadata()?.len();
    if len > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut buf = [0u8; 1];
        file.read_exact(&mut buf)?;
        if buf[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    file.write_all(&data)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn write_transition_trace(path: &Path, trace: &TransitionTraceSnippet) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec(trace).map_err(io::Error::other)?;
    std::fs::write(path, data)?;
    Ok(())
}

pub fn write_transition_trace_artifact(
    run_id: &str,
    trace: &TransitionTraceSnippet,
) -> io::Result<OutputArtifact> {
    let path = transition_trace_artifact_path(run_id);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec(trace).map_err(io::Error::other)?;
    std::fs::write(&path, &data)?;
    let digest = sha256_hex(&data);
    Ok(OutputArtifact {
        kind: TRANSITION_TRACE_ARTIFACT_KIND.to_string(),
        path: path.display().to_string(),
        record_count: Some(trace.entries.len()),
        step_index: None,
        artifact_ref: None,
        mime_type: Some(TRANSITION_TRACE_MIME_TYPE.to_string()),
        sha256: Some(digest),
        bytes: Some(data.len()),
    })
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

fn router_transition_counts(steps: &[StepRecord]) -> Vec<RouterTransitionSummary> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for step in steps {
        let Some(router) = step.router.as_ref() else {
            continue;
        };
        for transition in &router.transitions {
            let entry = counts.entry(transition.reason_code.clone()).or_insert(0);
            *entry = entry.saturating_add(1);
        }
    }
    let mut summary: Vec<RouterTransitionSummary> = counts
        .into_iter()
        .map(|(reason_code, count)| RouterTransitionSummary { reason_code, count })
        .collect();
    summary.sort_by(|left, right| left.reason_code.cmp(&right.reason_code));
    summary
}

pub fn build_run_summary(
    terminal_state: TerminalState,
    steps: &[StepRecord],
    mut extra_errors: Vec<RunErrorSummary>,
    output_artifacts: Vec<OutputArtifact>,
    repair_counts: RepairCounts,
    screenshots: ScreenshotSummary,
    router_final_state: RouterFinalState,
) -> RunSummary {
    let counts = step_counts(steps);
    let mut errors = step_errors(steps);
    errors.append(&mut extra_errors);
    let router_summary = RouterSummary {
        transitions: router_transition_counts(steps),
        final_state: router_final_state,
    };
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
        screenshots,
        router: router_summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::memory::{StepOutcomeLog, StepTimings, ValidationOutcome};
    use crate::types::{Action, ExtractResult, LlmPayloadMode, StepResult};

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
    fn screenshot_artifact_ref_is_step_scoped() {
        let value = screenshot_artifact_ref("task_1_now", 3);
        assert_eq!(value, "step://task_1_now/step-3/screenshot.png");
    }

    #[test]
    fn sha256_hex_is_deterministic() {
        let digest = sha256_hex(b"hello");
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(digest, sha256_hex(b"hello"));
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
                diagnostics: Vec::new(),
                new_state_hash: None,
                scroll: None,
                extract: None,
            },
            outcome: StepOutcomeLog::Progress,
            timings: timings(),
            llm_payload_mode: LlmPayloadMode::TextOnly,
            llm_usage: None,
            router: None,
        }];

        let output = build_extract_output("task", "task_1", "now", &steps);
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
                diagnostics: Vec::new(),
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
            llm_payload_mode: LlmPayloadMode::TextOnly,
            llm_usage: None,
            router: None,
        }];

        let output = build_extract_output("task", "task_1", "now", &steps).expect("output");
        assert_eq!(output.schema_version, EXTRACT_OUTPUT_SCHEMA_VERSION);
        assert_eq!(output.run_id, "task_1_now");
        assert_eq!(output.task_id, "task_1");
        assert_eq!(output.task, "task");
        assert_eq!(output.timestamp, "now");
        assert_eq!(output.extracts.len(), 1);
        assert_eq!(output.extracts[0].step_index, 1);
        assert_eq!(output.extracts[0].query, "price");
        assert_eq!(output.extracts[0].id, Some("el_9".to_string()));
        assert_eq!(output.extracts[0].value, "$10");
    }
}
