use crate::browser::act::{ActionApplier, ActionError, action_result_err, action_result_ok_with};
use crate::browser::observe::{Observer, ObserverConfig};
use crate::browser::{Browser, BrowserError, BrowserResult, ScreenshotCapture};
use crate::telemetry;
use crate::types::{Action, Observation, StepResult};
use async_trait::async_trait;
use chromiumoxide::browser::{Browser as ChromiumBrowser, BrowserConfig};
use chromiumoxide::detection::{DetectionOptions, default_executable};
use chromiumoxide::page::{Page, ScreenshotParams};
use chromiumoxide_cdp::cdp::browser_protocol::dom::BackendNodeId;
use chromiumoxide_cdp::cdp::browser_protocol::page::CaptureScreenshotFormat;
use futures::StreamExt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::timeout;

#[derive(Clone, Debug)]
pub struct CdpConfig {
    pub headful: bool,
    pub initial_url: String,
    pub cdp_url: Option<String>,
    pub executable_path: Option<PathBuf>,
    pub launch_timeout: Duration,
    pub no_sandbox: bool,
    pub extra_args: Vec<String>,
    pub keep_user_data_dir: bool,
    pub snapshot_timeout: Duration,
    pub action_timeout: Duration,
    pub max_elements: usize,
    pub max_text_len: usize,
    pub max_scroll: i64,
    pub max_wait_ms: u64,
    pub screenshot_enabled: bool,
}

impl Default for CdpConfig {
    fn default() -> Self {
        Self {
            headful: false,
            initial_url: "about:blank".to_string(),
            cdp_url: None,
            executable_path: None,
            launch_timeout: Duration::from_secs(20),
            no_sandbox: false,
            extra_args: Vec::new(),
            keep_user_data_dir: false,
            snapshot_timeout: Duration::from_secs(5),
            action_timeout: Duration::from_secs(10),
            max_elements: 50,
            max_text_len: 4000,
            max_scroll: 2000,
            max_wait_ms: 30_000,
            screenshot_enabled: false,
        }
    }
}

#[derive(Debug)]
struct Timeouts {
    snapshot: Duration,
    action: Duration,
}

#[derive(Debug)]
struct CdpSession {
    browser: Mutex<ChromiumBrowser>,
    page: Mutex<Page>,
    handler_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    owns_browser: bool,
    user_data_dir: Option<PathBuf>,
    keep_user_data_dir: bool,
    closed: AtomicBool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchDetails {
    executable_path: PathBuf,
    executable_source: &'static str,
    headful: bool,
    no_sandbox: bool,
    extra_args: Vec<String>,
    launch_timeout: Duration,
    user_data_dir: PathBuf,
    keep_user_data_dir: bool,
}

#[derive(Debug)]
struct PreparedLaunch {
    browser_config: BrowserConfig,
    details: LaunchDetails,
}

impl CdpSession {
    async fn launch(config: &CdpConfig) -> BrowserResult<Self> {
        let prepared = prepare_launch(config)?;
        let launch_details = prepared.details.clone();
        let (browser, mut handler) = ChromiumBrowser::launch(prepared.browser_config)
            .await
            .map_err(|err| {
                cleanup_user_data_dir(
                    &launch_details.user_data_dir,
                    launch_details.keep_user_data_dir,
                );
                launch_error("launch_process", &launch_details, err.to_string())
            })?;
        let handler_task =
            tokio::spawn(async move { while let Some(_event) = handler.next().await {} });
        let page = match create_page(&browser, &config.initial_url).await {
            Ok(page) => page,
            Err(err) => {
                let mut browser = browser;
                let _ = browser.close().await;
                cleanup_user_data_dir(
                    &launch_details.user_data_dir,
                    launch_details.keep_user_data_dir,
                );
                return Err(launch_error(
                    "initial_page",
                    &launch_details,
                    format!("{}: {}", err.code, err.message),
                ));
            }
        };

        Ok(Self {
            browser: Mutex::new(browser),
            page: Mutex::new(page),
            handler_task: Mutex::new(Some(handler_task)),
            owns_browser: true,
            user_data_dir: Some(launch_details.user_data_dir),
            keep_user_data_dir: config.keep_user_data_dir,
            closed: AtomicBool::new(false),
        })
    }

    async fn connect(config: &CdpConfig, url: &str) -> BrowserResult<Self> {
        let (browser, mut handler) = ChromiumBrowser::connect(url)
            .await
            .map_err(|err| BrowserError::new("cdp_connect_failed", err.to_string()))?;
        let handler_task =
            tokio::spawn(async move { while let Some(_event) = handler.next().await {} });
        let page = create_page(&browser, &config.initial_url).await?;

        Ok(Self {
            browser: Mutex::new(browser),
            page: Mutex::new(page),
            handler_task: Mutex::new(Some(handler_task)),
            owns_browser: false,
            user_data_dir: None,
            keep_user_data_dir: false,
            closed: AtomicBool::new(false),
        })
    }

    async fn shutdown(&self) -> BrowserResult<()> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let page = self.page.lock().await.clone();
        page.close()
            .await
            .map_err(|err| BrowserError::new("cdp_page_close_failed", err.to_string()))?;

        if self.owns_browser {
            let mut browser = self.browser.lock().await;
            browser
                .close()
                .await
                .map_err(|err| BrowserError::new("cdp_close_failed", err.to_string()))?;
        }

        let mut handler_opt = self.handler_task.lock().await;
        if let Some(mut handle) = handler_opt.take() {
            let shutdown_timer = tokio::time::sleep(Duration::from_secs(5));
            tokio::pin!(shutdown_timer);
            tokio::select! {
                result = &mut handle => {
                    if let Err(err) = result {
                        return Err(BrowserError::new("cdp_handler_failed", err.to_string()));
                    }
                }
                _ = &mut shutdown_timer => {
                    handle.abort();
                    let _ = handle.await;
                }
            }
        }

        if let Some(user_data_dir) = &self.user_data_dir {
            cleanup_user_data_dir(user_data_dir, self.keep_user_data_dir);
        }

        Ok(())
    }

    async fn page(&self) -> Page {
        self.page.lock().await.clone()
    }
}

fn unique_user_data_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("chromiumoxide-runner-{pid}-{ts}-{seq}"))
}

fn resolve_executable_path(config: &CdpConfig) -> BrowserResult<(PathBuf, &'static str)> {
    if let Some(path) = config.executable_path.as_ref() {
        return Ok((path.clone(), "explicit"));
    }
    let detected = default_executable(DetectionOptions::default()).map_err(|err| {
        BrowserError::new(
            "cdp_launch_failed",
            format!(
                "stage=resolve_executable executable=auto-detect headful={} no_sandbox={} launch_timeout_ms={} message={err}",
                config.headful,
                config.no_sandbox,
                config.launch_timeout.as_millis()
            ),
        )
    })?;
    Ok((detected, "auto-detect"))
}

fn prepare_launch(config: &CdpConfig) -> BrowserResult<PreparedLaunch> {
    let (executable_path, executable_source) = resolve_executable_path(config)?;
    let user_data_dir = unique_user_data_dir();
    std::fs::create_dir_all(&user_data_dir).map_err(|err| {
        BrowserError::new(
            "cdp_launch_failed",
            format!(
                "stage=prepare_user_data_dir executable={} executable_source={} headful={} no_sandbox={} launch_timeout_ms={} user_data_dir={} message={err}",
                executable_path.display(),
                executable_source,
                config.headful,
                config.no_sandbox,
                config.launch_timeout.as_millis(),
                user_data_dir.display(),
            ),
        )
    })?;

    let mut builder = BrowserConfig::builder()
        .chrome_executable(&executable_path)
        .launch_timeout(config.launch_timeout)
        .user_data_dir(&user_data_dir);
    if config.headful {
        builder = builder.with_head();
    }
    if config.no_sandbox {
        builder = builder.no_sandbox();
    }
    if !config.extra_args.is_empty() {
        builder = builder.args(config.extra_args.clone());
    }

    let details = LaunchDetails {
        executable_path,
        executable_source,
        headful: config.headful,
        no_sandbox: config.no_sandbox,
        extra_args: config.extra_args.clone(),
        launch_timeout: config.launch_timeout,
        user_data_dir: user_data_dir.clone(),
        keep_user_data_dir: config.keep_user_data_dir,
    };
    let browser_config = builder
        .build()
        .map_err(|err| launch_error("build_config", &details, err))?;

    Ok(PreparedLaunch {
        browser_config,
        details,
    })
}

fn cleanup_user_data_dir(path: &Path, keep_user_data_dir: bool) {
    if !keep_user_data_dir {
        let _ = std::fs::remove_dir_all(path);
    }
}

fn launch_failure_stage(message: &str) -> &'static str {
    if message.contains("before websocket URL could be resolved")
        || message.contains("resolving websocket URL")
    {
        "websocket_resolve"
    } else if message.contains("No such file or directory")
        || message.contains("os error 2")
        || message.contains("Permission denied")
    {
        "spawn_process"
    } else {
        "launch_process"
    }
}

fn launch_error(stage: &str, details: &LaunchDetails, message: impl Into<String>) -> BrowserError {
    let message = message.into();
    let effective_stage = if stage == "launch_process" {
        launch_failure_stage(&message)
    } else {
        stage
    };
    BrowserError::new(
        "cdp_launch_failed",
        format!(
            "stage={} executable={} executable_source={} headful={} no_sandbox={} launch_timeout_ms={} user_data_dir={} keep_user_data_dir={} extra_args={:?} message={}",
            effective_stage,
            details.executable_path.display(),
            details.executable_source,
            details.headful,
            details.no_sandbox,
            details.launch_timeout.as_millis(),
            details.user_data_dir.display(),
            details.keep_user_data_dir,
            details.extra_args,
            message
        ),
    )
}

async fn create_page(browser: &ChromiumBrowser, initial_url: &str) -> BrowserResult<Page> {
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|err| BrowserError::new("cdp_page_failed", err.to_string()))?;
    if !initial_url.is_empty() && initial_url != "about:blank" {
        page.goto(initial_url)
            .await
            .map_err(|err| BrowserError::new("cdp_nav_failed", err.to_string()))?;
    }
    Ok(page)
}

#[derive(Debug)]
pub struct CdpBrowser {
    session: CdpSession,
    observer: Observer,
    applier: ActionApplier,
    timeouts: Timeouts,
    element_map: Mutex<HashMap<String, BackendNodeId>>,
    screenshot_enabled: bool,
    last_screenshot: Mutex<Option<Vec<u8>>>,
    last_screenshot_error: Mutex<Option<BrowserError>>,
}

impl CdpBrowser {
    pub async fn bootstrap(config: CdpConfig) -> BrowserResult<()> {
        let browser = Self::start(config).await?;
        browser.shutdown().await?;
        Ok(())
    }

    pub async fn launch(config: CdpConfig) -> BrowserResult<Self> {
        let session = CdpSession::launch(&config).await?;
        Ok(Self::from_session(session, &config))
    }

    pub async fn start(config: CdpConfig) -> BrowserResult<Self> {
        if let Some(url) = config.cdp_url.as_deref().filter(|value| !value.is_empty()) {
            let session = CdpSession::connect(&config, url).await?;
            Ok(Self::from_session(session, &config))
        } else {
            Self::launch(config).await
        }
    }

    fn from_session(session: CdpSession, config: &CdpConfig) -> Self {
        let observer = Observer::new(ObserverConfig {
            max_elements: config.max_elements,
            max_text_len: config.max_text_len,
        });
        Self {
            session,
            observer,
            applier: ActionApplier::new(config.max_scroll, config.max_wait_ms),
            timeouts: Timeouts {
                snapshot: config.snapshot_timeout,
                action: config.action_timeout,
            },
            element_map: Mutex::new(HashMap::new()),
            screenshot_enabled: config.screenshot_enabled,
            last_screenshot: Mutex::new(None),
            last_screenshot_error: Mutex::new(None),
        }
    }
}

#[async_trait]
impl Browser for CdpBrowser {
    async fn snapshot(&self) -> BrowserResult<Observation> {
        let page = self.session.page().await;
        let snapshot = {
            let result = timeout(self.timeouts.snapshot, self.observer.snapshot(&page)).await;
            match result {
                Ok(snapshot) => snapshot?,
                Err(err) => {
                    return Err(BrowserError::new(
                        "timeout",
                        format!("snapshot timed out: {err}"),
                    ));
                }
            }
        };

        let (screenshot, screenshot_error) = if self.screenshot_enabled {
            match capture_viewport_screenshot(&page, self.timeouts.snapshot).await {
                Ok(bytes) => (Some(bytes), None),
                Err(err) => {
                    tracing::warn!(
                        event = "screenshot_capture_failed",
                        error_code = err.code,
                        error_message = %err.message
                    );
                    (None, Some(err))
                }
            }
        } else {
            (None, None)
        };
        let mut last_screenshot = self.last_screenshot.lock().await;
        *last_screenshot = screenshot;
        let mut last_screenshot_error = self.last_screenshot_error.lock().await;
        *last_screenshot_error = screenshot_error;

        let mut map = self.element_map.lock().await;
        *map = snapshot.element_map;
        Ok(snapshot.observation)
    }

    async fn apply(&self, action: &Action) -> BrowserResult<StepResult> {
        let page = self.session.page().await;
        let map = { self.element_map.lock().await.clone() };
        let timeout_duration = match action {
            Action::Wait { ms } => {
                let wait = Duration::from_millis(*ms);
                let padded = wait.checked_add(Duration::from_millis(50)).unwrap_or(wait);
                if padded > self.timeouts.action {
                    padded
                } else {
                    self.timeouts.action
                }
            }
            _ => self.timeouts.action,
        };
        let result = timeout(
            timeout_duration,
            self.applier.apply(&page, action, Some(&map)),
        )
        .await;
        let step = match result {
            Ok(Ok(outcome)) => action_result_ok_with(outcome),
            Ok(Err(err)) => action_result_err(err),
            Err(err) => action_result_err(ActionError::new(
                "timeout",
                format!("action timed out: {err}"),
            )),
        };
        Ok(step)
    }

    async fn shutdown(&self) -> BrowserResult<()> {
        self.session.shutdown().await
    }
    async fn take_last_screenshot(&self) -> BrowserResult<ScreenshotCapture> {
        let mut bytes = self.last_screenshot.lock().await;
        let mut error = self.last_screenshot_error.lock().await;
        Ok(ScreenshotCapture {
            bytes: bytes.take(),
            error: error.take(),
        })
    }
}

impl From<chromiumoxide::error::CdpError> for BrowserError {
    fn from(err: chromiumoxide::error::CdpError) -> Self {
        BrowserError::new("cdp_error", err.to_string())
    }
}

impl Drop for CdpBrowser {
    fn drop(&mut self) {
        if self.session.closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut handler_opt) = self.session.handler_task.try_lock() else {
            return;
        };
        if let Some(handle) = handler_opt.take() {
            handle.abort();
        }
    }
}

async fn capture_viewport_screenshot(
    page: &Page,
    timeout_duration: Duration,
) -> BrowserResult<Vec<u8>> {
    let started_at = Instant::now();
    let params = ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Png)
        .build();
    let result = timeout(timeout_duration, page.screenshot(params)).await;
    let bytes = match result {
        Ok(Ok(bytes)) => {
            telemetry::record_screenshot_capture(started_at.elapsed(), bytes.len());
            bytes
        }
        Ok(Err(err)) => {
            telemetry::inc_screenshot_failure("screenshot_failed");
            return Err(BrowserError::new(
                "screenshot_failed",
                format!("screenshot capture failed: {err}"),
            ));
        }
        Err(err) => {
            telemetry::inc_screenshot_failure("screenshot_timeout");
            return Err(BrowserError::new(
                "screenshot_timeout",
                format!("screenshot timed out: {err}"),
            ));
        }
    };
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_launch_uses_explicit_executable_and_args() {
        let config = CdpConfig {
            executable_path: Some(PathBuf::from("/tmp/fake-chrome")),
            launch_timeout: Duration::from_millis(12_345),
            no_sandbox: true,
            extra_args: vec!["--alpha".to_string(), "--beta=1".to_string()],
            keep_user_data_dir: true,
            ..CdpConfig::default()
        };

        let prepared = prepare_launch(&config).expect("prepare launch");
        assert_eq!(
            prepared.details.executable_path,
            PathBuf::from("/tmp/fake-chrome")
        );
        assert_eq!(prepared.details.executable_source, "explicit");
        assert!(prepared.details.no_sandbox);
        assert_eq!(
            prepared.details.extra_args,
            vec!["--alpha".to_string(), "--beta=1".to_string()]
        );
        assert_eq!(
            prepared.details.launch_timeout,
            Duration::from_millis(12_345)
        );
        assert!(prepared.details.keep_user_data_dir);
        assert_eq!(
            prepared.browser_config.user_data_dir,
            Some(prepared.details.user_data_dir.clone())
        );

        cleanup_user_data_dir(&prepared.details.user_data_dir, false);
    }

    #[test]
    fn cleanup_user_data_dir_removes_directory_when_debug_disabled() {
        let path = unique_user_data_dir();
        std::fs::create_dir_all(&path).expect("create temp dir");
        assert!(path.exists());

        cleanup_user_data_dir(&path, false);
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_user_data_dir_keeps_directory_when_debug_enabled() {
        let path = unique_user_data_dir();
        std::fs::create_dir_all(&path).expect("create temp dir");
        assert!(path.exists());

        cleanup_user_data_dir(&path, true);
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn launch_error_includes_runtime_context() {
        let details = LaunchDetails {
            executable_path: PathBuf::from("/tmp/chrome"),
            executable_source: "explicit",
            headful: false,
            no_sandbox: true,
            extra_args: vec!["--alpha".to_string()],
            launch_timeout: Duration::from_millis(4_000),
            user_data_dir: PathBuf::from("/tmp/profile"),
            keep_user_data_dir: true,
        };

        let err = launch_error(
            "launch_process",
            &details,
            "Browser process exited before websocket URL could be resolved",
        );

        assert_eq!(err.code, "cdp_launch_failed");
        assert!(err.message.contains("stage=websocket_resolve"));
        assert!(err.message.contains("executable=/tmp/chrome"));
        assert!(err.message.contains("no_sandbox=true"));
        assert!(err.message.contains("launch_timeout_ms=4000"));
        assert!(err.message.contains("keep_user_data_dir=true"));
        assert!(err.message.contains("--alpha"));
    }
}
