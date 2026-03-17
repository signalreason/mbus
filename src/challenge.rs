use crate::bench::{BenchObservedStatus, BenchTaskResult, BenchTokenUsage};
use crate::output::OutputArtifact;
use reqwest::Url;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const BASE_URL_TOKEN: &str = "{{base_url}}";

#[derive(Clone, Debug, Deserialize)]
pub struct ChallengeTask {
    pub id: String,
    pub task: String,
    #[serde(default)]
    pub plan: Option<String>,
    pub start_url: String,
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub max_steps: Option<usize>,
    #[serde(default)]
    pub expect: ChallengeExpectations,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ChallengeExpectations {
    #[serde(default)]
    pub final_url_contains: Option<String>,
    #[serde(default)]
    pub final_visible_text_contains: Option<String>,
    #[serde(default)]
    pub screenshot_artifact_required: bool,
}

pub fn load_tasks(tasks_dir: &Path) -> Result<Vec<ChallengeTask>, String> {
    let entries = std::fs::read_dir(tasks_dir).map_err(|err| {
        format!(
            "failed to read challenge dir {}: {err}",
            tasks_dir.display()
        )
    })?;
    let mut task_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read challenge entry: {err}"))?;
        let path = entry.path();
        let is_json = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if is_json {
            task_paths.push(path);
        }
    }
    task_paths.sort();

    if task_paths.is_empty() {
        return Err(format!(
            "no challenge fixtures found in {} (expected *.json)",
            tasks_dir.display()
        ));
    }

    let mut tasks = Vec::with_capacity(task_paths.len());
    for path in task_paths {
        let content = std::fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let task: ChallengeTask = serde_json::from_str(&content)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
        validate_task(&task).map_err(|err| format!("invalid {}: {err}", path.display()))?;
        tasks.push(task);
    }
    Ok(tasks)
}

pub fn resolve_start_url(start_url: &str, base_url: &str) -> String {
    start_url.replace(BASE_URL_TOKEN, base_url)
}

pub fn challenge_task_limit(task: &ChallengeTask, global_max_steps: usize) -> usize {
    task.max_steps
        .map(|value| value.min(global_max_steps))
        .unwrap_or(global_max_steps)
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_task(
    task: &ChallengeTask,
    status: BenchObservedStatus,
    steps: usize,
    final_url: Option<&str>,
    final_visible_text: Option<&str>,
    max_steps_per_task: usize,
    run_error: Option<&str>,
    usage: BenchTokenUsage,
    output_artifacts: Vec<OutputArtifact>,
) -> BenchTaskResult {
    let mut passed = true;
    let mut failure_reason = None;

    if let Some(err) = run_error {
        passed = false;
        failure_reason = Some(format!("run_error: {err}"));
    } else {
        if status != BenchObservedStatus::Done {
            passed = false;
            failure_reason = Some(format!("status_mismatch: expected Done, got {status:?}"));
        } else if steps > max_steps_per_task {
            passed = false;
            failure_reason = Some(format!(
                "step_limit_exceeded: steps={} limit={}",
                steps, max_steps_per_task
            ));
        }
        if passed {
            let observed = final_url.unwrap_or_default();
            if !is_url_allowed(observed, &task.allowed_domains) {
                passed = false;
                failure_reason = Some(format!(
                    "disallowed_final_url: host for '{observed}' not in {:?}",
                    task.allowed_domains
                ));
            }
        }
        if passed && let Some(expected) = task.expect.final_url_contains.as_deref() {
            let observed = final_url.unwrap_or_default();
            if !observed.contains(expected) {
                passed = false;
                failure_reason = Some(format!(
                    "final_url_mismatch: expected substring '{expected}', observed '{observed}'"
                ));
            }
        }
        if passed && let Some(expected) = task.expect.final_visible_text_contains.as_deref() {
            let observed = final_visible_text.unwrap_or_default();
            if !observed.contains(expected) {
                passed = false;
                failure_reason = Some(format!(
                    "visible_text_mismatch: expected substring '{expected}'"
                ));
            }
        }
        if passed && task.expect.screenshot_artifact_required {
            let has_screenshot = output_artifacts
                .iter()
                .any(|artifact| artifact.kind == crate::output::SCREENSHOT_ARTIFACT_KIND);
            if !has_screenshot {
                passed = false;
                failure_reason = Some("missing_screenshot_artifact".to_string());
            }
        }
    }

    BenchTaskResult {
        task_id: task.id.clone(),
        passed,
        status,
        steps,
        duration_ms: 0,
        usage,
        failure_reason,
        output_artifacts,
        final_url: final_url.map(ToOwned::to_owned),
        final_visible_text: final_visible_text.map(ToOwned::to_owned),
    }
}

pub fn report_path_default() -> PathBuf {
    PathBuf::from("target/challenge/report.json")
}

pub fn tasks_dir_default() -> PathBuf {
    PathBuf::from("harness/challenge")
}

pub fn is_url_allowed(url: &str, allowed_domains: &[String]) -> bool {
    let Some(host) = host_for_url(url) else {
        return false;
    };
    domain_allowed(&host, allowed_domains)
}

fn validate_task(task: &ChallengeTask) -> Result<(), String> {
    if task.id.trim().is_empty() {
        return Err("id must not be empty".to_string());
    }
    if task.task.trim().is_empty() {
        return Err("task must not be empty".to_string());
    }
    if task.start_url.trim().is_empty() {
        return Err("start_url must not be empty".to_string());
    }
    if task.allowed_domains.is_empty() {
        return Err("allowed_domains must not be empty".to_string());
    }
    if task.expect.final_url_contains.is_none()
        && task.expect.final_visible_text_contains.is_none()
        && !task.expect.screenshot_artifact_required
    {
        return Err("expect must include an observable success check".to_string());
    }

    let probe_url = resolve_start_url(&task.start_url, "http://127.0.0.1");
    if !is_url_allowed(&probe_url, &task.allowed_domains) {
        return Err(format!(
            "start_url host must be within allowed_domains: {}",
            task.start_url
        ));
    }

    Ok(())
}

fn host_for_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    parsed.host_str().map(|value| value.to_ascii_lowercase())
}

fn domain_allowed(host: &str, allowed_domains: &[String]) -> bool {
    allowed_domains.iter().any(|allowed| {
        let allowed = allowed.trim().trim_start_matches('.').to_ascii_lowercase();
        host == allowed || host.ends_with(&format!(".{allowed}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::BenchTokenUsage;
    use crate::output::OutputArtifact;

    fn sample_task() -> ChallengeTask {
        ChallengeTask {
            id: "challenge-01".to_string(),
            task: "Dismiss the cookie banner".to_string(),
            plan: None,
            start_url: "{{base_url}}/challenge/cookie-banner.html".to_string(),
            allowed_domains: vec!["127.0.0.1".to_string()],
            max_steps: None,
            expect: ChallengeExpectations {
                final_url_contains: Some("/challenge/cookie-banner.html".to_string()),
                final_visible_text_contains: Some("COOKIE BANNER DISMISSED".to_string()),
                screenshot_artifact_required: true,
            },
        }
    }

    #[test]
    fn resolve_start_url_replaces_base_token() {
        let url = resolve_start_url(
            "{{base_url}}/challenge/cookie-banner.html",
            "http://127.0.0.1:4000",
        );
        assert_eq!(url, "http://127.0.0.1:4000/challenge/cookie-banner.html");
    }

    #[test]
    fn evaluate_task_requires_screenshot_when_requested() {
        let task = sample_task();
        let result = evaluate_task(
            &task,
            BenchObservedStatus::Done,
            3,
            Some("http://127.0.0.1:4000/challenge/cookie-banner.html"),
            Some("COOKIE BANNER DISMISSED"),
            10,
            None,
            BenchTokenUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
                error: None,
            },
            Vec::new(),
        );
        assert!(!result.passed);
        assert_eq!(
            result.failure_reason.as_deref(),
            Some("missing_screenshot_artifact")
        );
    }

    #[test]
    fn evaluate_task_accepts_allowed_final_url_and_artifact() {
        let task = sample_task();
        let result = evaluate_task(
            &task,
            BenchObservedStatus::Done,
            3,
            Some("http://127.0.0.1:4000/challenge/cookie-banner.html"),
            Some("COOKIE BANNER DISMISSED"),
            10,
            None,
            BenchTokenUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
                error: None,
            },
            vec![OutputArtifact {
                kind: crate::output::SCREENSHOT_ARTIFACT_KIND.to_string(),
                path: "x".to_string(),
                record_count: None,
                step_index: Some(1),
                artifact_ref: Some("step://run/step-1/screenshot.png".to_string()),
                mime_type: Some("image/png".to_string()),
                sha256: Some("deadbeef".to_string()),
                bytes: Some(4),
            }],
        );
        assert!(result.passed);
        assert!(result.failure_reason.is_none());
    }

    #[test]
    fn load_tasks_rejects_missing_observable_expectation() {
        let task = ChallengeTask {
            id: "challenge-01".to_string(),
            task: "Dismiss the cookie banner".to_string(),
            plan: None,
            start_url: "{{base_url}}/challenge/cookie-banner.html".to_string(),
            allowed_domains: vec!["127.0.0.1".to_string()],
            max_steps: None,
            expect: ChallengeExpectations::default(),
        };
        let err = validate_task(&task).expect_err("invalid task");
        assert!(err.contains("observable success check"));
    }
}
