use async_trait::async_trait;
use mbus::agent::r#loop::{AgentLoop, LlmClients, RunStatus};
use mbus::agent::policy::AgentPolicy;
use mbus::browser::{Browser, CdpBrowser, CdpConfig};
use mbus::llm::client::{LlmClient, LlmContext, LlmError, LlmResponse};
use mbus::llm::router::{Router, RouterConfig, RouterLadderStep, Tier};
use mbus::types::{Action, ElementRef, LlmPayloadMode, Observation, ReasoningEffort};
use mbus::verify::{Validator, ValidatorConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};

const HARNESS_PAGE_PATH: &str = "harness/pages/actions.html";
const HARNESS_LOOP_PAGE_PATH: &str = "harness/pages/loop.html";

struct TestServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("server addr");
        let (shutdown, mut shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
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

        Self {
            addr,
            shutdown: Some(shutdown),
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for TestServer {
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

fn load_harness_page() -> String {
    std::fs::read_to_string(HARNESS_PAGE_PATH).expect("read harness page")
}

fn load_harness_loop_page() -> String {
    std::fs::read_to_string(HARNESS_LOOP_PAGE_PATH).expect("read harness loop page")
}

fn route_request(method: &str, path: &str) -> (&'static str, String, &'static str) {
    if method != "GET" {
        return (
            "405 Method Not Allowed",
            "method not allowed".to_string(),
            "text/plain",
        );
    }
    match path {
        "/" | "/harness" => ("200 OK", load_harness_page(), "text/html; charset=utf-8"),
        "/loop" => (
            "200 OK",
            load_harness_loop_page(),
            "text/html; charset=utf-8",
        ),
        "/favicon.ico" => ("404 Not Found", "not found".to_string(), "text/plain"),
        _ => ("404 Not Found", "not found".to_string(), "text/plain"),
    }
}

fn find_element<'a>(observation: &'a Observation, role: &str, name: &str) -> &'a ElementRef {
    observation
        .elements
        .iter()
        .find(|element| {
            element.role.eq_ignore_ascii_case(role)
                && element
                    .name
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing element role={role} name={name}"))
}

fn find_element_by_roles<'a>(
    observation: &'a Observation,
    roles: &[&str],
    name: &str,
) -> &'a ElementRef {
    observation
        .elements
        .iter()
        .find(|element| {
            roles
                .iter()
                .any(|role| element.role.eq_ignore_ascii_case(role))
                && element
                    .name
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing element roles={roles:?} name={name}"))
}

async fn wait_for_visible_text(browser: &CdpBrowser, needle: &str) -> Observation {
    for _ in 0..20 {
        let snapshot = browser.snapshot().await.expect("snapshot retry");
        if snapshot.visible_text.contains(needle) {
            return snapshot;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("expected visible text to include {needle}");
}

#[derive(Clone, Copy, Debug)]
enum HarnessMode {
    Click,
    Type,
}

#[derive(Clone, Debug)]
struct HarnessLlm {
    mode: HarnessMode,
    step: Arc<Mutex<usize>>,
}

impl HarnessLlm {
    fn new(mode: HarnessMode) -> Self {
        Self {
            mode,
            step: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmClient for HarnessLlm {
    async fn propose_action(&self, context: &LlmContext<'_>) -> Result<LlmResponse, LlmError> {
        let mut guard = self.step.lock().await;
        let observation = context.observation;
        let action = match (self.mode, *guard) {
            (HarnessMode::Click, 0) => {
                let id = find_element(observation, "button", "Click Button")
                    .id
                    .clone();
                Action::Click { id }
            }
            (HarnessMode::Type, 0) => {
                let id = find_element_by_roles(observation, &["textbox", "searchbox"], "Name")
                    .id
                    .clone();
                Action::Type {
                    id,
                    text: "Ada Lovelace".to_string(),
                    submit: Some(false),
                }
            }
            _ => Action::Done {
                summary: "done".to_string(),
            },
        };
        *guard += 1;
        Ok(LlmResponse {
            action,
            usage: None,
            payload_mode: LlmPayloadMode::TextOnly,
        })
    }
}

#[derive(Clone, Debug)]
struct WaitLoopLlm {
    step: Arc<Mutex<usize>>,
}

impl WaitLoopLlm {
    fn new() -> Self {
        Self {
            step: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmClient for WaitLoopLlm {
    async fn propose_action(&self, _context: &LlmContext<'_>) -> Result<LlmResponse, LlmError> {
        let mut guard = self.step.lock().await;
        let waits = [15_u64, 25, 35];
        let action = if let Some(wait_ms) = waits.get(*guard) {
            Action::Wait { ms: *wait_ms }
        } else {
            Action::Done {
                summary: "done".to_string(),
            }
        };
        *guard += 1;
        Ok(LlmResponse {
            action,
            usage: None,
            payload_mode: LlmPayloadMode::TextOnly,
        })
    }
}

#[derive(Clone, Debug)]
struct RepeatWaitLlm {
    wait_ms: u64,
}

impl RepeatWaitLlm {
    fn new(wait_ms: u64) -> Self {
        Self { wait_ms }
    }
}

#[async_trait]
impl LlmClient for RepeatWaitLlm {
    async fn propose_action(&self, _context: &LlmContext<'_>) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            action: Action::Wait { ms: self.wait_ms },
            usage: None,
            payload_mode: LlmPayloadMode::TextOnly,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_click_updates_status_via_agent_loop() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let llm = HarnessLlm::new(HarnessMode::Click);
    let clients = LlmClients::new(Box::new(llm.clone()), Box::new(llm.clone()), Box::new(llm));
    let mut agent = AgentLoop::new(browser, clients, "click test").with_policy(AgentPolicy {
        max_steps: 2,
        ..AgentPolicy::default()
    });

    let result = agent.run().await.expect("run");
    assert_eq!(result.status, RunStatus::Done);
    assert_eq!(result.steps.len(), 2);
    assert!(result.steps[0].result.ok);
    assert!(matches!(result.steps[0].action, Action::Click { .. }));
    assert!(
        result.final_observation.visible_text.contains("clicked"),
        "expected status to show click outcome"
    );

    agent.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_type_updates_status_via_agent_loop() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let llm = HarnessLlm::new(HarnessMode::Type);
    let clients = LlmClients::new(Box::new(llm.clone()), Box::new(llm.clone()), Box::new(llm));
    let mut agent = AgentLoop::new(browser, clients, "type test").with_policy(AgentPolicy {
        max_steps: 2,
        ..AgentPolicy::default()
    });

    let result = agent.run().await.expect("run");
    assert_eq!(result.status, RunStatus::Done);
    assert_eq!(result.steps.len(), 2);
    assert!(result.steps[0].result.ok);
    assert!(matches!(result.steps[0].action, Action::Type { .. }));
    assert!(
        result
            .final_observation
            .visible_text
            .contains("typed:Ada Lovelace"),
        "expected status to show typed value"
    );

    agent.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_unchanged_state_hash_loop_records_streaks() {
    let server = TestServer::start().await;
    let url = server.url("/loop");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let llm = WaitLoopLlm::new();
    let clients = LlmClients::new(Box::new(llm.clone()), Box::new(llm.clone()), Box::new(llm));
    let mut agent = AgentLoop::new(browser, clients, "no-progress test").with_policy(AgentPolicy {
        max_steps: 5,
        max_no_progress_steps: 3,
        ..AgentPolicy::default()
    });

    let result = agent.run().await.expect("run");
    assert_eq!(result.status, RunStatus::NoProgress);
    assert_eq!(result.steps.len(), 3);

    let hashes: Vec<&str> = result
        .steps
        .iter()
        .map(|step| step.result.new_state_hash.as_deref().expect("state hash"))
        .collect();
    assert!(hashes.iter().all(|hash| *hash == hashes[0]));
    assert_eq!(hashes[0], result.final_observation.state_hash);

    let streaks: Vec<u32> = result
        .steps
        .iter()
        .map(|step| {
            step.router
                .as_ref()
                .expect("router info")
                .triggers
                .state_hash_streak
        })
        .collect();
    assert_eq!(streaks, vec![1, 2, 3]);

    let no_progress: Vec<u32> = result
        .steps
        .iter()
        .map(|step| {
            step.router
                .as_ref()
                .expect("router info")
                .counters
                .no_progress
        })
        .collect();
    assert_eq!(no_progress, vec![1, 2, 3]);

    agent.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_repeat_validation_code_escalates_ladder() {
    let server = TestServer::start().await;
    let url = server.url("/loop");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let llm = RepeatWaitLlm::new(25);
    let clients = LlmClients::new(Box::new(llm.clone()), Box::new(llm.clone()), Box::new(llm));
    let mut router_config = RouterConfig::default();
    router_config.failures_to_mid = 10;
    router_config.failures_to_strong = 20;
    router_config.no_progress_to_mid = 10;
    router_config.no_progress_to_strong = 20;
    router_config.low_actionability_to_mid = 10;
    router_config.low_actionability_to_strong = 20;
    router_config.reasoning_effort = ReasoningEffort::Low;
    router_config.ladder = vec![
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
        RouterLadderStep {
            model: "strong".to_string(),
            tier: Tier::Strong,
            effort: ReasoningEffort::High,
        },
    ];
    let router = Router::new(router_config);
    let mut agent = AgentLoop::new(browser, clients, "repeat-validation test")
        .with_router(router)
        .with_policy(AgentPolicy {
            max_steps: 8,
            max_no_progress_steps: 10,
            ..AgentPolicy::default()
        });

    let result = agent.run().await.expect("run");
    assert_eq!(result.status, RunStatus::NoProgress);
    assert_eq!(result.steps.len(), 4);
    assert_eq!(
        result.final_action,
        Action::Done {
            summary: "no progress after 3 repeated blocked actions".to_string()
        }
    );

    let hashes: Vec<&str> = result
        .steps
        .iter()
        .map(|step| step.result.new_state_hash.as_deref().expect("state hash"))
        .collect();
    assert!(hashes.iter().all(|hash| *hash == hashes[0]));

    for step in result.steps.iter().skip(1) {
        assert!(!step.validation.ok);
        assert_eq!(
            step.result
                .error
                .as_ref()
                .and_then(|error| error.validation_code.as_deref()),
            Some("repeat_no_progress_action")
        );
    }

    let transition_step = result
        .steps
        .iter()
        .find(|step| {
            step.router.as_ref().is_some_and(|router| {
                router
                    .transitions
                    .iter()
                    .any(|transition| transition.reason_code == "repeat_validation_code")
            })
        })
        .expect("expected repeat_validation_code transition");
    let router_info = transition_step.router.as_ref().expect("router info");
    assert!(
        router_info.ladder_index >= 1,
        "expected ladder index to advance on repeat validation"
    );
    let transition = router_info
        .transitions
        .iter()
        .find(|transition| transition.reason_code == "repeat_validation_code")
        .expect("repeat_validation_code transition");
    assert_eq!(
        transition.validation_code.as_deref(),
        Some("repeat_no_progress_action")
    );
    assert_eq!(transition.streak, Some(2));

    agent.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_snapshot_metadata() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url.clone(),
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");

    let snapshot = browser.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.url, url, "snapshot should report page url");
    assert_eq!(
        snapshot.title, "mbus harness actions",
        "snapshot should report title"
    );
    assert!(
        snapshot.viewport[0] > 0 && snapshot.viewport[1] > 0,
        "snapshot should report non-zero viewport"
    );

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_snapshot_screenshot_bytes() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        screenshot_enabled: true,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");

    let _snapshot = browser.snapshot().await.expect("snapshot");
    let screenshot = browser
        .take_last_screenshot()
        .await
        .expect("screenshot capture")
        .bytes
        .expect("screenshot bytes");
    assert!(
        !screenshot.is_empty(),
        "expected non-empty screenshot bytes"
    );

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_snapshot_actionable_nodes() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");

    let snapshot = browser.snapshot().await.expect("snapshot");
    assert!(
        snapshot.elements.len() >= 10,
        "expected at least 10 actionable elements, got {}",
        snapshot.elements.len()
    );

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_click_type_select() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let validator = Validator::default();

    let snapshot = browser.snapshot().await.expect("initial snapshot");
    let click_id = find_element(&snapshot, "button", "Click Button").id.clone();
    let click = Action::Click { id: click_id };
    validator.validate(&click, &snapshot).expect("valid click");
    let step = browser.apply(&click).await.expect("apply click");
    assert!(step.ok);

    let snapshot = browser.snapshot().await.expect("snapshot after click");
    assert!(
        snapshot.visible_text.contains("clicked"),
        "visible text should reflect click"
    );

    let type_id = find_element_by_roles(&snapshot, &["textbox", "searchbox"], "Name")
        .id
        .clone();
    let focus = Action::Click {
        id: type_id.clone(),
    };
    validator.validate(&focus, &snapshot).expect("valid focus");
    let step = browser.apply(&focus).await.expect("apply focus");
    assert!(step.ok);

    let typed = Action::Type {
        id: type_id,
        text: "Ada Lovelace".to_string(),
        submit: Some(false),
    };
    validator.validate(&typed, &snapshot).expect("valid type");
    let step = browser.apply(&typed).await.expect("apply type");
    assert!(step.ok);

    let snapshot = wait_for_visible_text(&browser, "typed:Ada Lovelace").await;

    let submit_id = find_element_by_roles(&snapshot, &["textbox", "searchbox"], "Search")
        .id
        .clone();
    let submit = Action::Type {
        id: submit_id,
        text: "Lambda".to_string(),
        submit: Some(true),
    };
    validator
        .validate(&submit, &snapshot)
        .expect("valid submit type");
    let step = browser.apply(&submit).await.expect("apply submit type");
    assert!(step.ok);

    let snapshot = wait_for_visible_text(&browser, "submitted:Lambda").await;

    let select_id = find_element_by_roles(&snapshot, &["combobox", "listbox"], "Choice")
        .id
        .clone();
    let select = Action::Select {
        id: select_id.clone(),
        value: "beta".to_string(),
    };
    validator
        .validate(&select, &snapshot)
        .expect("valid select");
    let step = browser.apply(&select).await.expect("apply select");
    assert!(step.ok);

    let snapshot = wait_for_visible_text(&browser, "selected:beta").await;
    assert!(
        snapshot.visible_text.contains("selected:beta"),
        "visible text should reflect select change"
    );

    let invalid_select = Action::Select {
        id: select_id,
        value: "delta".to_string(),
    };
    validator
        .validate(&invalid_select, &snapshot)
        .expect("valid select action");
    let step = browser
        .apply(&invalid_select)
        .await
        .expect("apply invalid select");
    assert!(!step.ok, "invalid select should fail");
    let error = step.error.expect("invalid select error");
    assert_eq!(error.code, "select_failed");
    assert_eq!(error.message, "invalid_option");

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_select_updates_status_and_rejects_invalid_option() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let validator = Validator::default();

    let snapshot = browser.snapshot().await.expect("initial snapshot");
    let select_id = find_element_by_roles(&snapshot, &["combobox", "listbox"], "Choice")
        .id
        .clone();

    let select = Action::Select {
        id: select_id.clone(),
        value: "alpha".to_string(),
    };
    validator
        .validate(&select, &snapshot)
        .expect("valid select");
    let step = browser.apply(&select).await.expect("apply select");
    assert!(step.ok);

    let snapshot = wait_for_visible_text(&browser, "selected:alpha").await;
    assert!(
        snapshot.visible_text.contains("selected:alpha"),
        "visible text should reflect select change"
    );

    let invalid_select = Action::Select {
        id: select_id,
        value: "delta".to_string(),
    };
    validator
        .validate(&invalid_select, &snapshot)
        .expect("valid select action");
    let step = browser
        .apply(&invalid_select)
        .await
        .expect("apply invalid select");
    assert!(!step.ok, "invalid select should fail");
    let error = step.error.expect("invalid select error");
    assert_eq!(error.code, "select_failed");
    assert_eq!(error.message, "invalid_option");

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_scroll_within_bounds_and_rejects_out_of_bounds() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        max_scroll: 120,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let validator = Validator::new(ValidatorConfig {
        max_scroll: 120,
        ..ValidatorConfig::default()
    });

    let snapshot = browser.snapshot().await.expect("initial snapshot");
    let scroll = Action::Scroll { dx: 0, dy: 80 };
    validator
        .validate(&scroll, &snapshot)
        .expect("valid scroll");
    let step = browser.apply(&scroll).await.expect("apply scroll");
    assert!(step.ok);
    let coords = step.scroll.expect("scroll coords");
    assert!(coords[1] > 0.0, "expected positive scroll y");

    let invalid_scroll = Action::Scroll { dx: 0, dy: 240 };
    let errors = validator
        .validate(&invalid_scroll, &snapshot)
        .expect_err("invalid scroll should be rejected");
    assert_eq!(errors[0].code, "scroll_out_of_bounds");
    let step = browser
        .apply(&invalid_scroll)
        .await
        .expect("apply invalid scroll");
    assert!(!step.ok, "out of bounds scroll should fail");
    let error = step.error.expect("scroll error");
    assert_eq!(error.code, "scroll_out_of_bounds");

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_wait_within_bounds_and_rejects_out_of_bounds() {
    let server = TestServer::start().await;
    let url = server.url("/harness");
    let config = CdpConfig {
        initial_url: url,
        max_wait_ms: 120,
        ..CdpConfig::default()
    };
    let browser = CdpBrowser::launch(config).await.expect("launch browser");
    let validator = Validator::new(ValidatorConfig {
        max_wait_ms: 120,
        ..ValidatorConfig::default()
    });

    let snapshot = browser.snapshot().await.expect("initial snapshot");
    let wait = Action::Wait { ms: 80 };
    validator.validate(&wait, &snapshot).expect("valid wait");
    let start = Instant::now();
    let step = browser.apply(&wait).await.expect("apply wait");
    assert!(step.ok);
    assert!(
        start.elapsed() >= Duration::from_millis(80),
        "expected wait to delay at least requested duration"
    );

    let invalid_wait = Action::Wait { ms: 220 };
    let errors = validator
        .validate(&invalid_wait, &snapshot)
        .expect_err("invalid wait should be rejected");
    assert_eq!(errors[0].code, "wait_too_long");
    let step = browser
        .apply(&invalid_wait)
        .await
        .expect("apply invalid wait");
    assert!(!step.ok, "wait over max should fail");
    let error = step.error.expect("wait error");
    assert_eq!(error.code, "wait_too_long");

    browser.shutdown().await.expect("shutdown browser");
    server.shutdown().await;
}
