use crate::browser::act::{action_result_err, action_result_ok, ActionApplier, ActionError};
use crate::browser::observe::{Observer, ObserverConfig};
use crate::browser::{Browser, BrowserError, BrowserResult};
use crate::types::{Action, Observation, StepResult};
use async_trait::async_trait;
use chromiumoxide::browser::{Browser as ChromiumBrowser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio::time::timeout;
use std::time::Duration;
use chromiumoxide_cdp::cdp::browser_protocol::dom::BackendNodeId;

#[derive(Clone, Debug)]
pub struct CdpConfig {
    pub headful: bool,
    pub initial_url: String,
    pub snapshot_timeout: Duration,
    pub action_timeout: Duration,
    pub max_elements: usize,
    pub max_text_len: usize,
}

impl Default for CdpConfig {
    fn default() -> Self {
        Self {
            headful: false,
            initial_url: "about:blank".to_string(),
            snapshot_timeout: Duration::from_secs(5),
            action_timeout: Duration::from_secs(10),
            max_elements: 50,
            max_text_len: 4000,
        }
    }
}

#[derive(Debug)]
struct Timeouts {
    snapshot: Duration,
    action: Duration,
}

#[derive(Debug)]
pub struct CdpBrowser {
    browser: Mutex<ChromiumBrowser>,
    page: Mutex<Page>,
    handler_task: tokio::task::JoinHandle<()>,
    observer: Observer,
    applier: ActionApplier,
    timeouts: Timeouts,
    element_map: Mutex<HashMap<String, BackendNodeId>>,
}

impl CdpBrowser {
    pub async fn launch(config: CdpConfig) -> BrowserResult<Self> {
        let mut builder = BrowserConfig::builder();
        if config.headful {
            builder = builder.with_head();
        }
        let browser_config = builder
            .build()
            .map_err(|err| BrowserError::new("config_error", err))?;
        let (browser, mut handler) = ChromiumBrowser::launch(browser_config).await?;
        let handler_task = tokio::spawn(async move {
            while let Some(_event) = handler.next().await {}
        });

        let page = browser.new_page("about:blank").await?;
        if !config.initial_url.is_empty() && config.initial_url != "about:blank" {
            page.goto(config.initial_url.as_str()).await?;
        }

        let observer = Observer::new(ObserverConfig {
            max_elements: config.max_elements,
            max_text_len: config.max_text_len,
        });

        Ok(Self {
            browser: Mutex::new(browser),
            page: Mutex::new(page),
            handler_task,
            observer,
            applier: ActionApplier::new(),
            timeouts: Timeouts {
                snapshot: config.snapshot_timeout,
                action: config.action_timeout,
            },
            element_map: Mutex::new(HashMap::new()),
        })
    }
}

#[async_trait]
impl Browser for CdpBrowser {
    async fn snapshot(&self) -> BrowserResult<Observation> {
        let snapshot = {
            let page = self.page.lock().await;
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
        let page = self.page.lock().await;
        let map = { self.element_map.lock().await.clone() };
        let result =
            timeout(self.timeouts.action, self.applier.apply(&page, action, Some(&map))).await;
        let step = match result {
            Ok(Ok(())) => action_result_ok(),
            Ok(Err(err)) => action_result_err(err),
            Err(err) => action_result_err(ActionError::new(
                "timeout",
                format!("action timed out: {err}"),
            )),
        };
        Ok(step)
    }

    async fn shutdown(&self) -> BrowserResult<()> {
        let mut browser = self.browser.lock().await;
        browser.close().await?;
        self.handler_task.abort();
        Ok(())
    }
}

impl From<chromiumoxide::error::CdpError> for BrowserError {
    fn from(err: chromiumoxide::error::CdpError) -> Self {
        BrowserError::new("cdp_error", err.to_string())
    }
}

impl Drop for CdpBrowser {
    fn drop(&mut self) {
        self.handler_task.abort();
    }
}
