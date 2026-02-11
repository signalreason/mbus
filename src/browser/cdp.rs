use crate::browser::act::{action_result_err, action_result_ok_with, ActionApplier, ActionError};
use crate::browser::observe::{Observer, ObserverConfig};
use crate::browser::{Browser, BrowserError, BrowserResult};
use crate::types::{Action, Observation, StepResult};
use async_trait::async_trait;
use chromiumoxide::browser::{Browser as ChromiumBrowser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tokio::time::timeout;
use std::time::Duration;
use chromiumoxide_cdp::cdp::browser_protocol::dom::BackendNodeId;

#[derive(Clone, Debug)]
pub struct CdpConfig {
    pub headful: bool,
    pub initial_url: String,
    pub cdp_url: Option<String>,
    pub snapshot_timeout: Duration,
    pub action_timeout: Duration,
    pub max_elements: usize,
    pub max_text_len: usize,
    pub max_scroll: i64,
}

impl Default for CdpConfig {
    fn default() -> Self {
        Self {
            headful: false,
            initial_url: "about:blank".to_string(),
            cdp_url: None,
            snapshot_timeout: Duration::from_secs(5),
            action_timeout: Duration::from_secs(10),
            max_elements: 50,
            max_text_len: 4000,
            max_scroll: 2000,
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
    closed: AtomicBool,
}

impl CdpSession {
    async fn launch(config: &CdpConfig) -> BrowserResult<Self> {
        let mut builder = BrowserConfig::builder();
        if config.headful {
            builder = builder.with_head();
        }
        let browser_config = builder
            .build()
            .map_err(|err| BrowserError::new("config_error", err))?;
        let (browser, mut handler) = ChromiumBrowser::launch(browser_config)
            .await
            .map_err(|err| BrowserError::new("cdp_launch_failed", err.to_string()))?;
        let handler_task = tokio::spawn(async move {
            while let Some(_event) = handler.next().await {}
        });
        let page = create_page(&browser, &config.initial_url).await?;

        Ok(Self {
            browser: Mutex::new(browser),
            page: Mutex::new(page),
            handler_task: Mutex::new(Some(handler_task)),
            owns_browser: true,
            closed: AtomicBool::new(false),
        })
    }

    async fn connect(config: &CdpConfig, url: &str) -> BrowserResult<Self> {
        let (browser, mut handler) = ChromiumBrowser::connect(url)
            .await
            .map_err(|err| BrowserError::new("cdp_connect_failed", err.to_string()))?;
        let handler_task = tokio::spawn(async move {
            while let Some(_event) = handler.next().await {}
        });
        let page = create_page(&browser, &config.initial_url).await?;

        Ok(Self {
            browser: Mutex::new(browser),
            page: Mutex::new(page),
            handler_task: Mutex::new(Some(handler_task)),
            owns_browser: false,
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

        Ok(())
    }

    async fn page(&self) -> Page {
        self.page.lock().await.clone()
    }
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
            applier: ActionApplier::new(config.max_scroll),
            timeouts: Timeouts {
                snapshot: config.snapshot_timeout,
                action: config.action_timeout,
            },
            element_map: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Browser for CdpBrowser {
    async fn snapshot(&self) -> BrowserResult<Observation> {
        let snapshot = {
            let page = self.session.page().await;
            let result = timeout(self.timeouts.snapshot, self.observer.snapshot(&page)).await;
            match result {
                Ok(snapshot) => snapshot?,
                Err(err) => {
                    return Err(BrowserError::new(
                        "timeout",
                        format!("snapshot timed out: {err}"),
                    ))
                }
            }
        };

        let mut map = self.element_map.lock().await;
        *map = snapshot.element_map;
        Ok(snapshot.observation)
    }

    async fn apply(&self, action: &Action) -> BrowserResult<StepResult> {
        let page = self.session.page().await;
        let map = { self.element_map.lock().await.clone() };
        let result =
            timeout(self.timeouts.action, self.applier.apply(&page, action, Some(&map))).await;
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
}

impl From<chromiumoxide::error::CdpError> for BrowserError {
    fn from(err: chromiumoxide::error::CdpError) -> Self {
        BrowserError::new("cdp_error", err.to_string())
    }
}

impl Drop for CdpBrowser {
    fn drop(&mut self) {
        if !self.session.closed.load(Ordering::SeqCst) {
            if let Ok(mut handler_opt) = self.session.handler_task.try_lock() {
                if let Some(handle) = handler_opt.take() {
                    handle.abort();
                }
            }
        }
    }
}
