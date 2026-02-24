use crate::agent::memory::StepRecord;
use crate::llm::client::{LlmContext, LlmError, LlmResult};
use crate::llm::prompts::SYSTEM_PROMPT;
use crate::types::{Action, LlmPayloadMode, Observation, ScreenshotMetadata};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LlmRequest {
    pub system: String,
    pub payload_mode: LlmPayloadMode,
    pub user: LlmUserMessage,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LlmUserMessage {
    pub parts: Vec<LlmContentPart>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmContentPart {
    Text {
        text: String,
    },
    Image {
        source: String,
        mime_type: String,
        data_base64: String,
        sha256: String,
        bytes: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        artifact_ref: Option<String>,
    },
}

pub fn build_request(context: &LlmContext<'_>, schema_json: &Value) -> LlmResult<LlmRequest> {
    let prompt = build_prompt_text(
        context.task,
        context.plan,
        context.observation,
        context.observations,
        context.history,
        context.steps,
        schema_json,
    )?;
    Ok(build_request_from_prompt(
        prompt,
        context.observation,
        context.observation_screenshot,
    ))
}

pub fn build_request_from_prompt(
    prompt: String,
    observation: &Observation,
    screenshot_bytes: Option<&[u8]>,
) -> LlmRequest {
    let mut parts = vec![LlmContentPart::Text { text: prompt }];
    let payload_mode =
        if let (Some(metadata), Some(bytes)) = (&observation.screenshot, screenshot_bytes) {
            parts.push(build_image_part(metadata, bytes));
            LlmPayloadMode::Multimodal
        } else {
            LlmPayloadMode::TextOnly
        };
    LlmRequest {
        system: SYSTEM_PROMPT.to_string(),
        payload_mode,
        user: LlmUserMessage { parts },
    }
}

pub fn build_prompt_text(
    task: &str,
    plan: Option<&str>,
    observation: &Observation,
    observations: &VecDeque<Observation>,
    history: &[Action],
    steps: &[StepRecord],
    schema_json: &Value,
) -> LlmResult<String> {
    let observation_json = serde_json::to_string(observation)
        .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
    let observations_json = serde_json::to_string(observations)
        .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
    let history_json = serde_json::to_string(history)
        .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
    let history_tail_json = serde_json::to_string(&history_tail(history, 8))
        .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
    let step_feedback_json = serde_json::to_string(&step_feedback_tail(steps, 8))
        .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
    let schema_json = serde_json::to_string(schema_json)
        .map_err(|err| LlmError::new("serialize_error", err.to_string()))?;
    let plan_text = plan.unwrap_or("(none)");
    let state_hash_streak = trailing_state_hash_streak(observations);

    Ok(format!(
        "Task: {task}\nPlan: {plan_text}\nObservation: {observation_json}\nRecentObservations: {observations_json}\nStateHashStreak: {state_hash_streak}\nHistory: {history_json}\nRecentHistoryTail: {history_tail_json}\nRecentStepFeedback: {step_feedback_json}\nExecutionRules: [\"If StateHashStreak > 0, do not repeat the same exact action from RecentHistoryTail[-1]\", \"If RecentStepFeedback shows validation_code=repeat_no_progress_action, do not propose that blocked action/id again\", \"Use different element ids or action types when the state hash is unchanged\", \"Keep scroll deltas within |dx|<=2000 and |dy|<=2000\", \"Keep wait.ms <= 30000\"]\nSchema: {schema_json}\nReturn exactly one JSON action object matching the schema and nothing else.",
    ))
}

fn build_image_part(metadata: &ScreenshotMetadata, bytes: &[u8]) -> LlmContentPart {
    LlmContentPart::Image {
        source: "screenshot".to_string(),
        mime_type: metadata.mime_type.clone(),
        data_base64: STANDARD.encode(bytes),
        sha256: metadata.sha256.clone(),
        bytes: metadata.bytes,
        artifact_ref: metadata.artifact_ref.clone(),
    }
}

fn trailing_state_hash_streak(observations: &VecDeque<Observation>) -> u32 {
    let Some(latest) = observations.back() else {
        return 0;
    };
    let mut streak = 0_u32;
    for observation in observations.iter().rev().skip(1) {
        if observation.state_hash == latest.state_hash {
            streak = streak.saturating_add(1);
        } else {
            break;
        }
    }
    streak
}

fn history_tail(history: &[Action], max_items: usize) -> &[Action] {
    let start = history.len().saturating_sub(max_items);
    &history[start..]
}

#[derive(serde::Serialize)]
struct PromptStepFeedback<'a> {
    action: &'a Action,
    outcome: &'a str,
    result_ok: bool,
    error_code: Option<&'a str>,
    validation_code: Option<&'a str>,
    validation_codes: Vec<&'a str>,
    new_state_hash: Option<&'a str>,
}

fn step_feedback_tail(steps: &[StepRecord], max_items: usize) -> Vec<PromptStepFeedback<'_>> {
    let start = steps.len().saturating_sub(max_items);
    steps[start..]
        .iter()
        .map(|step| PromptStepFeedback {
            action: &step.action,
            outcome: outcome_label(&step.outcome),
            result_ok: step.result.ok,
            error_code: step.result.error.as_ref().map(|error| error.code.as_str()),
            validation_code: step
                .result
                .error
                .as_ref()
                .and_then(|error| error.validation_code.as_deref()),
            validation_codes: step
                .validation
                .errors
                .iter()
                .map(|error| error.code.as_str())
                .collect(),
            new_state_hash: step.result.new_state_hash.as_deref(),
        })
        .collect()
}

fn outcome_label(outcome: &crate::agent::memory::StepOutcomeLog) -> &'static str {
    match outcome {
        crate::agent::memory::StepOutcomeLog::Done => "done",
        crate::agent::memory::StepOutcomeLog::ValidationFailed => "validation_failed",
        crate::agent::memory::StepOutcomeLog::ApplyFailed => "apply_failed",
        crate::agent::memory::StepOutcomeLog::NoProgress => "no_progress",
        crate::agent::memory::StepOutcomeLog::Progress => "progress",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::schema::ActionSchema;
    use crate::types::{ElementFlags, ElementRef, LlmPayloadMode};
    use serde_json::json;

    fn sample_observation(hash: &str) -> Observation {
        Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            screenshot: None,
            state_hash: hash.to_string(),
            elements: Vec::new(),
        }
    }

    #[test]
    fn prompt_includes_recent_observations_in_order() {
        let schema = ActionSchema::default();
        let mut observations = VecDeque::new();
        observations.push_back(sample_observation("hash-1"));
        observations.push_back(sample_observation("hash-2"));
        let current = sample_observation("hash-2");

        let prompt = build_prompt_text(
            "task",
            None,
            &current,
            &observations,
            &[],
            &[],
            schema.json(),
        )
        .expect("prompt");

        let line = prompt
            .lines()
            .find(|line| line.starts_with("RecentObservations: "))
            .expect("recent observations line");
        let payload = line.trim_start_matches("RecentObservations: ");
        let parsed: Vec<Observation> = serde_json::from_str(payload).expect("parse observations");

        let hashes: Vec<String> = parsed.into_iter().map(|obs| obs.state_hash).collect();
        assert_eq!(hashes, vec!["hash-1".to_string(), "hash-2".to_string()]);
    }

    #[test]
    fn prompt_includes_state_hash_streak() {
        let schema = ActionSchema::default();
        let mut observations = VecDeque::new();
        observations.push_back(sample_observation("hash-1"));
        observations.push_back(sample_observation("hash-2"));
        observations.push_back(sample_observation("hash-2"));
        observations.push_back(sample_observation("hash-2"));
        let current = sample_observation("hash-2");

        let prompt = build_prompt_text(
            "task",
            None,
            &current,
            &observations,
            &[],
            &[],
            schema.json(),
        )
        .expect("prompt");

        assert!(
            prompt.contains("StateHashStreak: 2"),
            "expected prompt to include trailing streak count"
        );
    }

    #[test]
    fn prompt_includes_recent_step_feedback() {
        let schema = ActionSchema::default();
        let mut observations = VecDeque::new();
        observations.push_back(sample_observation("hash-1"));
        let current = sample_observation("hash-1");
        let step = StepRecord {
            action: Action::Click {
                id: "el_1".to_string(),
            },
            validation: crate::agent::memory::ValidationOutcome::failure(vec![
                crate::verify::rules::ValidationError {
                    code: "repeat_no_progress_action".to_string(),
                    field: None,
                    message: "blocked".to_string(),
                },
            ]),
            result: crate::types::StepResult {
                ok: false,
                error: Some(crate::types::StepError {
                    code: "invalid_action".to_string(),
                    message: "blocked".to_string(),
                    validation_code: Some("repeat_no_progress_action".to_string()),
                }),
                diagnostics: Vec::new(),
                new_state_hash: Some("hash-1".to_string()),
                scroll: None,
                extract: None,
            },
            outcome: crate::agent::memory::StepOutcomeLog::ValidationFailed,
            timings: crate::agent::memory::StepTimings {
                step_duration_ms: 1,
                llm_duration_ms: 1,
                apply_duration_ms: 0,
                snapshot_duration_ms: 0,
            },
            llm_payload_mode: LlmPayloadMode::TextOnly,
            llm_usage: None,
        };

        let prompt = build_prompt_text(
            "task",
            None,
            &current,
            &observations,
            &[],
            &[step],
            schema.json(),
        )
        .expect("prompt");

        assert!(prompt.contains("RecentStepFeedback: "));
        assert!(prompt.contains("repeat_no_progress_action"));
        assert!(prompt.contains("validation_failed"));
    }

    #[test]
    fn request_includes_image_part_when_metadata_and_bytes_present() {
        let observation = Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: Some("el_1".to_string()),
            visible_text: "Hello".to_string(),
            screenshot: Some(ScreenshotMetadata {
                mime_type: "image/png".to_string(),
                artifact_ref: Some("step://run/step-1/screenshot.png".to_string()),
                sha256: "deadbeef".to_string(),
                bytes: 4,
            }),
            state_hash: "hash-1".to_string(),
            elements: vec![ElementRef {
                id: "el_1".to_string(),
                role: "textbox".to_string(),
                name: Some("From".to_string()),
                value: None,
                bbox: [10.0, 20.0, 300.0, 40.0],
                flags: ElementFlags {
                    focused: Some(true),
                    ..ElementFlags::default()
                },
            }],
        };
        let prompt = "prompt".to_string();
        let request = build_request_from_prompt(prompt, &observation, Some(&[0, 1, 2, 3]));

        assert_eq!(request.payload_mode, LlmPayloadMode::Multimodal);
        assert_eq!(request.user.parts.len(), 2);
        assert!(matches!(request.user.parts[0], LlmContentPart::Text { .. }));
        match &request.user.parts[1] {
            LlmContentPart::Image {
                source,
                mime_type,
                data_base64,
                sha256,
                bytes,
                artifact_ref,
            } => {
                assert_eq!(source, "screenshot");
                assert_eq!(mime_type, "image/png");
                assert_eq!(data_base64, "AAECAw==");
                assert_eq!(sha256, "deadbeef");
                assert_eq!(*bytes, 4);
                assert_eq!(
                    artifact_ref.as_deref(),
                    Some("step://run/step-1/screenshot.png")
                );
            }
            _ => panic!("expected image part"),
        }
    }

    #[test]
    fn request_omits_image_part_without_bytes() {
        let observation = Observation {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            viewport: [1280, 800],
            focused: None,
            visible_text: "Hello".to_string(),
            screenshot: Some(ScreenshotMetadata {
                mime_type: "image/png".to_string(),
                artifact_ref: None,
                sha256: "deadbeef".to_string(),
                bytes: 4,
            }),
            state_hash: "hash-1".to_string(),
            elements: Vec::new(),
        };
        let request = build_request_from_prompt("prompt".to_string(), &observation, None);

        assert_eq!(request.payload_mode, LlmPayloadMode::TextOnly);
        assert_eq!(request.user.parts.len(), 1);
        assert!(matches!(request.user.parts[0], LlmContentPart::Text { .. }));
    }

    #[test]
    fn text_only_request_retains_observation_prompt_fields() {
        let schema = ActionSchema::default();
        let mut observations = VecDeque::new();
        observations.push_back(sample_observation("hash-1"));
        let current = sample_observation("hash-1");

        let context = LlmContext {
            task: "task",
            plan: None,
            observation: &current,
            observations: &observations,
            observation_screenshot: None,
            history: &[],
            steps: &[],
        };

        let request = build_request(&context, schema.json()).expect("request");
        assert_eq!(request.payload_mode, LlmPayloadMode::TextOnly);

        let text = match &request.user.parts[0] {
            LlmContentPart::Text { text } => text,
            _ => panic!("expected text part"),
        };
        assert!(text.contains("Observation: "));
        assert!(text.contains("RecentObservations: "));
        assert!(text.contains("StateHashStreak: "));
    }

    #[test]
    fn image_part_serializes_metadata_fields() {
        let request = LlmRequest {
            system: "system".to_string(),
            payload_mode: LlmPayloadMode::Multimodal,
            user: LlmUserMessage {
                parts: vec![LlmContentPart::Image {
                    source: "screenshot".to_string(),
                    mime_type: "image/png".to_string(),
                    data_base64: "AAECAw==".to_string(),
                    sha256: "deadbeef".to_string(),
                    bytes: 4,
                    artifact_ref: Some("step://run/step-1/screenshot.png".to_string()),
                }],
            },
        };

        let value = serde_json::to_value(&request).expect("serialize request");
        let parts = value
            .get("user")
            .and_then(|user| user.get("parts"))
            .and_then(|parts| parts.as_array())
            .expect("parts array");
        let image = parts.first().expect("image part");
        assert_eq!(image.get("mime_type"), Some(&json!("image/png")));
        assert_eq!(image.get("sha256"), Some(&json!("deadbeef")));
        assert_eq!(image.get("bytes"), Some(&json!(4)));
        assert_eq!(
            image.get("artifact_ref"),
            Some(&json!("step://run/step-1/screenshot.png"))
        );
    }
}
