use crate::types::Action;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const BASE_URL_TOKEN: &str = "{{base_url}}";

pub mod aggregate;

pub const BENCH_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize)]
pub struct BenchTask {
    pub id: String,
    pub task: String,
    #[serde(default)]
    pub plan: Option<String>,
    pub start_path: String,
    #[serde(default)]
    pub max_steps: Option<usize>,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub expect: BenchExpectations,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BenchExpectations {
    #[serde(default = "default_done_status")]
    pub status: BenchExpectedStatus,
    #[serde(default)]
    pub final_url_contains: Option<String>,
    #[serde(default)]
    pub final_visible_text_contains: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchExpectedStatus {
    Done,
    MaxSteps,
}

impl Default for BenchExpectedStatus {
    fn default() -> Self {
        Self::Done
    }
}

fn default_done_status() -> BenchExpectedStatus {
    BenchExpectedStatus::Done
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchObservedStatus {
    Done,
    MaxSteps,
    NoProgress,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchTaskResult {
    pub task_id: String,
    pub passed: bool,
    pub status: BenchObservedStatus,
    pub steps: usize,
    pub duration_ms: u64,
    pub usage: BenchTokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchTokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct BenchPricing {
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
}

impl BenchPricing {
    pub fn from_config(config: &crate::config::LlmConfig) -> Option<Self> {
        match (
            config.input_cost_per_million,
            config.output_cost_per_million,
        ) {
            (Some(input), Some(output)) => Some(Self {
                input_cost_per_million: input,
                output_cost_per_million: output,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchCostSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<BenchPricing>,
    pub input_cost_usd: Option<f64>,
    pub output_cost_usd: Option<f64>,
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchSummary {
    pub total_tasks: usize,
    pub passed_tasks: usize,
    pub required_passes: usize,
    pub completion_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_steps_success: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_steps_success: Option<u64>,
    pub gate_passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchGate {
    pub total_tasks: usize,
    pub passed_tasks: usize,
    pub required_passes: usize,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchLlmInfo {
    pub mode: String,
    pub model_fast: String,
    pub model_mid: String,
    pub model_strong: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchReport {
    pub schema_version: u32,
    pub timestamp: String,
    pub tasks_dir: String,
    pub report_path: String,
    pub llm: BenchLlmInfo,
    pub max_steps_per_task: usize,
    pub required_passes: usize,
    pub duration_ms: u64,
    pub gate: BenchGate,
    pub summary: BenchSummary,
    pub aggregate_usage: BenchTokenUsage,
    pub aggregate_cost: BenchCostSummary,
    pub results: Vec<BenchTaskResult>,
}

pub fn load_tasks(tasks_dir: &Path) -> Result<Vec<BenchTask>, String> {
    let entries = std::fs::read_dir(tasks_dir)
        .map_err(|err| format!("failed to read tasks dir {}: {err}", tasks_dir.display()))?;
    let mut task_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read task entry: {err}"))?;
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
            "no task fixtures found in {} (expected *.json)",
            tasks_dir.display()
        ));
    }

    let mut tasks = Vec::with_capacity(task_paths.len());
    for path in task_paths {
        let content = std::fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let task: BenchTask = serde_json::from_str(&content)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
        if task.actions.is_empty() {
            return Err(format!("fixture {} has empty actions", path.display()));
        }
        tasks.push(task);
    }
    Ok(tasks)
}

pub fn render_actions(actions: &[Action], base_url: &str) -> Result<String, String> {
    let mut rendered = Vec::with_capacity(actions.len());
    for action in actions {
        let action = match action {
            Action::Navigate { url } => Action::Navigate {
                url: url.replace(BASE_URL_TOKEN, base_url),
            },
            other => other.clone(),
        };
        rendered.push(action);
    }
    serde_json::to_string_pretty(&rendered)
        .map_err(|err| format!("failed to render actions: {err}"))
}

pub fn write_actions_file(path: &Path, actions_json: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create actions file directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    std::fs::write(path, actions_json)
        .map_err(|err| format!("failed to write actions file {}: {err}", path.display()))
}

pub fn evaluate_task(
    task: &BenchTask,
    status: BenchObservedStatus,
    steps: usize,
    final_url: Option<&str>,
    final_visible_text: Option<&str>,
    max_steps_per_task: usize,
    run_error: Option<&str>,
    usage: BenchTokenUsage,
) -> BenchTaskResult {
    let mut passed = true;
    let mut failure_reason = None;

    if let Some(err) = run_error {
        passed = false;
        failure_reason = Some(format!("run_error: {err}"));
    } else {
        let expected_status = match task.expect.status {
            BenchExpectedStatus::Done => BenchObservedStatus::Done,
            BenchExpectedStatus::MaxSteps => BenchObservedStatus::MaxSteps,
        };
        if status != expected_status {
            passed = false;
            failure_reason = Some(format!(
                "status_mismatch: expected {:?}, got {:?}",
                expected_status, status
            ));
        } else if steps > max_steps_per_task {
            passed = false;
            failure_reason = Some(format!(
                "step_limit_exceeded: steps={} limit={}",
                steps, max_steps_per_task
            ));
        }
        if passed {
            if let Some(expected) = task.expect.final_url_contains.as_deref() {
                let observed = final_url.unwrap_or_default();
                if !observed.contains(expected) {
                    passed = false;
                    failure_reason = Some(format!(
                        "final_url_mismatch: expected substring '{expected}', observed '{observed}'"
                    ));
                }
            }
        }
        if passed {
            if let Some(expected) = task.expect.final_visible_text_contains.as_deref() {
                let observed = final_visible_text.unwrap_or_default();
                if !observed.contains(expected) {
                    passed = false;
                    failure_reason = Some(format!(
                        "visible_text_mismatch: expected substring '{expected}'"
                    ));
                }
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
    }
}

pub fn evaluate_gate(results: &[BenchTaskResult], required_passes: usize) -> BenchGate {
    let total = results.len();
    let passed = results.iter().filter(|result| result.passed).count();
    let gate_passed = passed >= required_passes;
    let reason = if gate_passed {
        None
    } else {
        Some(format!(
            "passed {} of {} tasks (required {})",
            passed, total, required_passes
        ))
    };

    BenchGate {
        total_tasks: total,
        passed_tasks: passed,
        required_passes,
        passed: gate_passed,
        reason,
    }
}

pub fn build_summary(results: &[BenchTaskResult], gate: &BenchGate) -> BenchSummary {
    let total = results.len();
    let passed = results.iter().filter(|result| result.passed).count();
    let completion_rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    };
    let mut success_steps: Vec<u64> = results
        .iter()
        .filter(|result| result.passed)
        .map(|result| result.steps as u64)
        .collect();
    success_steps.sort_unstable();

    let median_steps_success = percentile_rounded(&success_steps, 50);
    let p95_steps_success = percentile_rounded(&success_steps, 95);

    BenchSummary {
        total_tasks: total,
        passed_tasks: passed,
        required_passes: gate.required_passes,
        completion_rate,
        median_steps_success,
        p95_steps_success,
        gate_passed: gate.passed,
    }
}

fn percentile_rounded(values: &[u64], percentile: u8) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    if percentile == 0 {
        return Some(values[0]);
    }
    let rank = ((values.len() - 1) * percentile as usize + 99) / 100;
    values.get(rank).copied()
}

pub fn write_report(report_path: &Path, report: &BenchReport) -> Result<(), String> {
    if let Some(parent) = report_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create report directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let data = serde_json::to_vec_pretty(report)
        .map_err(|err| format!("failed to serialize report: {err}"))?;
    std::fs::write(report_path, data)
        .map_err(|err| format!("failed to write report {}: {err}", report_path.display()))
}

pub fn bench_task_limit(task: &BenchTask, global_max_steps: usize) -> usize {
    task.max_steps
        .map(|value| value.min(global_max_steps))
        .unwrap_or(global_max_steps)
}

pub fn join_base_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub fn failure_buckets(results: &[BenchTaskResult]) -> BTreeMap<String, usize> {
    let mut buckets = BTreeMap::new();
    for result in results {
        if result.passed {
            continue;
        }
        let key = result
            .failure_reason
            .as_deref()
            .and_then(|reason| reason.split(':').next())
            .unwrap_or("unknown")
            .to_string();
        *buckets.entry(key).or_insert(0) += 1;
    }
    buckets
}

pub struct BenchServer {
    addr: std::net::SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl BenchServer {
    pub async fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|err| format!("bind bench server: {err}"))?;
        let addr = listener
            .local_addr()
            .map_err(|err| format!("bench server address: {err}"))?;
        let (shutdown, mut shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept = listener.accept() => {
                        match accept {
                            Ok((socket, _)) => {
                                tokio::spawn(async move {
                                    let _ = handle_connection(socket).await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        Ok(Self {
            addr,
            shutdown: Some(shutdown),
            handle: Some(handle),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for BenchServer {
    fn drop(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn handle_connection(mut socket: TcpStream) -> std::io::Result<()> {
    let mut buffer = [0u8; 8192];
    let read = socket.read(&mut buffer).await?;
    if read == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut parts = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");
    let (status, body, content_type) = route_request(method, path);
    let body_bytes = body.as_bytes();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_bytes.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    Ok(())
}

fn route_request(method: &str, path: &str) -> (&'static str, String, &'static str) {
    if method != "GET" {
        return (
            "405 Method Not Allowed",
            "method not allowed".to_string(),
            "text/plain",
        );
    }

    if let Some(body) = bench_page(path) {
        return ("200 OK", body, "text/html; charset=utf-8");
    }

    (
        "404 Not Found",
        "not found".to_string(),
        "text/plain; charset=utf-8",
    )
}

fn bench_page(path: &str) -> Option<String> {
    let relative = if path == "/" || path == "/bench/start" {
        Some("bench/start.html".to_string())
    } else if let Some(suffix) = path.strip_prefix("/bench/task-") {
        let id = suffix
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if id.len() == 2 {
            Some(format!("bench/task-{id}.html"))
        } else {
            None
        }
    } else {
        None
    }?;
    let full_path = Path::new("harness/pages").join(relative);
    std::fs::read_to_string(full_path).ok()
}

pub fn now_timestamp() -> Result<String, String> {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| format!("timestamp format error: {err}"))
}

pub fn actions_file_path(base_dir: &Path, task_id: &str) -> PathBuf {
    base_dir.join(format!("{task_id}.actions.json"))
}

pub fn report_path_default() -> PathBuf {
    PathBuf::from("target/bench/report.json")
}

pub fn tasks_dir_default() -> PathBuf {
    PathBuf::from("harness/tasks")
}

pub fn actions_work_dir(report_path: &Path) -> PathBuf {
    let mut dir = report_path
        .parent()
        .map(|value| value.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("target/bench"));
    dir.push("actions");
    dir
}

pub fn sleep_between_tasks() -> Duration {
    Duration::from_millis(50)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task() -> BenchTask {
        BenchTask {
            id: "bench-task-01".to_string(),
            task: "sample".to_string(),
            plan: None,
            start_path: "/bench/start".to_string(),
            max_steps: None,
            actions: vec![Action::Done {
                summary: "ok".to_string(),
            }],
            expect: BenchExpectations::default(),
        }
    }

    #[test]
    fn join_base_url_normalizes_slashes() {
        let url = join_base_url("http://127.0.0.1:4000/", "/bench/start");
        assert_eq!(url, "http://127.0.0.1:4000/bench/start");
    }

    #[test]
    fn render_actions_replaces_base_token() {
        let actions = vec![Action::Navigate {
            url: "{{base_url}}/bench/task-01".to_string(),
        }];
        let json = render_actions(&actions, "http://127.0.0.1:1234").expect("json");
        assert!(json.contains("http://127.0.0.1:1234/bench/task-01"));
    }

    #[test]
    fn percentile_handles_empty() {
        assert_eq!(percentile_rounded(&[], 95), None);
    }

    #[test]
    fn evaluate_task_fails_when_step_limit_exceeded() {
        let task = sample_task();
        let result = evaluate_task(
            &task,
            BenchObservedStatus::Done,
            3,
            Some("http://127.0.0.1/bench/task-01"),
            Some("BENCH TASK 01 READY"),
            2,
            None,
            BenchTokenUsage {
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
                error: Some("missing_usage".to_string()),
            },
        );
        assert!(!result.passed);
        assert!(
            result
                .failure_reason
                .as_deref()
                .unwrap_or_default()
                .starts_with("step_limit_exceeded")
        );
    }

    #[test]
    fn evaluate_gate_reports_pass_and_failure_reason() {
        let passed = BenchTaskResult {
            task_id: "t1".to_string(),
            passed: true,
            status: BenchObservedStatus::Done,
            steps: 2,
            duration_ms: 1,
            usage: BenchTokenUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
                error: None,
            },
            failure_reason: None,
        };
        let failed = BenchTaskResult {
            task_id: "t2".to_string(),
            passed: false,
            status: BenchObservedStatus::Error,
            steps: 1,
            duration_ms: 1,
            usage: BenchTokenUsage {
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
                error: Some("missing_usage".to_string()),
            },
            failure_reason: Some("run_error: boom".to_string()),
        };
        let gate = evaluate_gate(&[passed.clone(), failed.clone()], 1);
        assert!(gate.passed);
        assert!(gate.reason.is_none());

        let gate = evaluate_gate(&[passed, failed], 2);
        assert!(!gate.passed);
        assert!(gate.reason.unwrap_or_default().contains("required 2"));
    }
}
