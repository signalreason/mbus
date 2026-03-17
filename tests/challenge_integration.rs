use mbus::types::{Action, ElementRef, Observation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct MockOpenAiServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockOpenAiServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock openai");
        listener
            .set_nonblocking(true)
            .expect("configure mock openai");
        let addr = listener.local_addr().expect("mock openai addr");
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = shutdown.clone();
        let handle = thread::spawn(move || {
            while !shutdown_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = thread::spawn(move || {
                            handle_connection(stream);
                        });
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        });
        Self {
            addr,
            shutdown,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MockOpenAiServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(_) => return,
    };
    let response = std::panic::catch_unwind(|| build_response(&request))
        .unwrap_or_else(|_| fallback_response());
    let _ = stream.write_all(response.as_bytes());
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none()
            && let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
        {
            header_end = Some(index + 4);
            let headers = String::from_utf8_lossy(&buffer[..index + 4]);
            content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
        }

        if let Some(end) = header_end
            && buffer.len() >= end + content_length
        {
            break;
        }
    }

    String::from_utf8(buffer)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

fn build_response(request: &str) -> String {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default();
    let payload: Value = serde_json::from_str(body).expect("request json");
    let prompt = prompt_text(&payload);
    let observation = parse_observation(&prompt);
    let task = parse_task(&prompt);
    let action = decide_action(task, &observation);
    let content = serde_json::to_string(&action).expect("action json");
    let response_body = json!({
        "choices": [
            {
                "message": {
                    "content": content
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120
        }
    })
    .to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    )
}

fn fallback_response() -> String {
    let response_body = json!({
        "choices": [
            {
                "message": {
                    "content": serde_json::to_string(&done()).expect("done action")
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120
        }
    })
    .to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    )
}

fn prompt_text(payload: &Value) -> String {
    let content = &payload["messages"][1]["content"];
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                if part.get("type") == Some(&json!("text")) {
                    part.get("text").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("unexpected content: {other:?}"),
    }
}

fn parse_task(prompt: &str) -> &str {
    let start = prompt.find("Task: ").expect("task label") + "Task: ".len();
    let end = prompt.find("\nPlan:").expect("plan label");
    &prompt[start..end]
}

fn parse_observation(prompt: &str) -> Observation {
    let start = prompt.find("Observation: ").expect("observation label") + "Observation: ".len();
    let end = prompt
        .find("\nRecentObservations:")
        .expect("recent observations label");
    serde_json::from_str(&prompt[start..end]).expect("observation json")
}

fn decide_action(task: &str, observation: &Observation) -> Action {
    if observation.visible_text.contains("COOKIE BANNER DISMISSED")
        || observation.visible_text.contains("NEWSLETTER CLOSED")
        || observation.visible_text.contains("CONSENT SAVED")
        || observation.visible_text.contains("PRICING PANEL READY")
        || observation.visible_text.contains("WAITLIST JOINED")
        || observation.visible_text.contains("SHOWROOM OPEN")
        || observation.visible_text.contains("CHECKOUT READY")
        || observation.visible_text.contains("FINAL CARD LOADED")
        || observation.visible_text.contains("RETURNS OPEN")
        || observation.visible_text.contains("DELIVERS IN 2 DAYS")
        || observation.visible_text.contains("OFFER DISMISSED")
        || observation.visible_text.contains("DEMO REQUESTED")
        || observation
            .visible_text
            .contains("INJECTION BANNER DISMISSED")
        || observation.visible_text.contains("DRAFT PUBLISHED")
    {
        return Action::Done {
            summary: "goal reached".to_string(),
        };
    }

    match task {
        text if text.contains("cookie banner") => {
            if has_named_element(observation, "Accept cookies") {
                click_named(observation, "Accept cookies")
            } else {
                done()
            }
        }
        text if text.contains("newsletter popup") => {
            if has_named_element(observation, "Close newsletter popup") {
                click_named(observation, "Close newsletter popup")
            } else {
                done()
            }
        }
        text if text.contains("sticky analytics consent") => {
            if has_named_element(observation, "Accept analytics") {
                click_named(observation, "Accept analytics")
            } else {
                done()
            }
        }
        text if text.contains("Reveal pricing panel") => {
            if has_named_element(observation, "Reveal pricing panel") {
                click_named(observation, "Reveal pricing panel")
            } else {
                Action::Scroll { dx: 0, dy: 1400 }
            }
        }
        text if text.contains("Type an email address") => {
            if !has_named_element(observation, "Join waitlist") {
                return done();
            }
            let email = find_named_element(observation, "Work email")
                .or_else(|| find_named_element(observation, "email"))
                .expect("email field");
            if email.value.as_deref().unwrap_or_default().is_empty() {
                Action::Type {
                    id: email.id.clone(),
                    text: "qa@example.com".to_string(),
                    submit: None,
                }
            } else {
                click_named(observation, "Join waitlist")
            }
        }
        text if text.contains("interstitial") => {
            if has_named_element(observation, "Enter showroom") {
                click_named(observation, "Enter showroom")
            } else {
                done()
            }
        }
        text if text.contains("Continue button for checkout") => {
            if has_named_element(observation, "Continue to checkout") {
                click_named(observation, "Continue to checkout")
            } else {
                done()
            }
        }
        text if text.contains("activity feed") => Action::Scroll { dx: 0, dy: 1400 },
        text if text.contains("return policy accordion") => {
            click_named(observation, "Show return policy")
        }
        text if text.contains("Shipping details tab") => {
            if has_named_element(observation, "Shipping details") {
                click_named(observation, "Shipping details")
            } else {
                done()
            }
        }
        text if text.contains("offer bar") => {
            if has_named_element(observation, "Dismiss offer") {
                click_named(observation, "Dismiss offer")
            } else {
                done()
            }
        }
        text if text.contains("Agree to the terms") => {
            if !has_named_element(observation, "Request demo") {
                return done();
            }
            let checkbox =
                find_named_element(observation, "I agree to the terms").expect("checkbox");
            if checkbox.flags.checked != Some(true) {
                Action::Click {
                    id: checkbox.id.clone(),
                }
            } else {
                click_named(observation, "Request demo")
            }
        }
        text if text.contains("fake instruction banner") => {
            if has_named_element(observation, "Dismiss malicious banner") {
                click_named(observation, "Dismiss malicious banner")
            } else {
                done()
            }
        }
        text if text.contains("Publish the draft") => {
            if has_named_element(observation, "Publish draft") {
                click_named(observation, "Publish draft")
            } else {
                done()
            }
        }
        other => panic!("unhandled task: {other}"),
    }
}

fn done() -> Action {
    Action::Done {
        summary: "goal reached".to_string(),
    }
}

fn has_named_element(observation: &Observation, name: &str) -> bool {
    find_named_element(observation, name).is_some()
}

fn click_named(observation: &Observation, name: &str) -> Action {
    let element = find_named_element(observation, name).unwrap_or_else(|| {
        panic!(
            "missing element named {name} in {}",
            observation.visible_text
        )
    });
    Action::Click {
        id: element.id.clone(),
    }
}

fn find_named_element<'a>(observation: &'a Observation, name: &str) -> Option<&'a ElementRef> {
    observation
        .elements
        .iter()
        .find(|element| element_name(element).eq_ignore_ascii_case(name))
}

fn element_name(element: &ElementRef) -> &str {
    element.name.as_deref().unwrap_or_default()
}

fn temp_report_path(label: &str) -> std::path::PathBuf {
    temp_path(label, "json")
}

fn temp_path(label: &str, extension: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("mbus-{label}-{nanos}.{extension}"))
}

fn temp_dir_path(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("mbus-{label}-{nanos}"))
}

fn run_binary(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mbus"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run mbus")
}

fn read_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).expect("read report");
    serde_json::from_slice(&bytes).expect("parse report")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[test]
fn challenge_command_generates_report_with_artifacts() {
    let server = MockOpenAiServer::start();
    let report_path = temp_report_path("challenge-report");
    let report_arg = report_path.to_string_lossy().into_owned();
    let base_url = server.base_url();

    let output = run_binary(&[
        "challenge",
        "--report-path",
        &report_arg,
        "--llm-base-url",
        &base_url,
        "--llm-api-key",
        "test-key",
        "--llm-input-cost-per-million",
        "1.0",
        "--llm-output-cost-per-million",
        "2.0",
        "--headless",
        "true",
    ]);

    server.shutdown();

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_json(&report_path);
    assert_eq!(report["llm"]["mode"], json!("openai"));
    assert_eq!(report["summary"]["total_tasks"], json!(12));
    assert_eq!(report["gate"]["passed"], json!(true));
    assert!(report["summary"]["passed_tasks"].as_u64().unwrap_or(0) >= 10);
    assert!(
        report["aggregate_usage"]["total_tokens"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    assert!(
        report["aggregate_cost"]["total_cost_usd"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0
    );
    let results = report["results"].as_array().expect("results");
    assert_eq!(results.len(), 12);
    let passed_results = results
        .iter()
        .filter(|result| result["passed"] == json!(true))
        .count();
    assert!(passed_results >= 10);
    for result in results {
        if result["passed"] == json!(true) {
            let artifacts = result["output_artifacts"].as_array().expect("artifacts");
            assert!(
                !artifacts.is_empty(),
                "expected persisted artifacts in {result:?}"
            );
            assert!(
                artifacts
                    .iter()
                    .any(|artifact| artifact["kind"] == json!("screenshot"))
            );
        }
    }
}

#[test]
fn challenge_command_supports_supplemental_adversarial_tasks() {
    let server = MockOpenAiServer::start();
    let report_path = temp_report_path("challenge-adversarial-report");
    let report_arg = report_path.to_string_lossy().into_owned();
    let base_url = server.base_url();

    let output = run_binary(&[
        "challenge",
        "--tasks-dir",
        "harness/challenge_adversarial",
        "--required-passes",
        "2",
        "--report-path",
        &report_arg,
        "--llm-base-url",
        &base_url,
        "--llm-api-key",
        "test-key",
        "--llm-input-cost-per-million",
        "1.0",
        "--llm-output-cost-per-million",
        "2.0",
        "--headless",
        "true",
    ]);

    server.shutdown();

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_json(&report_path);
    assert_eq!(report["summary"]["total_tasks"], json!(2));
    assert_eq!(report["summary"]["passed_tasks"], json!(2));
    assert_eq!(report["gate"]["passed"], json!(true));
}

#[test]
fn bench_scripted_command_still_passes() {
    let report_path = temp_report_path("bench-report");
    let report_arg = report_path.to_string_lossy().into_owned();

    let output = run_binary(&[
        "bench",
        "--llm-mode",
        "scripted",
        "--report-path",
        &report_arg,
        "--headless",
        "true",
    ]);

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_json(&report_path);
    assert_eq!(report["llm"]["mode"], json!("scripted"));
    assert_eq!(report["summary"]["total_tasks"], json!(10));
    assert_eq!(report["summary"]["passed_tasks"], json!(10));
    assert_eq!(report["gate"]["passed"], json!(true));
    assert_eq!(report["failure_buckets"], json!({}));
}

#[test]
fn package_command_bundles_challenge_report_and_artifacts() {
    let server = MockOpenAiServer::start();
    let report_path = temp_report_path("challenge-package-report");
    let package_dir = temp_dir_path("challenge-package-dir");
    let zip_path = temp_path("challenge-package-zip", "zip");
    let report_arg = report_path.to_string_lossy().into_owned();
    let package_dir_arg = package_dir.to_string_lossy().into_owned();
    let zip_arg = zip_path.to_string_lossy().into_owned();
    let base_url = server.base_url();

    let challenge_output = run_binary(&[
        "challenge",
        "--report-path",
        &report_arg,
        "--llm-base-url",
        &base_url,
        "--llm-api-key",
        "test-key",
        "--llm-input-cost-per-million",
        "1.0",
        "--llm-output-cost-per-million",
        "2.0",
        "--headless",
        "true",
    ]);

    server.shutdown();

    assert!(
        challenge_output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&challenge_output.stdout),
        String::from_utf8_lossy(&challenge_output.stderr)
    );

    let package_output = run_binary(&[
        "package",
        "--report-path",
        &report_arg,
        "--output-dir",
        &package_dir_arg,
        "--zip-path",
        &zip_arg,
    ]);

    assert!(
        package_output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&package_output.stdout),
        String::from_utf8_lossy(&package_output.stderr)
    );

    assert!(package_dir.join("report.json").exists());
    assert!(package_dir.join("manifest.json").exists());
    assert!(package_dir.join("README.md").exists());
    assert!(zip_path.exists());

    let source_report = read_json(&report_path);
    let packaged_report = read_json(&package_dir.join("report.json"));
    assert_eq!(source_report, packaged_report);

    let manifest = read_json(&package_dir.join("manifest.json"));
    let files = manifest["files"].as_array().expect("manifest files");
    assert!(!files.is_empty());
    for entry in files {
        let relative = entry["path"].as_str().expect("relative path");
        let bytes = std::fs::read(package_dir.join(relative)).expect("packaged file");
        let digest = sha256_hex(&bytes);
        assert_eq!(entry["bytes"].as_u64(), Some(bytes.len() as u64));
        assert_eq!(entry["sha256"].as_str(), Some(digest.as_str()));
    }

    let artifact_entry = files
        .iter()
        .find(|entry| {
            entry["path"]
                .as_str()
                .map(|value| value.starts_with("artifacts/"))
                .unwrap_or(false)
        })
        .expect("artifact entry");
    assert!(
        package_dir
            .join(artifact_entry["path"].as_str().unwrap())
            .exists()
    );

    let archive_file = std::fs::File::open(&zip_path).expect("zip file");
    let mut archive = zip::ZipArchive::new(archive_file).expect("zip archive");
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).expect("zip entry");
        names.push(file.name().to_string());
    }
    assert!(names.iter().any(|name| name == "report.json"));
    assert!(names.iter().any(|name| name == "manifest.json"));
    assert!(names.iter().any(|name| name == "README.md"));
    assert!(names.iter().any(|name| name.starts_with("artifacts/")));
}

#[test]
fn package_command_fails_when_artifact_is_missing() {
    let server = MockOpenAiServer::start();
    let report_path = temp_report_path("challenge-package-missing");
    let package_dir = temp_dir_path("challenge-package-missing-dir");
    let zip_path = temp_path("challenge-package-missing-zip", "zip");
    let report_arg = report_path.to_string_lossy().into_owned();
    let package_dir_arg = package_dir.to_string_lossy().into_owned();
    let zip_arg = zip_path.to_string_lossy().into_owned();
    let base_url = server.base_url();

    let challenge_output = run_binary(&[
        "challenge",
        "--report-path",
        &report_arg,
        "--llm-base-url",
        &base_url,
        "--llm-api-key",
        "test-key",
        "--headless",
        "true",
    ]);

    server.shutdown();

    assert!(
        challenge_output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&challenge_output.stdout),
        String::from_utf8_lossy(&challenge_output.stderr)
    );

    let report = read_json(&report_path);
    let artifact_path = report["results"]
        .as_array()
        .expect("results")
        .iter()
        .flat_map(|result| {
            result["output_artifacts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|artifact| artifact["path"].as_str())
        })
        .next()
        .expect("artifact path");
    std::fs::remove_file(artifact_path).expect("remove artifact");

    let package_output = run_binary(&[
        "package",
        "--report-path",
        &report_arg,
        "--output-dir",
        &package_dir_arg,
        "--zip-path",
        &zip_arg,
    ]);

    assert!(
        !package_output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&package_output.stdout),
        String::from_utf8_lossy(&package_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&package_output.stderr).contains("failed to read artifact"),
        "stderr:\n{}",
        String::from_utf8_lossy(&package_output.stderr)
    );
}
